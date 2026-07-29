use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::schema::v1::{
    ContentBlock as AcpContentBlock, SessionConfigKind as AcpSessionConfigKind,
    SessionConfigOption as AcpSessionConfigOption,
    SessionConfigOptionCategory as AcpSessionConfigOptionCategory,
    SessionConfigOptionValue as AcpSessionConfigOptionValue,
    SessionConfigSelectOptions as AcpSessionConfigSelectOptions, StopReason as AcpStopReason,
    TextContent as AcpTextContent,
};
use anyhow::Result;
use rieul_daemon_core::config::{AgentServerConfig, SystemConfig};
use rieul_daemon_core::generated::rpc::{
    AgentAttachmentState, AgentConfigInput, AgentConfigOption, AgentConfigOptionCategory,
    AgentConfigSelectGroup, AgentConfigSelectOption, AgentConfigValue, AgentContent, AgentFailure,
    AgentMessage, AgentMessageRole, AgentMessageState, AgentProjectAvailability, AgentProjectInfo,
    AgentProjectsTableEvent, AgentProviderAuthentication, AgentProviderAvailability,
    AgentProviderCapabilities, AgentProviderInfo, AgentProvidersTableEvent,
    AgentSessionArchiveFilter, AgentSessionEvent, AgentSessionInfo, AgentSessionLiveSnapshot,
    AgentSessionRecoverability, AgentSessionSummary, AgentSessionTitleUpdate,
    AgentSessionTurnState, AgentSessionWorkspaceFilter, AgentStopReason, AgentTaskWorkspaceSource,
    AgentTaskWorkspaceState, AgentTurnInfo, AgentTurnRecord, AgentTurnState, AgentUsage,
    AgentWorkspaceBinding, CreateAgentProjectReq, CreateAgentSessionReq, CreateAgentTurnReq,
    CreateAgentWorkspace, ListAgentSessionTurnsReq, ListAgentSessionTurnsRes, ListAgentSessionsReq,
    ListAgentSessionsRes, SetAgentSessionConfigReq, SetAgentSessionConfigRes,
    UpdateAgentSessionReq,
};
use tokio::sync::{broadcast, mpsc, watch};

use crate::agent_runtime::{
    start_agent_runtime, AgentRuntimeConfig, AgentRuntimeEvent, AgentRuntimeHandle,
    AgentRuntimeSession,
};
use crate::state_db::{
    DaemonStateDb, NewAgentSession, NewAgentTaskWorkspace, StoredAgentArchiveFilter,
    StoredAgentConfigValue, StoredAgentMessage, StoredAgentMessageContent, StoredAgentProject,
    StoredAgentSession, StoredAgentSessionQuery, StoredAgentTurn, StoredAgentTurnRecord,
    StoredAgentWorkspaceFilter, UpdatedAgentSession,
};

const MAX_SESSION_PAGE_SIZE: usize = 100;
const MAX_SESSION_HISTORY_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentErrorKind {
    Failed,
    NotFound,
    InvalidArgument,
    Conflict,
    Unavailable,
    PermissionDenied,
}

#[derive(Debug)]
pub struct AgentError {
    pub kind: AgentErrorKind,
    pub message: String,
}

impl AgentError {
    fn new(kind: AgentErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::Failed, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::NotFound, message)
    }

    fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::InvalidArgument, message)
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::Unavailable, message)
    }

    fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::PermissionDenied, message)
    }
}

#[derive(Clone)]
pub struct AgentManager {
    db: Arc<StdMutex<DaemonStateDb>>,
    task_workspace_root: PathBuf,
    project_events: watch::Sender<u64>,
    catalog_events: watch::Sender<u64>,
    runtimes: Arc<StdMutex<HashMap<String, AgentRuntimeEntry>>>,
    attachment_locks: Arc<StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

#[derive(Clone)]
struct AgentRuntimeEntry {
    handle: AgentRuntimeHandle,
    live: Arc<AgentLiveSession>,
}

struct AgentLiveSession {
    snapshot: StdMutex<AgentSessionLiveSnapshot>,
    state_update: StdMutex<()>,
    config_changing: AtomicBool,
    events: broadcast::Sender<AgentSessionEvent>,
}

struct AgentConfigChangeGuard<'a>(&'a AtomicBool);

impl<'a> AgentConfigChangeGuard<'a> {
    fn begin(changing: &'a AtomicBool) -> Result<Self, AgentError> {
        changing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                AgentError::new(
                    AgentErrorKind::Conflict,
                    "agent session configuration is already changing",
                )
            })?;
        Ok(Self(changing))
    }
}

impl Drop for AgentConfigChangeGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub struct AgentSessionSubscription {
    pub snapshot: AgentSessionLiveSnapshot,
    pub events: Option<broadcast::Receiver<AgentSessionEvent>>,
}

impl AgentManager {
    pub fn open(db: DaemonStateDb, task_workspace_root: PathBuf) -> Result<Self> {
        let catalog_revision = db.agent_session_catalog_revision()?;
        let (project_events, _) = watch::channel(0);
        let (catalog_events, _) = watch::channel(catalog_revision);
        Ok(Self {
            db: Arc::new(StdMutex::new(db)),
            task_workspace_root,
            project_events,
            catalog_events,
            runtimes: Arc::new(StdMutex::new(HashMap::new())),
            attachment_locks: Arc::new(StdMutex::new(HashMap::new())),
        })
    }

    pub fn subscribe_project_events(&self) -> watch::Receiver<u64> {
        self.project_events.subscribe()
    }

    pub fn subscribe_catalog_events(&self) -> watch::Receiver<u64> {
        self.catalog_events.subscribe()
    }

    pub fn projects_snapshot(&self) -> Result<Vec<AgentProjectInfo>, AgentError> {
        self.with_db(|db| db.load_agent_projects())
            .map(|projects| projects.into_iter().map(project_info).collect())
    }

