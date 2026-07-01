use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::shell_integration;
use anyhow::Result;
use portable_pty::{
    native_pty_system, Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize,
};
use tokio::sync::broadcast;
use tracing::warn;
use wgo_daemon_core::rpc::{
    AttachTerminalSessionReq, AvailableShellInfo, CreateTerminalSessionReq, TakeTerminalControlReq,
    TakeTerminalControlRes, TerminalEvent, TerminalExit, TerminalLaunchSpec,
    TerminalSessionCloseReason, TerminalSessionInfo, TerminalSessionKey,
    TerminalSessionsTableEvent, MAX_U53,
};
use wgo_daemon_core::traits::ServiceError;

const TERMINAL_OUTPUT_RETENTION_BYTES: usize = 1024 * 1024;
const TERMINAL_EVENT_CHANNEL_CAPACITY: usize = 4096;
const TERMINAL_TABLE_CHANNEL_CAPACITY: usize = 256;
const MAX_OSC_METADATA_BYTES: usize = 4096;

type SharedSession = Arc<Mutex<TerminalSession>>;

#[derive(Clone)]
pub struct TerminalManager {
    inner: Arc<TerminalManagerInner>,
    shell_integration_dir: PathBuf,
}

struct TerminalManagerInner {
    sessions: Mutex<BTreeMap<String, SharedSession>>,
    next_session_id: AtomicU64,
    next_attach_id: AtomicU64,
    table_tx: broadcast::Sender<TerminalSessionsTableEvent>,
}

struct TerminalSession {
    info: TerminalSessionInfo,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child_killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    output_tx: broadcast::Sender<TerminalEvent>,
    output_buffer: VecDeque<OutputRecord>,
    retained_output_bytes: usize,
    metadata_parser: TerminalMetadataParser,
    attach_owners: HashMap<String, u64>,
    closed: bool,
}