    pub fn create_project(
        &self,
        request: CreateAgentProjectReq,
    ) -> Result<AgentProjectInfo, AgentError> {
        let root_path = canonical_project_path(&request.root_path)?;
        if let Some(existing) = self.with_db(|db| db.find_agent_project_by_root_path(&root_path))? {
            return Ok(project_info(existing));
        }
        let title = request
            .title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| default_project_title(&root_path));
        let project = StoredAgentProject {
            project_id: random_id("project"),
            title,
            root_path,
            created_at_ms: current_unix_ms(),
            last_opened_at_ms: None,
        };
        self.with_db(|db| db.insert_agent_project(&project))?;
        notify_watch(&self.project_events);
        Ok(project_info(project))
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), AgentError> {
        if project_id.is_empty() {
            return Err(AgentError::invalid_argument("projectId must not be empty"));
        }
        let removed = self.with_db(|db| db.remove_agent_project(project_id))?;
        if !removed {
            return Err(AgentError::not_found("agent project was not found"));
        }
        notify_watch(&self.project_events);
        Ok(())
    }

    pub fn create_session(
        &self,
        request: CreateAgentSessionReq,
        configured_provider_ids: &HashSet<String>,
    ) -> Result<AgentSessionInfo, AgentError> {
        if request.creation_request_id.is_empty() {
            return Err(AgentError::invalid_argument(
                "creationRequestId must not be empty",
            ));
        }
        if let Some(existing) = self
            .with_db(|db| db.find_agent_session_by_creation_request(&request.creation_request_id))?
        {
            return if session_matches_create_request(&existing, &request) {
                Ok(session_info(existing))
            } else {
                Err(AgentError::new(
                    AgentErrorKind::Conflict,
                    "creationRequestId was already used for a different session creation request",
                ))
            };
        }
        if !configured_provider_ids.contains(&request.provider_id) {
            return Err(AgentError::invalid_argument(
                "providerId is not configured in agentServers",
            ));
        }

        let creation_request = request.clone();
        let now = current_unix_ms();
        let session_id = random_id("session");
        let (workspace_kind, project_id, task_workspace_id, cwd, task_workspace) =
            match request.workspace {
                CreateAgentWorkspace::Project { project_id } => {
                    let project = self
                        .with_db(|db| db.find_agent_project_by_id(&project_id))?
                        .ok_or_else(|| AgentError::not_found("agent project was not found"))?;
                    if !Path::new(&project.root_path).is_dir() {
                        return Err(AgentError::unavailable(
                            "agent project directory is unavailable",
                        ));
                    }
                    (
                        "project".to_string(),
                        Some(project_id),
                        None,
                        project.root_path,
                        None,
                    )
                }
                CreateAgentWorkspace::Task {
                    source: AgentTaskWorkspaceSource::Empty,
                } => {
                    let task_workspace_id = random_id("task");
                    let root = self.task_workspace_root.join(&task_workspace_id);
                    fs::create_dir_all(&root).map_err(|error| {
                        AgentError::failed(format!("create task workspace: {error}"))
                    })?;
                    let root_path = path_text(&root);
                    let workspace = NewAgentTaskWorkspace {
                        task_workspace_id: task_workspace_id.clone(),
                        root_path: root_path.clone(),
                        source_kind: "empty".to_string(),
                        source_project_id: None,
                        git_base_ref: None,
                        copy_include_untracked: None,
                        state_kind: "ready".to_string(),
                        created_at_ms: now,
                        updated_at_ms: now,
                    };
                    (
                        "task".to_string(),
                        None,
                        Some(task_workspace_id),
                        root_path,
                        Some(workspace),
                    )
                }
                CreateAgentWorkspace::Task { .. } => {
                    return Err(AgentError::invalid_argument(
                        "project-based task workspaces are not implemented yet",
                    ));
                }
            };
        let session = NewAgentSession {
            session_id,
            provider_id: request.provider_id,
            title: request.title,
            workspace_kind,
            project_id,
            task_workspace_id,
            cwd,
            creation_request_id: request.creation_request_id,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let created = session.clone();
        let revision = match self
            .with_db_mut(|db| db.create_agent_session(&session, task_workspace.as_ref()))
        {
            Ok(revision) => revision,
            Err(error) => {
                if let Some(workspace) = &task_workspace {
                    let _ = fs::remove_dir(&workspace.root_path);
                }
                if let Some(existing) = self.with_db(|db| {
                    db.find_agent_session_by_creation_request(&session.creation_request_id)
                })? {
                    return if session_matches_create_request(&existing, &creation_request) {
                        Ok(session_info(existing))
                    } else {
                        Err(AgentError::new(
                            AgentErrorKind::Conflict,
                            "creationRequestId was already used for a different session creation request",
                        ))
                    };
                }
                return Err(error);
            }
        };
        let _ = self.catalog_events.send(revision);
        Ok(session_info(StoredAgentSession {
            session_id: created.session_id,
            provider_id: created.provider_id,
            provider_session_id: None,
            title: created.title,
            workspace_kind: created.workspace_kind,
            project_id: created.project_id,
            task_workspace_id: created.task_workspace_id,
            task_source_project_id: None,
            task_state_kind: task_workspace.map(|workspace| workspace.state_kind),
            cwd: created.cwd,
            archived: false,
            latest_seq: 0,
            last_message_preview: None,
            created_at_ms: created.created_at_ms,
            updated_at_ms: created.updated_at_ms,
            active_turn_state_kind: None,
        }))
    }

    pub async fn create_and_attach_session(
        &self,
        request: CreateAgentSessionReq,
        provider: AgentServerConfig,
    ) -> Result<AgentSessionInfo, AgentError> {
        let provider_id = request.provider_id.clone();
        let persisted = self.create_session(request, &HashSet::from([provider_id]))?;
        let session_id = persisted.summary.session_id.clone();
        let attachment_lock = self.attachment_lock(&session_id)?;
        let _attaching = attachment_lock.lock().await;
        if let Some(runtime) = self.runtime(&session_id)? {
            return runtime
                .live
                .snapshot
                .lock()
                .map(|snapshot| snapshot.session.clone())
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"));
        }
        self.attach_persisted_session(persisted, provider).await
    }

    pub async fn attach_session(
        &self,
        session_id: &str,
        providers: &BTreeMap<String, AgentServerConfig>,
    ) -> Result<AgentSessionInfo, AgentError> {
        if session_id.is_empty() {
            return Err(AgentError::invalid_argument("sessionId must not be empty"));
        }
        let attachment_lock = self.attachment_lock(session_id)?;
        let _attaching = attachment_lock.lock().await;
        if let Some(runtime) = self.runtime(session_id)? {
            return runtime
                .live
                .snapshot
                .lock()
                .map(|snapshot| snapshot.session.clone())
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"));
        }
        let persisted = self
            .with_db(|db| db.find_agent_session_by_id(session_id))?
            .ok_or_else(|| AgentError::not_found("agent session was not found"))?;
        let provider = providers
            .get(&persisted.provider_id)
            .cloned()
            .ok_or_else(|| {
                AgentError::unavailable(format!(
                    "agent provider '{}' is not configured",
                    persisted.provider_id
                ))
            })?;
        self.attach_persisted_session(session_info(persisted), provider)
            .await
    }

    async fn attach_persisted_session(
        &self,
        persisted: AgentSessionInfo,
        provider: AgentServerConfig,
    ) -> Result<AgentSessionInfo, AgentError> {
        let session_id = persisted.summary.session_id.clone();
        let persisted_seq = self
            .with_db(|db| db.find_agent_session_by_id(&session_id))?
            .map(|session| session.latest_seq)
            .unwrap_or(0);
        let mut starting = persisted.clone();
        starting.summary.attachment = AgentAttachmentState::Starting;
        let (event_tx, _) = broadcast::channel(256);
        let live = Arc::new(AgentLiveSession {
            snapshot: StdMutex::new(AgentSessionLiveSnapshot {
                session: starting,
                active_turn: None,
                usage: None,
                config_options: Vec::new(),
                latest_seq: persisted_seq,
            }),
            state_update: StdMutex::new(()),
            config_changing: AtomicBool::new(false),
            events: event_tx,
        });
        let (runtime_events, runtime_receiver) = mpsc::unbounded_channel();
        let runtime = start_agent_runtime(
            AgentRuntimeConfig {
                command: provider.command,
                args: provider.args,
                env: provider.env,
                cwd: PathBuf::from(&persisted.summary.cwd),
                session: match persisted.provider_session_id.clone() {
                    Some(provider_session_id) => AgentRuntimeSession::Existing {
                        provider_session_id,
                    },
                    None => AgentRuntimeSession::New,
                },
            },
            runtime_events,
        )
        .await;
        let (handle, ready) = match runtime {
            Ok(runtime) => runtime,
            Err(message) => {
                if let Ok(mut snapshot) = live.snapshot.lock() {
                    snapshot.session.summary.attachment = AgentAttachmentState::Failed;
                    snapshot.session.failure = Some(AgentFailure {
                        message: message.clone(),
                        code: Some("acp_initialization_failed".to_string()),
                        retryable: true,
                    });
                }
                return Err(AgentError::unavailable(format!(
                    "initialize ACP agent: {message}"
                )));
            }
        };
        let attached_at_ms = current_unix_ms();
        let config_options = normalize_acp_config_options(ready.config_options);
        let config_values = stored_config_values(&config_options);
        if persisted.provider_session_id.is_none() {
            self.with_db(|db| {
                db.set_agent_provider_session_id(
                    &session_id,
                    &ready.provider_session_id,
                    attached_at_ms,
                )
            })?;
        }
        self.with_db_mut(|db| {
            db.replace_agent_session_config_values(&session_id, &config_values, attached_at_ms)
        })?;
        if let Ok(mut snapshot) = live.snapshot.lock() {
            snapshot.session.provider_session_id = Some(ready.provider_session_id);
            snapshot.session.attached_at_ms = Some(attached_at_ms);
            snapshot.session.summary.attachment = AgentAttachmentState::Attached;
            snapshot.session.summary.recoverability = if ready.resume_session {
                AgentSessionRecoverability::Resumable
            } else if ready.load_session {
                AgentSessionRecoverability::Loadable
            } else {
                AgentSessionRecoverability::ProcessLocal
            };
            snapshot.session.summary.updated_at_ms = attached_at_ms;
            snapshot.config_options = config_options;
        }
        self.runtimes
            .lock()
            .map_err(|_| AgentError::failed("agent runtimes lock was poisoned"))?
            .insert(
                session_id.clone(),
                AgentRuntimeEntry {
                    handle,
                    live: live.clone(),
                },
            );
        let manager = self.clone();
        let event_live = live.clone();
        tokio::spawn(async move {
            manager
                .consume_runtime_events(session_id, event_live, runtime_receiver)
                .await;
        });
        live.snapshot
            .lock()
            .map(|snapshot| snapshot.session.clone())
            .map_err(|_| AgentError::failed("agent live state lock was poisoned"))
    }

    pub fn subscribe_session(
        &self,
        session_id: &str,
    ) -> Result<AgentSessionSubscription, AgentError> {
        if let Some(runtime) = self.runtime(session_id)? {
            // Subscribe before reading the snapshot so an update cannot fall into
            // the gap between the two operations. Events already represented by
            // latestSeq may be delivered again and are safe for clients to ignore.
            let events = runtime.live.events.subscribe();
            let snapshot = runtime
                .live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?
                .clone();
            return Ok(AgentSessionSubscription {
                snapshot,
                events: Some(events),
            });
        }
        let session = self
            .with_db(|db| db.find_agent_session_by_id(session_id))?
            .ok_or_else(|| AgentError::not_found("agent session was not found"))?;
        Ok(AgentSessionSubscription {
            snapshot: AgentSessionLiveSnapshot {
                latest_seq: session.latest_seq,
                session: session_info(session),
                active_turn: None,
                usage: None,
                config_options: Vec::new(),
            },
            events: None,
        })
    }

    pub async fn set_session_config(
        &self,
        request: SetAgentSessionConfigReq,
    ) -> Result<SetAgentSessionConfigRes, AgentError> {
        if request.config_id.is_empty() {
            return Err(AgentError::invalid_argument("configId must not be empty"));
        }
        let runtime = self
            .runtime(&request.session_id)?
            .ok_or_else(|| AgentError::unavailable("agent session is not attached"))?;
        let _changing = AgentConfigChangeGuard::begin(&runtime.live.config_changing)?;
        let acp_value = {
            let _update = runtime
                .live
                .state_update
                .lock()
                .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
            let snapshot = runtime
                .live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            if snapshot.session.summary.attachment != AgentAttachmentState::Attached {
                return Err(AgentError::unavailable("agent session is not attached"));
            }
            if snapshot.active_turn.is_some() {
                return Err(AgentError::new(
                    AgentErrorKind::Conflict,
                    "session configuration cannot be changed during an active turn",
                ));
            }
            let option = snapshot
                .config_options
                .iter()
                .find(|option| option.config_id == request.config_id)
                .ok_or_else(|| AgentError::invalid_argument("configId is not currently offered"))?;
            validate_agent_config_value(option, request.value)?
        };
        let options = runtime
            .handle
            .set_config(request.config_id, acp_value)
            .await
            .map_err(|message| {
                AgentError::unavailable(format!("set ACP session configuration: {message}"))
            })?;
        self.apply_agent_config_options(&request.session_id, options, &runtime.live)
    }

    pub fn create_turn(&self, request: CreateAgentTurnReq) -> Result<AgentTurnInfo, AgentError> {
        if request.client_request_id.is_empty() {
            return Err(AgentError::invalid_argument(
                "clientRequestId must not be empty",
            ));
        }
        if let Some(existing) = self.with_db(|db| {
            db.find_agent_turn_by_client_request(&request.session_id, &request.client_request_id)
        })? {
            return Ok(stored_turn_info(existing));
        }
        let text = request
            .content
            .iter()
            .map(|content| match content {
                AgentContent::Text { text } => Ok(text.as_str()),
                _ => Err(AgentError::invalid_argument(
                    "only text content is supported by CreateAgentTurn for now",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        if text.is_empty() {
            return Err(AgentError::invalid_argument(
                "CreateAgentTurn content must not be empty",
            ));
        }
        let runtime = self
            .runtime(&request.session_id)?
            .ok_or_else(|| AgentError::unavailable("agent session is not attached"))?;
        let state_update = runtime
            .live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        if runtime.live.config_changing.load(Ordering::SeqCst) {
            return Err(AgentError::new(
                AgentErrorKind::Conflict,
                "agent session configuration is changing",
            ));
        }
        {
            let snapshot = runtime
                .live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            if snapshot.session.summary.attachment != AgentAttachmentState::Attached {
                return Err(AgentError::unavailable("agent session is not attached"));
            }
            if snapshot.active_turn.is_some() {
                return Err(AgentError::new(
                    AgentErrorKind::Conflict,
                    "agent session already has an active turn",
                ));
            }
        }
        let now = current_unix_ms();
        let turn_id = random_id("turn");
        let user_message_id = random_id("message");
        let assistant_message_id = random_id("message");
        let created = match self.with_db_mut(|db| {
            db.create_agent_text_turn(
                &request.session_id,
                &request.client_request_id,
                &turn_id,
                &user_message_id,
                &assistant_message_id,
                &text,
                now,
            )
        }) {
            Ok(created) => created,
            Err(error) => {
                if let Some(existing) = self.with_db(|db| {
                    db.find_agent_turn_by_client_request(
                        &request.session_id,
                        &request.client_request_id,
                    )
                })? {
                    return Ok(stored_turn_info(existing));
                }
                return Err(error);
            }
        };
        let turn = AgentTurnInfo {
            turn_id: turn_id.clone(),
            session_id: request.session_id.clone(),
            state: AgentTurnState::Running,
            created_at_ms: now,
            started_at_ms: Some(now),
            finished_at_ms: None,
            context: None,
        };
        let user_message = AgentMessage {
            message_id: user_message_id,
            turn_id: Some(turn_id.clone()),
            role: AgentMessageRole::User,
            content: vec![AgentContent::Text { text: text.clone() }],
            state: AgentMessageState::Complete,
            created_at_ms: now,
        };
        let assistant_message = AgentMessage {
            message_id: assistant_message_id,
            turn_id: Some(turn_id.clone()),
            role: AgentMessageRole::Assistant,
            content: Vec::new(),
            state: AgentMessageState::Streaming,
            created_at_ms: now,
        };
        {
            let mut snapshot = runtime
                .live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            snapshot.latest_seq = created.assistant_message_seq;
            snapshot.session.summary.turn_state = AgentSessionTurnState::Running;
            snapshot.session.summary.updated_at_ms = now;
            snapshot.active_turn = Some(AgentTurnRecord {
                turn: turn.clone(),
                messages: vec![user_message.clone(), assistant_message.clone()],
                tool_calls: Vec::new(),
                permissions: Vec::new(),
                plan: None,
                terminals: Vec::new(),
            });
        }
        let _ = runtime.live.events.send(AgentSessionEvent::TurnUpsert {
            seq: created.turn_seq,
            turn: turn.clone(),
        });
        let _ = runtime.live.events.send(AgentSessionEvent::MessageUpsert {
            seq: created.user_message_seq,
            message: user_message,
        });
        let _ = runtime.live.events.send(AgentSessionEvent::MessageUpsert {
            seq: created.assistant_message_seq,
            message: assistant_message,
        });
        let _ = self.catalog_events.send(created.catalog_revision);
        drop(state_update);
        if let Err(message) = runtime.handle.prompt(
            turn_id.clone(),
            vec![AcpContentBlock::Text(AcpTextContent::new(text))],
        ) {
            let _ = self.finish_live_turn(
                &request.session_id,
                &turn_id,
                Err(AgentFailure {
                    message: message.clone(),
                    code: Some("acp_runtime_unavailable".to_string()),
                    retryable: true,
                }),
                &runtime.live,
            );
            return Err(AgentError::unavailable(message));
        }
        Ok(turn)
    }

    pub fn list_sessions(
        &self,
        request: ListAgentSessionsReq,
    ) -> Result<ListAgentSessionsRes, AgentError> {
        let limit = usize::try_from(request.limit)
            .ok()
            .filter(|limit| (1..=MAX_SESSION_PAGE_SIZE).contains(limit))
            .ok_or_else(|| {
                AgentError::invalid_argument(format!(
                    "limit must be between 1 and {MAX_SESSION_PAGE_SIZE}"
                ))
            })?;
        let fingerprint = session_filter_fingerprint(
            &request.workspace,
            request.archived,
            request.query.as_deref(),
        );
        let cursor = request
            .cursor
            .as_deref()
            .map(|cursor| decode_session_cursor(cursor, fingerprint))
            .transpose()?;
        let workspace = match request.workspace {
            AgentSessionWorkspaceFilter::Any => StoredAgentWorkspaceFilter::Any,
            AgentSessionWorkspaceFilter::Project { project_id } => {
                StoredAgentWorkspaceFilter::Project(project_id)
            }
            AgentSessionWorkspaceFilter::Task { source_project_id } => {
                StoredAgentWorkspaceFilter::Task(source_project_id)
            }
        };
        let archived = match request.archived {
            AgentSessionArchiveFilter::ActiveOnly => StoredAgentArchiveFilter::ActiveOnly,
            AgentSessionArchiveFilter::ArchivedOnly => StoredAgentArchiveFilter::ArchivedOnly,
            AgentSessionArchiveFilter::All => StoredAgentArchiveFilter::All,
        };
        let mut rows = self.with_db(|db| {
            db.list_agent_sessions(&StoredAgentSessionQuery {
                workspace,
                archived,
                query: request.query,
                cursor,
                limit: limit + 1,
            })
        })?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more.then(|| {
            let last = rows.last().expect("non-empty paginated agent session page");
            encode_session_cursor(fingerprint, last.updated_at_ms, &last.session_id)
        });
        let catalog_revision = self.with_db(|db| db.agent_session_catalog_revision())?;
        Ok(ListAgentSessionsRes {
            rows: rows.into_iter().map(session_summary).collect(),
            next_cursor,
            catalog_revision,
        })
    }

    pub fn update_session(
        &self,
        request: UpdateAgentSessionReq,
    ) -> Result<AgentSessionInfo, AgentError> {
        if request.session_id.is_empty() {
            return Err(AgentError::invalid_argument("sessionId must not be empty"));
        }
        let title = match request.title {
            Some(AgentSessionTitleUpdate::Set { value }) => {
                let value = value.trim();
                if value.is_empty() {
                    return Err(AgentError::invalid_argument(
                        "title must not be empty; use Clear instead",
                    ));
                }
                Some(Some(value.to_string()))
            }
            Some(AgentSessionTitleUpdate::Clear) => Some(None),
            None => None,
        };
        if title.is_none() && request.archived.is_none() {
            return Err(AgentError::invalid_argument(
                "at least one session field must be updated",
            ));
        }
        if self
            .with_db(|db| db.find_agent_session_by_id(&request.session_id))?
            .is_none()
        {
            return Err(AgentError::not_found("agent session was not found"));
        }

        let runtime = self.runtime(&request.session_id)?;
        let update = || {
            self.with_db_mut(|db| {
                db.update_agent_session(
                    &request.session_id,
                    title.as_ref().map(|title| title.as_deref()),
                    request.archived,
                    current_unix_ms(),
                )
            })
        };
        let UpdatedAgentSession {
            session,
            seq,
            catalog_revision,
        } = if let Some(runtime) = runtime.as_ref() {
            let _state_update = runtime
                .live
                .state_update
                .lock()
                .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
            update()?
        } else {
            update()?
        };

        let response = if let Some(runtime) = runtime {
            let mut snapshot = runtime
                .live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            snapshot.latest_seq = seq;
            snapshot.session.summary.title = session.title;
            snapshot.session.summary.archived = session.archived;
            snapshot.session.summary.updated_at_ms = session.updated_at_ms;
            let response = snapshot.session.clone();
            drop(snapshot);
            let _ = runtime.live.events.send(AgentSessionEvent::SessionUpsert {
                seq,
                session: response.clone(),
            });
            response
        } else {
            session_info(session)
        };
        let _ = self.catalog_events.send(catalog_revision);
        Ok(response)
    }

    pub fn list_session_turns(
        &self,
        request: ListAgentSessionTurnsReq,
    ) -> Result<ListAgentSessionTurnsRes, AgentError> {
        if request.session_id.is_empty() {
            return Err(AgentError::invalid_argument("sessionId must not be empty"));
        }
        let limit = usize::try_from(request.limit)
            .ok()
            .filter(|limit| (1..=MAX_SESSION_HISTORY_PAGE_SIZE).contains(limit))
            .ok_or_else(|| {
                AgentError::invalid_argument(format!(
                    "limit must be between 1 and {MAX_SESSION_HISTORY_PAGE_SIZE}"
                ))
            })?;
        let session = self
            .with_db(|db| db.find_agent_session_by_id(&request.session_id))?
            .ok_or_else(|| AgentError::not_found("agent session was not found"))?;
        if request.through_seq > session.latest_seq {
            return Err(AgentError::invalid_argument(
                "throughSeq is newer than the persisted session state",
            ));
        }
        let fingerprint = history_cursor_fingerprint(&request.session_id, request.through_seq);
        let cursor = request
            .cursor
            .as_deref()
            .map(|cursor| decode_session_cursor(cursor, fingerprint))
            .transpose()?;
        let mut rows = self.with_db(|db| {
            db.list_completed_agent_turns(
                &request.session_id,
                request.through_seq,
                cursor,
                limit + 1,
            )
        })?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more.then(|| {
            let last = rows.last().expect("non-empty paginated agent turn history");
            encode_session_cursor(fingerprint, last.completed_seq, &last.turn.turn_id)
        });
        rows.reverse();
        Ok(ListAgentSessionTurnsRes {
            turns: rows.into_iter().map(stored_turn_record).collect(),
            next_cursor,
        })
    }

    async fn consume_runtime_events(
        &self,
        session_id: String,
        live: Arc<AgentLiveSession>,
        mut events: mpsc::UnboundedReceiver<AgentRuntimeEvent>,
    ) {
        while let Some(event) = events.recv().await {
            let result = match event {
                AgentRuntimeEvent::AgentTextChunk { turn_id, text } => {
                    self.apply_agent_text_chunk(&session_id, &turn_id, &text, &live)
                }
                AgentRuntimeEvent::Usage { used, size } => {
                    self.apply_agent_usage(&session_id, used, size, &live)
                }
                AgentRuntimeEvent::ConfigOptions { options } => self
                    .apply_agent_config_options(&session_id, options, &live)
                    .map(|_| ()),
                AgentRuntimeEvent::PromptFinished {
                    turn_id,
                    stop_reason,
                } => self.finish_live_turn(
                    &session_id,
                    &turn_id,
                    Ok(acp_stop_reason(stop_reason)),
                    &live,
                ),
                AgentRuntimeEvent::PromptFailed { turn_id, message } => self.finish_live_turn(
                    &session_id,
                    &turn_id,
                    Err(AgentFailure {
                        message,
                        code: Some("acp_prompt_failed".to_string()),
                        retryable: true,
                    }),
                    &live,
                ),
                AgentRuntimeEvent::Exited { message } => {
                    let active_turn_id = live.snapshot.lock().ok().and_then(|snapshot| {
                        snapshot
                            .active_turn
                            .as_ref()
                            .map(|turn| turn.turn.turn_id.clone())
                    });
                    if let Some(turn_id) = active_turn_id {
                        let failure_message = message
                            .clone()
                            .unwrap_or_else(|| "ACP agent exited during the prompt".to_string());
                        let _ = self.finish_live_turn(
                            &session_id,
                            &turn_id,
                            Err(AgentFailure {
                                message: failure_message,
                                code: Some("acp_process_exited".to_string()),
                                retryable: true,
                            }),
                            &live,
                        );
                    }
                    let result = self.apply_runtime_exit(&session_id, message, &live);
                    self.remove_runtime(&session_id, &live);
                    result
                }
            };
            if result.is_err() {
                break;
            }
        }
    }

    fn apply_agent_config_options(
        &self,
        session_id: &str,
        acp_options: Vec<AcpSessionConfigOption>,
        live: &AgentLiveSession,
    ) -> Result<SetAgentSessionConfigRes, AgentError> {
        let _update = live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        let options = normalize_acp_config_options(acp_options);
        {
            let snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            if snapshot.config_options == options {
                return Ok(SetAgentSessionConfigRes {
                    seq: snapshot.latest_seq,
                    config_options: options,
                });
            }
        }
        let now = current_unix_ms();
        let values = stored_config_values(&options);
        let seq = self.with_db_mut(|db| {
            db.replace_agent_session_config_values_and_advance(session_id, &values, now)
        })?;
        {
            let mut snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            snapshot.latest_seq = seq;
            snapshot.config_options = options.clone();
            snapshot.session.summary.updated_at_ms = now;
        }
        let _ = live.events.send(AgentSessionEvent::ConfigOptionsReplace {
            seq,
            options: options.clone(),
        });
        Ok(SetAgentSessionConfigRes {
            seq,
            config_options: options,
        })
    }

    fn apply_agent_text_chunk(
        &self,
        session_id: &str,
        turn_id: &str,
        text: &str,
        live: &AgentLiveSession,
    ) -> Result<(), AgentError> {
        let _update = live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        let message_id = {
            let snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            active_assistant_message(&snapshot, turn_id)
                .map(|message| message.message_id.clone())
                .ok_or_else(|| AgentError::failed("active assistant message was not found"))?
        };
        let now = current_unix_ms();
        let seq =
            self.with_db_mut(|db| db.append_agent_text_chunk(session_id, &message_id, text, now))?;
        {
            let mut snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            if let Some(message) = active_assistant_message_mut(&mut snapshot, turn_id) {
                message.content.push(AgentContent::Text {
                    text: text.to_string(),
                });
            }
            snapshot.latest_seq = seq;
            snapshot.session.summary.updated_at_ms = now;
        }
        let _ = live.events.send(AgentSessionEvent::MessageContentAppend {
            seq,
            message_id,
            content: AgentContent::Text {
                text: text.to_string(),
            },
        });
        Ok(())
    }

    fn apply_agent_usage(
        &self,
        session_id: &str,
        used: u64,
        size: u64,
        live: &AgentLiveSession,
    ) -> Result<(), AgentError> {
        let _update = live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        let now = current_unix_ms();
        let seq = self.with_db_mut(|db| db.advance_agent_session_sequence(session_id, now))?;
        let usage = AgentUsage {
            input_tokens: Some(used),
            output_tokens: None,
            cached_input_tokens: None,
            context_window_tokens: Some(size),
        };
        {
            let mut snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            snapshot.latest_seq = seq;
            snapshot.usage = Some(usage.clone());
        }
        let _ = live
            .events
            .send(AgentSessionEvent::UsageUpdate { seq, usage });
        Ok(())
    }

    fn finish_live_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        outcome: Result<AgentStopReason, AgentFailure>,
        live: &AgentLiveSession,
    ) -> Result<(), AgentError> {
        let _update = live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        let (message_id, preview) = {
            let snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            let message = active_assistant_message(&snapshot, turn_id)
                .ok_or_else(|| AgentError::failed("active assistant message was not found"))?;
            (message.message_id.clone(), message_preview(message))
        };
        let now = current_unix_ms();
        let (state_kind, stop_kind, stop_other, failure_message, failure_code, retryable) =
            match &outcome {
                Ok(reason) => {
                    let (kind, other) = stop_reason_db(reason);
                    ("completed", Some(kind), other, None, None, None)
                }
                Err(failure) => (
                    "failed",
                    None,
                    None,
                    Some(failure.message.as_str()),
                    failure.code.as_deref(),
                    Some(failure.retryable),
                ),
            };
        let finished = self.with_db_mut(|db| {
            db.finish_agent_turn(
                session_id,
                turn_id,
                &message_id,
                state_kind,
                stop_kind,
                stop_other.as_deref(),
                failure_message,
                failure_code,
                retryable,
                preview.as_deref(),
                now,
            )
        })?;
        let (message, turn) = {
            let mut snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            let active = snapshot
                .active_turn
                .as_mut()
                .ok_or_else(|| AgentError::failed("active agent turn was not found"))?;
            let message = active
                .messages
                .iter_mut()
                .find(|message| message.message_id == message_id)
                .ok_or_else(|| AgentError::failed("active assistant message was not found"))?;
            message.state = AgentMessageState::Complete;
            active.turn.state = match outcome {
                Ok(stop_reason) => AgentTurnState::Completed { stop_reason },
                Err(failure) => AgentTurnState::Failed { failure },
            };
            active.turn.finished_at_ms = Some(now);
            let message = message.clone();
            let turn = active.turn.clone();
            snapshot.latest_seq = finished.turn_seq;
            snapshot.session.summary.turn_state = AgentSessionTurnState::Idle;
            snapshot.session.summary.updated_at_ms = now;
            snapshot.session.summary.last_message_preview = preview;
            snapshot.active_turn = None;
            (message, turn)
        };
        let _ = live.events.send(AgentSessionEvent::MessageUpsert {
            seq: finished.assistant_message_seq,
            message,
        });
        let _ = live.events.send(AgentSessionEvent::TurnUpsert {
            seq: finished.turn_seq,
            turn,
        });
        let _ = self.catalog_events.send(finished.catalog_revision);
        Ok(())
    }

    fn apply_runtime_exit(
        &self,
        session_id: &str,
        message: Option<String>,
        live: &AgentLiveSession,
    ) -> Result<(), AgentError> {
        let _update = live
            .state_update
            .lock()
            .map_err(|_| AgentError::failed("agent state update lock was poisoned"))?;
        let now = current_unix_ms();
        let seq = self.with_db_mut(|db| db.advance_agent_session_sequence(session_id, now))?;
        let session = {
            let mut snapshot = live
                .snapshot
                .lock()
                .map_err(|_| AgentError::failed("agent live state lock was poisoned"))?;
            snapshot.latest_seq = seq;
            snapshot.session.detached_at_ms = Some(now);
            snapshot.session.summary.updated_at_ms = now;
            if let Some(message) = message {
                snapshot.session.summary.attachment = AgentAttachmentState::Failed;
                snapshot.session.failure = Some(AgentFailure {
                    message,
                    code: Some("acp_process_exited".to_string()),
                    retryable: true,
                });
            } else {
                snapshot.session.summary.attachment = AgentAttachmentState::Dormant;
            }
            snapshot.session.clone()
        };
        let _ = live
            .events
            .send(AgentSessionEvent::SessionUpsert { seq, session });
        Ok(())
    }

    fn runtime(&self, session_id: &str) -> Result<Option<AgentRuntimeEntry>, AgentError> {
        self.runtimes
            .lock()
            .map_err(|_| AgentError::failed("agent runtimes lock was poisoned"))
            .map(|runtimes| runtimes.get(session_id).cloned())
    }

    fn attachment_lock(&self, session_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, AgentError> {
        let mut locks = self
            .attachment_locks
            .lock()
            .map_err(|_| AgentError::failed("agent attachment locks were poisoned"))?;
        Ok(locks
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }

    fn remove_runtime(&self, session_id: &str, live: &Arc<AgentLiveSession>) {
        if let Ok(mut runtimes) = self.runtimes.lock() {
            let should_remove = runtimes
                .get(session_id)
                .is_some_and(|runtime| Arc::ptr_eq(&runtime.live, live));
            if should_remove {
                runtimes.remove(session_id);
            }
        }
    }

    fn with_db<T>(
        &self,
        operation: impl FnOnce(&DaemonStateDb) -> Result<T>,
    ) -> Result<T, AgentError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AgentError::failed("agent database lock was poisoned"))?;
        operation(&db).map_err(|error| AgentError::failed(format!("agent database: {error:#}")))
    }

    fn with_db_mut<T>(
        &self,
        operation: impl FnOnce(&mut DaemonStateDb) -> Result<T>,
    ) -> Result<T, AgentError> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| AgentError::failed("agent database lock was poisoned"))?;
        operation(&mut db).map_err(|error| AgentError::failed(format!("agent database: {error:#}")))
    }
}

fn normalize_acp_config_options(options: Vec<AcpSessionConfigOption>) -> Vec<AgentConfigOption> {
    options
        .into_iter()
        .filter_map(normalize_acp_config_option)
        .collect()
}

fn normalize_acp_config_option(option: AcpSessionConfigOption) -> Option<AgentConfigOption> {
    let input = match option.kind {
        AcpSessionConfigKind::Select(select) => {
            let mut options = Vec::new();
            match select.options {
                AcpSessionConfigSelectOptions::Ungrouped(values) => {
                    options.extend(values.into_iter().map(|value| AgentConfigSelectOption {
                        value: value.value.to_string(),
                        title: value.name,
                        description: value.description,
                        group: None,
                    }));
                }
                AcpSessionConfigSelectOptions::Grouped(groups) => {
                    for group in groups {
                        let normalized_group = AgentConfigSelectGroup {
                            group_id: group.group.to_string(),
                            title: group.name,
                        };
                        options.extend(group.options.into_iter().map(|value| {
                            AgentConfigSelectOption {
                                value: value.value.to_string(),
                                title: value.name,
                                description: value.description,
                                group: Some(normalized_group.clone()),
                            }
                        }));
                    }
                }
                _ => return None,
            }
            AgentConfigInput::Select {
                current_value: select.current_value.to_string(),
                options,
            }
        }
        AcpSessionConfigKind::Boolean(value) => AgentConfigInput::Boolean {
            current_value: value.current_value,
        },
        _ => return None,
    };
    Some(AgentConfigOption {
        config_id: option.id.to_string(),
        title: option.name,
        description: option.description,
        input,
        category: option.category.map(|category| match category {
            AcpSessionConfigOptionCategory::Mode => AgentConfigOptionCategory::Mode,
            AcpSessionConfigOptionCategory::Model => AgentConfigOptionCategory::Model,
            AcpSessionConfigOptionCategory::ModelConfig => AgentConfigOptionCategory::ModelConfig,
            AcpSessionConfigOptionCategory::ThoughtLevel => AgentConfigOptionCategory::ThoughtLevel,
            AcpSessionConfigOptionCategory::Other(name) => {
                AgentConfigOptionCategory::Other { name }
            }
            _ => AgentConfigOptionCategory::Other {
                name: "unknown".to_string(),
            },
        }),
    })
}

fn validate_agent_config_value(
    option: &AgentConfigOption,
    value: AgentConfigValue,
) -> Result<AcpSessionConfigOptionValue, AgentError> {
    match (&option.input, value) {
        (AgentConfigInput::Select { options, .. }, AgentConfigValue::String { value }) => {
            if !options.iter().any(|option| option.value == value) {
                return Err(AgentError::invalid_argument(
                    "the selected value is not currently offered",
                ));
            }
            Ok(AcpSessionConfigOptionValue::value_id(value))
        }
        (AgentConfigInput::Boolean { .. }, AgentConfigValue::Boolean { value }) => {
            Ok(AcpSessionConfigOptionValue::boolean(value))
        }
        (AgentConfigInput::Text { .. }, AgentConfigValue::String { .. }) => Err(
            AgentError::invalid_argument("text session configuration is not supported by ACP"),
        ),
        _ => Err(AgentError::invalid_argument(
            "the configuration value type does not match the current option",
        )),
    }
}

fn stored_config_values(options: &[AgentConfigOption]) -> Vec<StoredAgentConfigValue> {
    options
        .iter()
        .filter_map(|option| match &option.input {
            AgentConfigInput::Select { current_value, .. }
            | AgentConfigInput::Text {
                current_value: Some(current_value),
                ..
            } => Some(StoredAgentConfigValue::String {
                config_id: option.config_id.clone(),
                value: current_value.clone(),
            }),
            AgentConfigInput::Boolean { current_value } => Some(StoredAgentConfigValue::Boolean {
                config_id: option.config_id.clone(),
                value: *current_value,
            }),
            AgentConfigInput::Text {
                current_value: None,
                ..
            } => None,
        })
        .collect()
}

fn active_assistant_message<'a>(
    snapshot: &'a AgentSessionLiveSnapshot,
    turn_id: &str,
) -> Option<&'a AgentMessage> {
    snapshot
        .active_turn
        .as_ref()
        .filter(|turn| turn.turn.turn_id == turn_id)
        .and_then(|turn| {
            turn.messages
                .iter()
                .find(|message| message.role == AgentMessageRole::Assistant)
        })
}

fn active_assistant_message_mut<'a>(
    snapshot: &'a mut AgentSessionLiveSnapshot,
    turn_id: &str,
) -> Option<&'a mut AgentMessage> {
    snapshot
        .active_turn
        .as_mut()
        .filter(|turn| turn.turn.turn_id == turn_id)
        .and_then(|turn| {
            turn.messages
                .iter_mut()
                .find(|message| message.role == AgentMessageRole::Assistant)
        })
}

fn message_preview(message: &AgentMessage) -> Option<String> {
    let text = message
        .content
        .iter()
        .filter_map(|content| match content {
            AgentContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(240).collect())
    }
}

fn acp_stop_reason(reason: AcpStopReason) -> AgentStopReason {
    match reason {
        AcpStopReason::EndTurn => AgentStopReason::EndTurn,
        AcpStopReason::MaxTokens => AgentStopReason::MaxTokens,
        AcpStopReason::Refusal => AgentStopReason::Refusal,
        AcpStopReason::Cancelled => AgentStopReason::Cancelled,
        AcpStopReason::MaxTurnRequests => AgentStopReason::Other {
            name: "max_turn_requests".to_string(),
        },
        _ => AgentStopReason::Other {
            name: "unknown".to_string(),
        },
    }
}

fn stop_reason_db(reason: &AgentStopReason) -> (&'static str, Option<String>) {
    match reason {
        AgentStopReason::EndTurn => ("end_turn", None),
        AgentStopReason::MaxTokens => ("max_tokens", None),
        AgentStopReason::Refusal => ("refusal", None),
        AgentStopReason::Cancelled => ("cancelled", None),
        AgentStopReason::Other { name } => ("other", Some(name.clone())),
    }
}