#[derive(Debug, Clone)]
struct OutputRecord {
    seq: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct LaunchCommand {
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalMetadata {
    Title(String),
    Cwd(String),
}

#[derive(Debug, Clone)]
enum TerminalMetadataParserState {
    Normal,
    Escape,
    Osc { buffer: Vec<u8>, escape: bool },
}

#[derive(Debug, Clone)]
struct TerminalMetadataParser {
    state: TerminalMetadataParserState,
}

impl TerminalMetadataParser {
    fn new() -> Self {
        Self {
            state: TerminalMetadataParserState::Normal,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<TerminalMetadata> {
        let mut events = Vec::new();
        for byte in bytes {
            let mut finish_osc = false;
            match &mut self.state {
                TerminalMetadataParserState::Normal => {
                    if *byte == 0x1b {
                        self.state = TerminalMetadataParserState::Escape;
                    }
                }
                TerminalMetadataParserState::Escape => match *byte {
                    b']' => {
                        self.state = TerminalMetadataParserState::Osc {
                            buffer: Vec::new(),
                            escape: false,
                        };
                    }
                    0x1b => {}
                    _ => {
                        self.state = TerminalMetadataParserState::Normal;
                    }
                },
                TerminalMetadataParserState::Osc { buffer, escape } => {
                    if *escape {
                        if *byte == b'\\' {
                            finish_osc = true;
                        } else {
                            push_osc_byte(buffer, 0x1b);
                            push_osc_byte(buffer, *byte);
                            *escape = false;
                        }
                    } else if *byte == 0x07 {
                        finish_osc = true;
                    } else if *byte == 0x1b {
                        *escape = true;
                    } else if buffer.len() < MAX_OSC_METADATA_BYTES {
                        buffer.push(*byte);
                    } else {
                        self.state = TerminalMetadataParserState::Normal;
                    }
                }
            }
            if finish_osc {
                self.finish_osc(&mut events);
            }
        }
        events
    }

    fn finish_osc(&mut self, events: &mut Vec<TerminalMetadata>) {
        let state = std::mem::replace(&mut self.state, TerminalMetadataParserState::Normal);
        let TerminalMetadataParserState::Osc { buffer, .. } = state else {
            return;
        };
        let text = String::from_utf8_lossy(&buffer);
        if let Some(event) = parse_osc_metadata(&text) {
            events.push(event);
        }
    }
}

fn push_osc_byte(buffer: &mut Vec<u8>, byte: u8) {
    if buffer.len() < MAX_OSC_METADATA_BYTES {
        buffer.push(byte);
    }
}

fn parse_osc_metadata(text: &str) -> Option<TerminalMetadata> {
    let (kind, payload) = text.split_once(';')?;
    match kind {
        "0" | "2" => non_empty_string(payload).map(TerminalMetadata::Title),
        "7" => parse_osc7_cwd(payload).map(TerminalMetadata::Cwd),
        "633" => parse_osc633_metadata(payload),
        "9" => {
            let (subkind, cwd) = payload.split_once(';')?;
            if subkind == "9" {
                non_empty_string(strip_wrapping_quotes(cwd)).map(TerminalMetadata::Cwd)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_osc633_metadata(payload: &str) -> Option<TerminalMetadata> {
    let (kind, rest) = payload.split_once(';')?;
    if kind != "P" {
        return None;
    }
    let (name, value) = rest.split_once('=')?;
    match name {
        "Cwd" => non_empty_string(&unescape_osc633_value(value)).map(TerminalMetadata::Cwd),
        _ => None,
    }
}

fn parse_osc7_cwd(payload: &str) -> Option<String> {
    let payload = payload.trim();
    let Some(rest) = payload.strip_prefix("file://") else {
        return non_empty_string(payload);
    };
    let path = rest.find('/').map(|index| &rest[index..])?;
    let decoded = percent_decode(path);
    normalize_file_url_path(&decoded)
}

fn normalize_file_url_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    #[cfg(windows)]
    {
        let bytes = path.as_bytes();
        if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':' {
            return Some(path[1..].replace('/', "\\"));
        }
    }
    non_empty_string(path)
}

fn unescape_osc633_value(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                output.push(b'\\');
                index += 2;
                continue;
            }
            if index + 3 < bytes.len() && bytes[index + 1] == b'x' {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 2]), hex_value(bytes[index + 3]))
                {
                    output.push((high << 4) | low);
                    index += 4;
                    continue;
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn strip_wrapping_quotes(text: &str) -> &str {
    text.trim().trim_matches('"')
}

fn non_empty_string(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

impl TerminalManager {
    pub fn new(shell_integration_dir: PathBuf) -> Self {
        let (table_tx, _) = broadcast::channel(TERMINAL_TABLE_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(TerminalManagerInner {
                sessions: Mutex::new(BTreeMap::new()),
                next_session_id: AtomicU64::new(1),
                next_attach_id: AtomicU64::new(1),
                table_tx,
            }),
            shell_integration_dir,
        }
    }

    pub fn create_session(
        &self,
        request: CreateTerminalSessionReq,
        creator_client_id: String,
    ) -> Result<TerminalSessionInfo, ServiceError> {
        validate_size(request.cols, request.rows)?;
        let launch = resolve_launch(&request.launch)?;
        let spawn_launch = self.terminal_spawn_launch(&launch);
        let mut command = CommandBuilder::new(&spawn_launch.command);
        command.args(&spawn_launch.args);
        for (key, value) in &spawn_launch.env {
            command.env(key, value);
        }
        let initial_cwd = request
            .cwd
            .clone()
            .or_else(|| launch.cwd.clone())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned())
            });
        if let Some(cwd) = initial_cwd.as_deref() {
            command.cwd(PathBuf::from(cwd));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: request.rows as u16,
                cols: request.cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(operation_failed)?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(operation_failed)?;
        let child_killer = child.clone_killer();
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(operation_failed)?;
        let writer = pair.master.take_writer().map_err(operation_failed)?;
        let terminal_session_id = format!(
            "terminal-{}",
            self.inner.next_session_id.fetch_add(1, Ordering::Relaxed)
        );
        let now = current_unix_ms();
        let (output_tx, _) = broadcast::channel(TERMINAL_EVENT_CHANNEL_CAPACITY);
        let info = TerminalSessionInfo {
            terminal_session_id: terminal_session_id.clone(),
            creator_client_id,
            created_at_ms: now,
            last_attached_at_ms: None,
            last_detached_at_ms: None,
            last_output_at_ms: None,
            cols: request.cols,
            rows: request.rows,
            primary_attach_id: None,
            latest_output_seq: 0,
            last_known_title: request.title.clone(),
            exit: None,
            last_known_cwd: initial_cwd,
            launch: request.launch.clone(),
        };
        let session = Arc::new(Mutex::new(TerminalSession {
            info: info.clone(),
            master: pair.master,
            writer,
            child_killer: Some(child_killer),
            output_tx,
            output_buffer: VecDeque::new(),
            retained_output_bytes: 0,
            metadata_parser: TerminalMetadataParser::new(),
            attach_owners: HashMap::new(),
            closed: false,
        }));

        self.inner
            .sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .insert(terminal_session_id.clone(), session.clone());
        self.broadcast_table_upsert(&info);
        spawn_output_reader(self.clone(), terminal_session_id.clone(), reader);
        spawn_child_waiter(self.clone(), terminal_session_id.clone(), child);
        Ok(info)
    }

    pub fn subscribe_sessions(&self) -> broadcast::Receiver<TerminalSessionsTableEvent> {
        self.inner.table_tx.subscribe()
    }

    pub fn sessions_snapshot(&self) -> Vec<TerminalSessionInfo> {
        self.inner
            .sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .values()
            .map(|session| {
                session
                    .lock()
                    .expect("terminal session lock poisoned")
                    .info
                    .clone()
            })
            .collect()
    }

    pub fn available_shells_snapshot(&self) -> Vec<AvailableShellInfo> {
        discover_available_shells()
    }

    pub fn attach(
        &self,
        request: AttachTerminalSessionReq,
        rpc_session_id: u64,
    ) -> Result<AttachedTerminal, ServiceError> {
        validate_size(request.viewport_cols, request.viewport_rows)?;
        let session = self.get_session(&request.terminal_session_id)?;
        let attach_id = format!(
            "attach-{}",
            self.inner.next_attach_id.fetch_add(1, Ordering::Relaxed)
        );
        let now = current_unix_ms();
        let mut guard = session.lock().expect("terminal session lock poisoned");
        if guard.closed {
            return Err(ServiceError::NotFound);
        }
        guard
            .attach_owners
            .insert(attach_id.clone(), rpc_session_id);
        guard.info.last_attached_at_ms = Some(now);
        let info = guard.info.clone();
        let primary_attach_id = guard.info.primary_attach_id.clone();
        let replay = replay_events(&guard, request.after_seq);
        let receiver = guard.output_tx.subscribe();
        drop(guard);
        self.broadcast_table_upsert(&info);
        Ok(AttachedTerminal {
            terminal_session_id: request.terminal_session_id,
            attach_id,
            primary_attach_id,
            session: info,
            replay,
            receiver,
        })
    }

    pub fn detach(&self, terminal_session_id: &str, attach_id: &str) {
        let Ok(session) = self.get_session(terminal_session_id) else {
            return;
        };
        let mut guard = session.lock().expect("terminal session lock poisoned");
        guard.attach_owners.remove(attach_id);
        guard.info.last_detached_at_ms = Some(current_unix_ms());
        let info = guard.info.clone();
        drop(guard);
        self.broadcast_table_upsert(&info);
    }

    pub fn take_control(
        &self,
        request: TakeTerminalControlReq,
        rpc_session_id: u64,
    ) -> Result<TakeTerminalControlRes, ServiceError> {
        validate_size(request.viewport_cols, request.viewport_rows)?;
        let session = self.get_session(&request.terminal_session_id)?;
        let mut guard = session.lock().expect("terminal session lock poisoned");
        validate_attach_owner(&guard, &request.attach_id, rpc_session_id)?;

        let previous_primary = guard.info.primary_attach_id.clone();
        let previous_size = (guard.info.cols, guard.info.rows);
        guard.info.primary_attach_id = Some(request.attach_id.clone());
        guard.info.cols = request.viewport_cols;
        guard.info.rows = request.viewport_rows;
        guard
            .master
            .resize(PtySize {
                rows: request.viewport_rows as u16,
                cols: request.viewport_cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(operation_failed)?;

        let info = guard.info.clone();
        if previous_primary.as_deref() != Some(request.attach_id.as_str()) {
            let _ = guard.output_tx.send(TerminalEvent::ControlChanged {
                primary_attach_id: request.attach_id.clone(),
            });
        }
        if previous_size != (request.viewport_cols, request.viewport_rows) {
            let _ = guard.output_tx.send(TerminalEvent::PseudoTerminalResized {
                cols: request.viewport_cols,
                rows: request.viewport_rows,
            });
        }
        drop(guard);
        self.broadcast_table_upsert(&info);
        Ok(TakeTerminalControlRes {
            primary_attach_id: request.attach_id,
        })
    }

    pub fn write_input(
        &self,
        terminal_session_id: &str,
        attach_id: &str,
        bytes: &[u8],
        rpc_session_id: u64,
    ) -> Result<(), ServiceError> {
        let session = self.get_session(terminal_session_id)?;
        let mut guard = session.lock().expect("terminal session lock poisoned");
        validate_attach_owner(&guard, attach_id, rpc_session_id)?;
        if guard.info.primary_attach_id.as_deref() != Some(attach_id) {
            return Err(ServiceError::OperationFailed(
                "attach is not the live primary attach".to_string(),
            ));
        }
        guard.writer.write_all(bytes).map_err(operation_failed)?;
        guard.writer.flush().map_err(operation_failed)?;
        Ok(())
    }

    pub fn close_session(&self, terminal_session_id: &str) -> Result<(), ServiceError> {
        let session = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .expect("terminal sessions lock poisoned");
            sessions
                .remove(terminal_session_id)
                .ok_or(ServiceError::NotFound)?
        };

        let mut guard = session.lock().expect("terminal session lock poisoned");
        guard.closed = true;
        if let Some(mut child_killer) = guard.child_killer.take() {
            if guard.info.exit.is_none() {
                if let Err(err) = child_killer.kill() {
                    warn!(?err, terminal_session_id, "failed to kill terminal child");
                }
            }
        }
        let _ = guard.output_tx.send(TerminalEvent::SessionClosed {
            reason: TerminalSessionCloseReason::ClosedByClient,
        });
        drop(guard);
        self.broadcast_table_remove(terminal_session_id);
        Ok(())
    }

    fn record_output(&self, terminal_session_id: &str, bytes: Vec<u8>) -> bool {
        let Ok(session) = self.get_session(terminal_session_id) else {
            return false;
        };
        let mut guard = session.lock().expect("terminal session lock poisoned");
        if guard.closed || bytes.is_empty() {
            return false;
        }
        let metadata = guard.metadata_parser.push(&bytes);
        let seq = guard.info.latest_output_seq.saturating_add(1);
        guard.info.latest_output_seq = seq;
        guard.info.last_output_at_ms = Some(current_unix_ms());
        let mut metadata_changed = false;
        for event in &metadata {
            match event {
                TerminalMetadata::Title(title) => {
                    if guard.info.last_known_title.as_deref() != Some(title) {
                        guard.info.last_known_title = Some(title.clone());
                        metadata_changed = true;
                    }
                }
                TerminalMetadata::Cwd(cwd) => {
                    if guard.info.last_known_cwd.as_deref() != Some(cwd) {
                        guard.info.last_known_cwd = Some(cwd.clone());
                        metadata_changed = true;
                    }
                }
            }
        }
        let updated_info = metadata_changed.then(|| guard.info.clone());
        guard.retained_output_bytes += bytes.len();
        guard.output_buffer.push_back(OutputRecord {
            seq,
            bytes: bytes.clone(),
        });
        while guard.retained_output_bytes > TERMINAL_OUTPUT_RETENTION_BYTES {
            let Some(record) = guard.output_buffer.pop_front() else {
                break;
            };
            guard.retained_output_bytes = guard
                .retained_output_bytes
                .saturating_sub(record.bytes.len());
        }
        let _ = guard
            .output_tx
            .send(TerminalEvent::OutputChunk { seq, bytes });
        for event in metadata {
            let _ = match event {
                TerminalMetadata::Title(title) => {
                    guard.output_tx.send(TerminalEvent::TitleChanged { title })
                }
                TerminalMetadata::Cwd(cwd) => guard
                    .output_tx
                    .send(TerminalEvent::WorkingDirectoryChanged { cwd }),
            };
        }
        drop(guard);
        if let Some(info) = updated_info {
            self.broadcast_table_upsert(&info);
        }
        true
    }

    fn mark_exited(&self, terminal_session_id: &str, status: Option<ExitStatus>) {
        let Ok(session) = self.get_session(terminal_session_id) else {
            return;
        };
        let mut guard = session.lock().expect("terminal session lock poisoned");
        if guard.closed || guard.info.exit.is_some() {
            return;
        }
        guard.child_killer = None;
        let exit = TerminalExit {
            code: status.as_ref().map(|status| i64::from(status.exit_code())),
            signal: status
                .as_ref()
                .and_then(|status| status.signal().map(ToString::to_string)),
            exited_at_ms: current_unix_ms(),
        };
        guard.info.exit = Some(exit.clone());
        let info = guard.info.clone();
        let _ = guard.output_tx.send(TerminalEvent::SessionExited { exit });
        drop(guard);
        self.broadcast_table_upsert(&info);
    }

    fn get_session(&self, terminal_session_id: &str) -> Result<SharedSession, ServiceError> {
        self.inner
            .sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .get(terminal_session_id)
            .cloned()
            .ok_or(ServiceError::NotFound)
    }

    fn broadcast_table_upsert(&self, info: &TerminalSessionInfo) {
        let _ = self.inner.table_tx.send(TerminalSessionsTableEvent::Patch {
            removes: Vec::new(),
            upserts: vec![info.clone()],
        });
    }

    fn broadcast_table_remove(&self, terminal_session_id: &str) {
        let _ = self.inner.table_tx.send(TerminalSessionsTableEvent::Patch {
            removes: vec![TerminalSessionKey {
                terminal_session_id: terminal_session_id.to_string(),
            }],
            upserts: Vec::new(),
        });
    }
}

pub struct AttachedTerminal {
    pub terminal_session_id: String,
    pub attach_id: String,
    pub primary_attach_id: Option<String>,
    pub session: TerminalSessionInfo,
    pub replay: Vec<TerminalEvent>,
    pub receiver: broadcast::Receiver<TerminalEvent>,
}

fn spawn_output_reader(
    manager: TerminalManager,
    terminal_session_id: String,
    mut reader: Box<dyn Read + Send>,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if !manager.record_output(&terminal_session_id, buffer[..count].to_vec()) {
                        break;
                    }
                }
                Err(err) => {
                    warn!(?err, terminal_session_id, "terminal output reader failed");
                    break;
                }
            }
        }
    });
}

fn spawn_child_waiter(
    manager: TerminalManager,
    terminal_session_id: String,
    mut child: Box<dyn Child + Send + Sync>,
) {
    thread::spawn(move || {
        let status = match child.wait() {
            Ok(status) => Some(status),
            Err(err) => {
                warn!(
                    ?err,
                    terminal_session_id, "failed to wait for terminal child"
                );
                None
            }
        };
        manager.mark_exited(&terminal_session_id, status);
    });
}

fn replay_events(session: &TerminalSession, after_seq: Option<u64>) -> Vec<TerminalEvent> {
    let Some(first_record) = session.output_buffer.front() else {
        return Vec::new();
    };
    let mut events = Vec::new();
    let min_requested_seq = after_seq.map_or(first_record.seq, |seq| seq.saturating_add(1));
    if min_requested_seq < first_record.seq {
        events.push(TerminalEvent::HistoryGap {
            next_seq: first_record.seq,
        });
    }
    events.extend(
        session
            .output_buffer
            .iter()
            .filter(|record| after_seq.map_or(true, |seq| record.seq > seq))
            .map(|record| TerminalEvent::OutputChunk {
                seq: record.seq,
                bytes: record.bytes.clone(),
            }),
    );
    events
}

fn validate_attach_owner(
    session: &TerminalSession,
    attach_id: &str,
    rpc_session_id: u64,
) -> Result<(), ServiceError> {
    match session.attach_owners.get(attach_id) {
        Some(owner) if *owner == rpc_session_id => Ok(()),
        Some(_) => Err(ServiceError::PermissionDenied),
        None => Err(ServiceError::OperationFailed(
            "attach not found".to_string(),
        )),
    }
}

fn validate_size(cols: u64, rows: u64) -> Result<(), ServiceError> {
    if cols == 0 || rows == 0 || cols > u64::from(u16::MAX) || rows > u64::from(u16::MAX) {
        return Err(ServiceError::OperationFailed(
            "terminal size is invalid".to_string(),
        ));
    }
    Ok(())
}

fn resolve_launch(launch: &TerminalLaunchSpec) -> Result<LaunchCommand, ServiceError> {
    if launch.command.trim().is_empty() {
        return Err(ServiceError::OperationFailed(
            "terminal command is empty".to_string(),
        ));
    }
    Ok(LaunchCommand {
        command: launch.command.clone(),
        args: launch.args.clone(),
        cwd: None,
        env: Vec::new(),
    })
}

impl TerminalManager {
    fn terminal_spawn_launch(&self, launch: &LaunchCommand) -> LaunchCommand {
        terminal_spawn_launch(launch, &self.shell_integration_dir)
    }
}

fn terminal_spawn_launch(launch: &LaunchCommand, shell_integration_dir: &Path) -> LaunchCommand {
    let integrated = shell_integration::integrate_shell_launch(
        shell_integration_dir,
        &launch.command,
        &launch.args,
    );
    let launch = LaunchCommand {
        command: integrated.command,
        args: integrated.args,
        cwd: launch.cwd.clone(),
        env: integrated.env,
    };

    #[cfg(target_os = "macos")]
    if should_spawn_terminal_as_macos_console_user() {
        if let Some(user) = macos_console_user_name() {
            let mut args = vec![
                "-H".to_string(),
                "-u".to_string(),
                user,
                "--".to_string(),
                launch.command.clone(),
            ];
            args.extend(launch.args.clone());
            return LaunchCommand {
                command: "/usr/bin/sudo".to_string(),
                args,
                cwd: launch.cwd.clone(),
                env: launch.env.clone(),
            };
        }
    }

    launch
}

#[cfg(target_os = "macos")]
fn should_spawn_terminal_as_macos_console_user() -> bool {
    current_effective_uid().as_deref() == Some("0")
}

#[cfg(target_os = "macos")]
fn current_effective_uid() -> Option<String> {
    trimmed_command_output("/usr/bin/id", &["-u"])
}

fn discover_available_shells() -> Vec<AvailableShellInfo> {
    let mut shells = Vec::new();
    #[cfg(windows)]
    discover_windows_shells(&mut shells);
    #[cfg(not(windows))]
    discover_unix_shells(&mut shells);
    dedupe_shells(shells)
}

#[cfg(windows)]
fn discover_windows_shells(shells: &mut Vec<AvailableShellInfo>) {
    if let Some(shell) = discover_windows_terminal_default_shell() {
        shells.push(shell);
    }
    if let Some(path) = find_on_path("pwsh.exe") {
        shells.push(shell_info(
            "pwsh",
            "PowerShell 7",
            path,
            Vec::new(),
            shells.is_empty(),
        ));
    }
    if let Some(path) = find_on_path("powershell.exe") {
        shells.push(shell_info(
            "windows-powershell",
            "Windows PowerShell",
            path,
            Vec::new(),
            shells.is_empty(),
        ));
    }
    let cmd = std::env::var("COMSPEC")
        .ok()
        .or_else(|| find_on_path("cmd.exe"))
        .unwrap_or_else(|| "cmd.exe".to_string());
    shells.push(shell_info(
        "cmd",
        "Command Prompt",
        cmd,
        Vec::new(),
        shells.is_empty(),
    ));
    if let Some(path) = find_on_path("bash.exe") {
        shells.push(shell_info("bash", "Bash", path, Vec::new(), false));
    }
}

#[cfg(windows)]
fn discover_windows_terminal_default_shell() -> Option<AvailableShellInfo> {
    windows_terminal_settings_paths()
        .into_iter()
        .find_map(|path| read_windows_terminal_default_shell(&path))
}

#[cfg(windows)]
fn windows_terminal_settings_paths() -> Vec<PathBuf> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    let local_app_data = PathBuf::from(local_app_data);
    vec![
        local_app_data
            .join("Packages")
            .join("Microsoft.WindowsTerminal_8wekyb3d8bbwe")
            .join("LocalState")
            .join("settings.json"),
        local_app_data
            .join("Packages")
            .join("Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe")
            .join("LocalState")
            .join("settings.json"),
        local_app_data
            .join("Packages")
            .join("Microsoft.WindowsTerminalCanary_8wekyb3d8bbwe")
            .join("LocalState")
            .join("settings.json"),
        local_app_data
            .join("Microsoft")
            .join("Windows Terminal")
            .join("settings.json"),
    ]
}