fn stored_turn_info(turn: StoredAgentTurn) -> AgentTurnInfo {
    let state = match turn.state_kind.as_str() {
        "running" => AgentTurnState::Running,
        "awaiting_permission" => AgentTurnState::AwaitingPermission,
        "completed" => AgentTurnState::Completed {
            stop_reason: match turn.stop_reason_kind.as_deref() {
                Some("end_turn") => AgentStopReason::EndTurn,
                Some("max_tokens") => AgentStopReason::MaxTokens,
                Some("refusal") => AgentStopReason::Refusal,
                Some("cancelled") => AgentStopReason::Cancelled,
                Some(_) => AgentStopReason::Other {
                    name: turn
                        .stop_reason_other
                        .unwrap_or_else(|| "other".to_string()),
                },
                None => AgentStopReason::Other {
                    name: "unknown".to_string(),
                },
            },
        },
        "cancelled" => AgentTurnState::Cancelled,
        "failed" => AgentTurnState::Failed {
            failure: AgentFailure {
                message: turn
                    .failure_message
                    .unwrap_or_else(|| "agent turn failed".to_string()),
                code: turn.failure_code,
                retryable: turn.failure_retryable.unwrap_or(false),
            },
        },
        _ => AgentTurnState::Queued,
    };
    AgentTurnInfo {
        turn_id: turn.turn_id,
        session_id: turn.session_id,
        state,
        created_at_ms: turn.created_at_ms,
        started_at_ms: turn.started_at_ms,
        finished_at_ms: turn.finished_at_ms,
        context: None,
    }
}

fn stored_turn_record(record: StoredAgentTurnRecord) -> AgentTurnRecord {
    AgentTurnRecord {
        turn: stored_turn_info(record.turn),
        messages: record.messages.into_iter().map(stored_message).collect(),
        tool_calls: Vec::new(),
        permissions: Vec::new(),
        plan: None,
        terminals: Vec::new(),
    }
}

fn stored_message(message: StoredAgentMessage) -> AgentMessage {
    let role = match message.role_kind.as_str() {
        "user" => AgentMessageRole::User,
        "assistant" => AgentMessageRole::Assistant,
        "thought" => AgentMessageRole::Thought,
        "system" => AgentMessageRole::System,
        _ => AgentMessageRole::Other {
            name: message
                .role_other
                .unwrap_or_else(|| message.role_kind.clone()),
        },
    };
    AgentMessage {
        message_id: message.message_id,
        turn_id: message.turn_id,
        role,
        content: message
            .content
            .into_iter()
            .map(|content| match content {
                StoredAgentMessageContent::Text { text } => AgentContent::Text { text },
                StoredAgentMessageContent::Image { mime_type, data } => {
                    AgentContent::Image { mime_type, data }
                }
                StoredAgentMessageContent::ResourceLink {
                    uri,
                    name,
                    mime_type,
                } => AgentContent::ResourceLink {
                    uri,
                    name,
                    mime_type,
                },
                StoredAgentMessageContent::EmbeddedText {
                    uri,
                    mime_type,
                    text,
                } => AgentContent::EmbeddedText {
                    uri,
                    mime_type,
                    text,
                },
            })
            .collect(),
        state: if message.state_kind == "streaming" {
            AgentMessageState::Streaming
        } else {
            AgentMessageState::Complete
        },
        created_at_ms: message.created_at_ms,
    }
}