#[cfg(windows)]
fn read_windows_terminal_default_shell(path: &Path) -> Option<AvailableShellInfo> {
    let text = std::fs::read_to_string(path).ok()?;
    let value = jsonc_parser::parse_to_value(&text, &Default::default())
        .ok()
        .flatten()?;
    let jsonc_parser::JsonValue::Object(root) = value else {
        return None;
    };
    let default_profile = normalize_windows_terminal_guid(root.get_string("defaultProfile")?);
    let profiles = root.get_object("profiles")?;
    let profile_list = profiles.get_array("list")?;

    for profile_value in profile_list.iter() {
        let jsonc_parser::JsonValue::Object(profile) = profile_value else {
            continue;
        };
        let Some(guid) = profile.get_string("guid") else {
            continue;
        };
        if normalize_windows_terminal_guid(guid) != default_profile {
            continue;
        }

        let name = profile.get_string("name").map(|name| name.trim());
        if let Some(command_line) = profile.get_string("commandline") {
            let command_line = command_line.trim();
            if !command_line.is_empty() {
                return windows_terminal_profile_shell_info(name, command_line);
            }
        }
        if let Some(source) = profile.get_string("source") {
            return windows_terminal_source_shell_info(name, source.trim());
        }
        return None;
    }

    None
}