fn session_matches_create_request(
    session: &StoredAgentSession,
    request: &CreateAgentSessionReq,
) -> bool {
    if session.provider_id != request.provider_id || session.title != request.title {
        return false;
    }
    match &request.workspace {
        CreateAgentWorkspace::Project { project_id } => {
            session.workspace_kind == "project"
                && session.project_id.as_deref() == Some(project_id.as_str())
        }
        CreateAgentWorkspace::Task {
            source: AgentTaskWorkspaceSource::Empty,
        } => session.workspace_kind == "task" && session.task_source_project_id.is_none(),
        CreateAgentWorkspace::Task { .. } => false,
    }
}

pub fn provider_rows(config: &SystemConfig) -> Vec<AgentProviderInfo> {
    config
        .agent_servers
        .iter()
        .map(|(provider_id, server)| AgentProviderInfo {
            provider_id: provider_id.clone(),
            title: provider_id.clone(),
            version: None,
            availability: if command_available(&server.command) {
                AgentProviderAvailability::Available
            } else {
                AgentProviderAvailability::Missing
            },
            authentication: AgentProviderAuthentication::NotRequired,
            capabilities: AgentProviderCapabilities::default(),
        })
        .collect()
}

pub fn provider_ids(config: &SystemConfig) -> HashSet<String> {
    config.agent_servers.keys().cloned().collect()
}

pub fn providers_patch(
    previous: &[AgentProviderInfo],
    next: &[AgentProviderInfo],
) -> Option<AgentProvidersTableEvent> {
    table_patch(previous, next, |row| row.provider_id.as_str())
        .map(|(removes, upserts)| AgentProvidersTableEvent::Patch { removes, upserts })
}

pub fn projects_patch(
    previous: &[AgentProjectInfo],
    next: &[AgentProjectInfo],
) -> Option<AgentProjectsTableEvent> {
    table_patch(previous, next, |row| row.project_id.as_str())
        .map(|(removes, upserts)| AgentProjectsTableEvent::Patch { removes, upserts })
}

fn table_patch<T: Clone + PartialEq>(
    previous: &[T],
    next: &[T],
    key: impl Fn(&T) -> &str,
) -> Option<(Vec<String>, Vec<T>)> {
    let previous = previous
        .iter()
        .map(|row| (key(row).to_string(), row))
        .collect::<BTreeMap<_, _>>();
    let next = next
        .iter()
        .map(|row| (key(row).to_string(), row))
        .collect::<BTreeMap<_, _>>();
    let removes = previous
        .keys()
        .filter(|id| !next.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let upserts = next
        .iter()
        .filter(|(id, row)| previous.get(*id).is_none_or(|previous| *previous != **row))
        .map(|(_, row)| (*row).clone())
        .collect::<Vec<_>>();
    (!removes.is_empty() || !upserts.is_empty()).then_some((removes, upserts))
}

fn project_info(project: StoredAgentProject) -> AgentProjectInfo {
    let availability = match fs::metadata(&project.root_path) {
        Ok(metadata) if metadata.is_dir() => AgentProjectAvailability::Available,
        Ok(_) => AgentProjectAvailability::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            AgentProjectAvailability::PermissionDenied
        }
        Err(_) => AgentProjectAvailability::Missing,
    };
    AgentProjectInfo {
        project_id: project.project_id,
        title: project.title,
        root_path: project.root_path,
        availability,
        created_at_ms: project.created_at_ms,
        last_opened_at_ms: project.last_opened_at_ms,
    }
}

fn session_info(session: StoredAgentSession) -> AgentSessionInfo {
    let provider_session_id = session.provider_session_id.clone();
    AgentSessionInfo {
        summary: session_summary(session),
        provider_session_id,
        attached_at_ms: None,
        detached_at_ms: None,
        failure: None,
    }
}

fn session_summary(session: StoredAgentSession) -> AgentSessionSummary {
    let workspace = if session.workspace_kind == "project" {
        AgentWorkspaceBinding::Project {
            project_id: session.project_id.unwrap_or_default(),
        }
    } else {
        AgentWorkspaceBinding::Task {
            task_workspace_id: session.task_workspace_id.unwrap_or_default(),
            source_project_id: session.task_source_project_id,
            state: task_workspace_state(session.task_state_kind.as_deref()),
        }
    };
    AgentSessionSummary {
        session_id: session.session_id,
        provider_id: session.provider_id,
        title: session.title,
        cwd: session.cwd,
        workspace,
        attachment: AgentAttachmentState::Dormant,
        turn_state: turn_state(session.active_turn_state_kind.as_deref()),
        recoverability: AgentSessionRecoverability::Unknown,
        archived: session.archived,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        last_message_preview: session.last_message_preview,
    }
}

fn task_workspace_state(state: Option<&str>) -> AgentTaskWorkspaceState {
    match state {
        Some("provisioning") => AgentTaskWorkspaceState::Provisioning,
        Some("missing") => AgentTaskWorkspaceState::Missing,
        Some("cleanup_pending") => AgentTaskWorkspaceState::CleanupPending,
        _ => AgentTaskWorkspaceState::Ready,
    }
}

fn turn_state(state: Option<&str>) -> AgentSessionTurnState {
    match state {
        Some("queued") => AgentSessionTurnState::Queued,
        Some("running") => AgentSessionTurnState::Running,
        Some("awaiting_permission") => AgentSessionTurnState::AwaitingPermission,
        _ => AgentSessionTurnState::Idle,
    }
}

fn canonical_project_path(path: &str) -> Result<String, AgentError> {
    if path.trim().is_empty() {
        return Err(AgentError::invalid_argument("rootPath must not be empty"));
    }
    let canonical = fs::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => AgentError::not_found("project directory was not found"),
        std::io::ErrorKind::PermissionDenied => {
            AgentError::permission_denied("project directory cannot be accessed")
        }
        _ => AgentError::failed(format!("canonicalize project directory: {error}")),
    })?;
    if !canonical.is_dir() {
        return Err(AgentError::invalid_argument(
            "rootPath must refer to a directory",
        ));
    }
    Ok(path_text(&canonical))
}

fn path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(path) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{path}");
        }
        if let Some(path) = text.strip_prefix(r"\\?\") {
            return path.to_string();
        }
    }
    text.into_owned()
}

fn default_project_title(root_path: &str) -> String {
    Path::new(root_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(root_path)
        .to_string()
}

fn command_available(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    let path = Path::new(command);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file();
    }
    let Some(search_path) = env::var_os("PATH") else {
        return false;
    };
    let extensions = executable_extensions();
    env::split_paths(&search_path).any(|directory| {
        extensions
            .iter()
            .any(|extension| directory.join(format!("{command}{extension}")).is_file())
    })
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut extensions = vec![String::new()];
        extensions.extend(
            env::var_os("PATHEXT")
                .map(|value| {
                    value
                        .to_string_lossy()
                        .split(';')
                        .filter(|value| !value.is_empty())
                        .map(|value| value.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| {
                    vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]
                }),
        );
        extensions
    }
    #[cfg(not(windows))]
    {
        vec![String::new()]
    }
}