#[cfg(windows)]
fn windows_terminal_source_shell_info(
    profile_name: Option<&str>,
    source: &str,
) -> Option<AvailableShellInfo> {
    match source {
        "Git" => {
            let command = find_git_bash_command()?;
            let name = profile_name
                .filter(|name| !name.is_empty())
                .unwrap_or("Git Bash");
            Some(shell_info(
                "bash",
                name,
                command,
                vec!["--login".to_string(), "-i".to_string()],
                true,
            ))
        }
        _ => None,
    }
}

#[cfg(windows)]
fn find_git_bash_command() -> Option<String> {
    git_bash_candidate_paths()
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| find_on_path("bash.exe"))
}

#[cfg(windows)]
fn git_bash_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        paths.push(
            PathBuf::from(program_files)
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        paths.push(
            PathBuf::from(program_files_x86)
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        paths.push(
            PathBuf::from(local_app_data)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    paths
}

#[cfg(windows)]
fn windows_terminal_profile_shell_info(
    profile_name: Option<&str>,
    command_line: &str,
) -> Option<AvailableShellInfo> {
    let mut args = split_windows_command_line(command_line)
        .into_iter()
        .map(|arg| expand_windows_env_vars(&arg))
        .collect::<Vec<_>>();
    if args.is_empty() {
        return None;
    }
    let command = args.remove(0);
    let name = profile_name
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| command_file_name(&command))
        .unwrap_or_else(|| "Windows Terminal default".to_string());
    let shell_id = windows_terminal_shell_id(&command);
    Some(shell_info(&shell_id, &name, command, args, true))
}

#[cfg(windows)]
fn normalize_windows_terminal_guid(guid: &str) -> String {
    guid.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn windows_terminal_shell_id(command: &str) -> String {
    let Some(file_name) = command_file_name(command) else {
        return "windows-terminal-default".to_string();
    };
    match file_name.to_ascii_lowercase().as_str() {
        "pwsh.exe" => "pwsh".to_string(),
        "powershell.exe" => "windows-powershell".to_string(),
        "cmd.exe" => "cmd".to_string(),
        "bash.exe" => "bash".to_string(),
        _ => "windows-terminal-default".to_string(),
    }
}

#[cfg(windows)]
fn command_file_name(command: &str) -> Option<String> {
    PathBuf::from(command)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

#[cfg(windows)]
fn expand_windows_env_vars(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after_start[..end];
        if name.is_empty() {
            output.push_str("%%");
        } else if let Ok(value) = std::env::var(name) {
            output.push_str(&value);
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

#[cfg(windows)]
fn split_windows_command_line(command_line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in command_line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

#[cfg(not(windows))]
fn discover_unix_shells(shells: &mut Vec<AvailableShellInfo>) {
    let default_shell = default_unix_shell();
    let mut paths = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/etc/shells") {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            paths.push(line.to_string());
        }
    }
    if let Some(shell) = &default_shell {
        paths.insert(0, shell.clone());
    }
    for path in paths {
        let name = PathBuf::from(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path.as_str())
            .to_string();
        let id = format!("path:{path}");
        shells.push(shell_info(
            &id,
            &name,
            path.clone(),
            Vec::new(),
            default_shell.as_deref() == Some(path.as_str()) || shells.is_empty(),
        ));
    }
}

#[cfg(not(windows))]
fn default_unix_shell() -> Option<String> {
    #[cfg(target_os = "macos")]
    if let Some(shell) = macos_console_user_shell() {
        return Some(shell);
    }

    std::env::var("SHELL")
        .ok()
        .map(|shell| shell.trim().to_string())
        .filter(|shell| !shell.is_empty())
}

#[cfg(target_os = "macos")]
fn macos_console_user_shell() -> Option<String> {
    let user = macos_console_user_name()?;

    let output = std::process::Command::new("/usr/bin/dscl")
        .args([".", "-read", &format!("/Users/{user}"), "UserShell"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_dscl_user_shell(&String::from_utf8(output.stdout).ok()?)
}

#[cfg(target_os = "macos")]
fn parse_dscl_user_shell(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("UserShell:"))
        .map(str::trim)
        .filter(|shell| !shell.is_empty())
        .map(str::to_string)
}

#[cfg(target_os = "macos")]
fn macos_console_user_name() -> Option<String> {
    trimmed_command_output("/usr/bin/stat", &["-f", "%Su", "/dev/console"])
        .filter(|user| user != "root")
}

#[cfg(target_os = "macos")]
fn trimmed_command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn shell_info(
    shell_id: &str,
    name: &str,
    command: String,
    args: Vec<String>,
    is_default: bool,
) -> AvailableShellInfo {
    AvailableShellInfo {
        shell_id: shell_id.to_string(),
        name: name.to_string(),
        command,
        args,
        is_default,
    }
}

fn dedupe_shells(shells: Vec<AvailableShellInfo>) -> Vec<AvailableShellInfo> {
    let mut seen = std::collections::HashSet::new();
    let mut output = Vec::new();
    let mut has_default = false;
    for mut shell in shells {
        let key = shell.shell_id.clone();
        if !seen.insert(key) {
            continue;
        }
        if shell.is_default {
            if has_default {
                shell.is_default = false;
            } else {
                has_default = true;
            }
        }
        output.push(shell);
    }
    if !has_default {
        if let Some(shell) = output.first_mut() {
            shell.is_default = true;
        }
    }
    output
}

#[cfg(windows)]
fn find_on_path(binary: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(binary))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn current_unix_ms() -> u64 {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    let millis = duration.as_millis();
    if millis > u128::from(MAX_U53) {
        MAX_U53
    } else {
        millis as u64
    }
}

fn operation_failed(err: impl std::fmt::Display) -> ServiceError {
    ServiceError::OperationFailed(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_osc_metadata, TerminalMetadata};

    #[cfg(target_os = "macos")]
    use super::parse_dscl_user_shell;

    #[test]
    fn parses_osc633_cwd_property() {
        assert_eq!(
            parse_osc_metadata("633;P;Cwd=C:\\\\Users\\\\user\\\\repo"),
            Some(TerminalMetadata::Cwd("C:\\Users\\user\\repo".to_string()))
        );
    }

    #[test]
    fn parses_osc633_escaped_cwd_property() {
        assert_eq!(
            parse_osc_metadata("633;P;Cwd=/tmp/has\\x3bsemi"),
            Some(TerminalMetadata::Cwd("/tmp/has;semi".to_string()))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_macos_user_shell_from_dscl_output() {
        assert_eq!(
            parse_dscl_user_shell("UserShell: /bin/zsh\n"),
            Some("/bin/zsh".to_string())
        );
    }
}