fn random_id(prefix: &str) -> String {
    format!("{prefix}-{:032x}", rand::random::<u128>())
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn notify_watch(sender: &watch::Sender<u64>) {
    sender.send_modify(|revision| *revision = revision.saturating_add(1));
}

fn session_filter_fingerprint(
    workspace: &AgentSessionWorkspaceFilter,
    archived: AgentSessionArchiveFilter,
    query: Option<&str>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match workspace {
        AgentSessionWorkspaceFilter::Any => "any".hash(&mut hasher),
        AgentSessionWorkspaceFilter::Project { project_id } => {
            "project".hash(&mut hasher);
            project_id.hash(&mut hasher);
        }
        AgentSessionWorkspaceFilter::Task { source_project_id } => {
            "task".hash(&mut hasher);
            source_project_id.hash(&mut hasher);
        }
    }
    match archived {
        AgentSessionArchiveFilter::ActiveOnly => "active".hash(&mut hasher),
        AgentSessionArchiveFilter::ArchivedOnly => "archived".hash(&mut hasher),
        AgentSessionArchiveFilter::All => "all".hash(&mut hasher),
    }
    query.hash(&mut hasher);
    hasher.finish()
}

fn history_cursor_fingerprint(session_id: &str, through_seq: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "agent-session-history".hash(&mut hasher);
    session_id.hash(&mut hasher);
    through_seq.hash(&mut hasher);
    hasher.finish()
}

fn encode_session_cursor(fingerprint: u64, updated_at_ms: u64, session_id: &str) -> String {
    format!("{fingerprint:016x}:{updated_at_ms}:{session_id}")
}

fn decode_session_cursor(
    cursor: &str,
    expected_fingerprint: u64,
) -> Result<(u64, String), AgentError> {
    let mut parts = cursor.splitn(3, ':');
    let fingerprint = parts
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    let updated_at_ms = parts.next().and_then(|value| value.parse::<u64>().ok());
    let session_id = parts.next().filter(|value| !value.is_empty());
    if fingerprint != Some(expected_fingerprint) {
        return Err(AgentError::invalid_argument(
            "session cursor does not match the request filters",
        ));
    }
    match (updated_at_ms, session_id) {
        (Some(updated_at_ms), Some(session_id)) => Ok((updated_at_ms, session_id.to_string())),
        _ => Err(AgentError::invalid_argument("session cursor is invalid")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{SessionConfigSelectGroup, SessionConfigSelectOption};
    use rieul_daemon_core::generated::rpc::{
        AgentSessionArchiveFilter, AgentSessionWorkspaceFilter,
    };

    fn manager(root: &Path) -> AgentManager {
        AgentManager::open(
            DaemonStateDb::open_in_memory_for_tests().unwrap(),
            root.join("tasks"),
        )
        .unwrap()
    }

    #[test]
    fn normalizes_grouped_thought_level_config() {
        let option = AcpSessionConfigOption::select(
            "reasoning_effort",
            "Reasoning effort",
            "medium",
            vec![SessionConfigSelectGroup::new(
                "effort",
                "Effort",
                vec![
                    SessionConfigSelectOption::new("low", "Low"),
                    SessionConfigSelectOption::new("medium", "Medium"),
                    SessionConfigSelectOption::new("high", "High"),
                ],
            )],
        )
        .category(AcpSessionConfigOptionCategory::ThoughtLevel);

        let normalized = normalize_acp_config_options(vec![option]);

        assert_eq!(normalized.len(), 1);
        assert_eq!(
            normalized[0].category,
            Some(AgentConfigOptionCategory::ThoughtLevel)
        );
        let AgentConfigInput::Select {
            current_value,
            options,
        } = &normalized[0].input
        else {
            panic!("expected select config");
        };
        assert_eq!(current_value, "medium");
        assert_eq!(options.len(), 3);
        assert_eq!(
            options[0].group,
            Some(AgentConfigSelectGroup {
                group_id: "effort".to_string(),
                title: "Effort".to_string(),
            })
        );
    }

    #[test]
    fn creates_deduplicates_and_removes_projects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let first = manager
            .create_project(CreateAgentProjectReq {
                root_path: dir.path().to_string_lossy().into_owned(),
                title: Some("Workspace".to_string()),
            })
            .unwrap();
        let second = manager
            .create_project(CreateAgentProjectReq {
                root_path: dir.path().join(".").to_string_lossy().into_owned(),
                title: Some("Ignored".to_string()),
            })
            .unwrap();

        assert_eq!(first.project_id, second.project_id);
        assert_eq!(manager.projects_snapshot().unwrap().len(), 1);
        manager.remove_project(&first.project_id).unwrap();
        assert!(manager.projects_snapshot().unwrap().is_empty());
    }

    #[test]
    fn creates_idempotent_project_sessions_and_lists_them() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let project = manager
            .create_project(CreateAgentProjectReq {
                root_path: dir.path().to_string_lossy().into_owned(),
                title: None,
            })
            .unwrap();
        let request = CreateAgentSessionReq {
            provider_id: "test".to_string(),
            workspace: CreateAgentWorkspace::Project {
                project_id: project.project_id,
            },
            title: Some("Session".to_string()),
            creation_request_id: "request-1".to_string(),
        };
        let providers = HashSet::from(["test".to_string()]);
        let first = manager.create_session(request.clone(), &providers).unwrap();
        let second = manager.create_session(request, &providers).unwrap();

        assert_eq!(first.summary.session_id, second.summary.session_id);
        let page = manager
            .list_sessions(ListAgentSessionsReq {
                workspace: AgentSessionWorkspaceFilter::Any,
                archived: AgentSessionArchiveFilter::ActiveOnly,
                query: None,
                cursor: None,
                limit: 20,
            })
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.catalog_revision, 1);
    }

    #[test]
    fn updates_and_archives_persisted_session() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let session = manager
            .create_session(
                CreateAgentSessionReq {
                    provider_id: "test".to_string(),
                    workspace: CreateAgentWorkspace::Task {
                        source: AgentTaskWorkspaceSource::Empty,
                    },
                    title: Some("Session".to_string()),
                    creation_request_id: "archive-request".to_string(),
                },
                &HashSet::from(["test".to_string()]),
            )
            .unwrap();

        let updated = manager
            .update_session(UpdateAgentSessionReq {
                session_id: session.summary.session_id.clone(),
                title: Some(AgentSessionTitleUpdate::Set {
                    value: "Renamed".to_string(),
                }),
                archived: Some(true),
            })
            .unwrap();

        assert_eq!(updated.summary.title.as_deref(), Some("Renamed"));
        assert!(updated.summary.archived);
        let active = manager
            .list_sessions(ListAgentSessionsReq {
                workspace: AgentSessionWorkspaceFilter::Any,
                archived: AgentSessionArchiveFilter::ActiveOnly,
                query: None,
                cursor: None,
                limit: 20,
            })
            .unwrap();
        assert!(active.rows.is_empty());
        let archived = manager
            .list_sessions(ListAgentSessionsReq {
                workspace: AgentSessionWorkspaceFilter::Any,
                archived: AgentSessionArchiveFilter::ArchivedOnly,
                query: None,
                cursor: None,
                limit: 20,
            })
            .unwrap();
        assert_eq!(archived.rows, vec![updated.summary]);
        assert_eq!(archived.catalog_revision, 2);
        assert_eq!(
            manager
                .subscribe_session(&session.summary.session_id)
                .unwrap()
                .snapshot
                .latest_seq,
            1
        );
    }

    #[test]
    fn creates_empty_task_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let session = manager
            .create_session(
                CreateAgentSessionReq {
                    provider_id: "test".to_string(),
                    workspace: CreateAgentWorkspace::Task {
                        source: AgentTaskWorkspaceSource::Empty,
                    },
                    title: None,
                    creation_request_id: "request-1".to_string(),
                },
                &HashSet::from(["test".to_string()]),
            )
            .unwrap();

        assert!(Path::new(&session.summary.cwd).is_dir());
        assert!(matches!(
            session.summary.workspace,
            AgentWorkspaceBinding::Task { .. }
        ));
    }

    #[test]
    fn pages_completed_session_turn_history_from_snapshot_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let session = manager
            .create_session(
                CreateAgentSessionReq {
                    provider_id: "test".to_string(),
                    workspace: CreateAgentWorkspace::Task {
                        source: AgentTaskWorkspaceSource::Empty,
                    },
                    title: None,
                    creation_request_id: "history-session-request".to_string(),
                },
                &HashSet::from(["test".to_string()]),
            )
            .unwrap();
        let session_id = session.summary.session_id;
        manager
            .with_db_mut(|db| {
                for (index, reply) in [(1, "first reply"), (2, "second reply")] {
                    let turn_id = format!("turn-{index}");
                    let assistant_message_id = format!("assistant-{index}");
                    db.create_agent_text_turn(
                        &session_id,
                        &format!("request-{index}"),
                        &turn_id,
                        &format!("user-{index}"),
                        &assistant_message_id,
                        &format!("prompt {index}"),
                        index,
                    )?;
                    db.append_agent_text_chunk(&session_id, &assistant_message_id, reply, index)?;
                    db.finish_agent_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
                        "completed",
                        Some("end_turn"),
                        None,
                        None,
                        None,
                        None,
                        Some(reply),
                        index,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let through_seq = manager
            .with_db(|db| db.find_agent_session_by_id(&session_id))
            .unwrap()
            .unwrap()
            .latest_seq;

        let newest = manager
            .list_session_turns(ListAgentSessionTurnsReq {
                session_id: session_id.clone(),
                through_seq,
                cursor: None,
                limit: 1,
            })
            .unwrap();
        assert_eq!(newest.turns[0].turn.turn_id, "turn-2");
        assert_eq!(
            newest.turns[0].messages[1].content,
            vec![AgentContent::Text {
                text: "second reply".to_string(),
            }]
        );
        let older = manager
            .list_session_turns(ListAgentSessionTurnsReq {
                session_id,
                through_seq,
                cursor: newest.next_cursor,
                limit: 1,
            })
            .unwrap();
        assert_eq!(older.turns[0].turn.turn_id, "turn-1");
        assert!(older.next_cursor.is_none());
    }

    #[test]
    fn rejects_reusing_creation_key_for_different_session() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let providers = HashSet::from(["test".to_string()]);
        manager
            .create_session(
                CreateAgentSessionReq {
                    provider_id: "test".to_string(),
                    workspace: CreateAgentWorkspace::Task {
                        source: AgentTaskWorkspaceSource::Empty,
                    },
                    title: Some("First".to_string()),
                    creation_request_id: "request-1".to_string(),
                },
                &providers,
            )
            .unwrap();

        let error = manager
            .create_session(
                CreateAgentSessionReq {
                    provider_id: "test".to_string(),
                    workspace: CreateAgentWorkspace::Task {
                        source: AgentTaskWorkspaceSource::Empty,
                    },
                    title: Some("Second".to_string()),
                    creation_request_id: "request-1".to_string(),
                },
                &providers,
            )
            .unwrap_err();

        assert_eq!(error.kind, AgentErrorKind::Conflict);
    }
}
