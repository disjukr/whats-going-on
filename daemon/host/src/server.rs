use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use notify::{Event, RecursiveMode, Watcher};
use rieul_daemon_core::config::{
    client_credentials_path, daemon_agent_workspaces_path, daemon_state_database_path,
    daemon_status_path, load_client_credentials_or_default, load_or_default,
    load_or_generated_default, save, save_client_credentials, ClientCredentialRecord,
    ClientCredentials, SystemConfig,
};
use rieul_daemon_core::generated::rpc::{
    AgentProjectsTableEvent, AgentProvidersTableEvent, AgentSessionEvent, AttachAgentSessionReq,
    CreateAgentProjectReq, CreateAgentSessionReq, CreateAgentTurnReq, ListAgentSessionTurnsReq,
    ListAgentSessionsReq, RemoveAgentProjectReq, SetAgentSessionConfigReq,
    SubscribeAgentSessionReq, UpdateAgentSessionReq, WriteFileReq as GeneratedWriteFileReq,
    WriteTerminalInputReq as GeneratedWriteTerminalInputReq,
};
use rieul_daemon_core::pairing::{
    create_pairing_code, issue_client_secret, reissue_client_secret, renew_client_credential,
    verify_client_credential, verify_pairing_code, PairingRecord,
};
use rieul_daemon_core::rpc::{
    AttachTerminalSessionReq, AvailableShellsTableEvent, BulkMutationItemResult, BulkMutationRes,
    ClearJobsReq, ClientInfo, ClientKey, ClientsTableEvent, CloseTerminalSessionReq,
    CompletePairingRequest, CompletePairingResponse, CreateJobReq, CreateNodesReq,
    CreateScheduleReq, CreateTerminalSessionReq, DaemonEnvironment, DaemonInfo, DeleteJobsReq,
    DeleteMode, DeletePathsReq, DeleteSchedulesReq, DirectoryEntryKey,
    DirectorySubscriptionCloseReason, DirectoryTableEvent, FsEntry, GetScheduleNextRunsReq,
    JobOutputEvent, JobsTableEvent, KillJobReq, KillProcessReq, ProcId, ProcStream, ProcessDetail,
    ProcessDetailEvent, ProcessInfo, ProcessIoUsage, ProcessMetadata, ProcessModuleInfo,
    ProcessModulesTableEvent, ProcessResourceInUseInfo, ProcessResourceUsage,
    ProcessResourcesInUseTableEvent, ProcessSocketInUseInfo, ProcessSocketsInUseTableEvent,
    ProcessStatus, ProcessesTableEvent, PurgeTrashItemsReq, ReadFileChunk, ReadFileReq,
    RemoveClientReq, RenamePathsReq, RenewClientCredentialResponse, RestoreTrashItemsReq,
    RootEntryKey, RootsSubscriptionCloseReason, RootsTableEvent, RpcErrorCode, RpcErrorPayload,
    RpcHandlerFuture, RpcHandlers, RpcRequest, RpcRequestDecodeError, RpcResponse, RunCommandReq,
    SchedulesTableEvent, StartPairingRequest, StartPairingResponse, SubscribeDirectoryReq,
    SubscribeJobOutputReq, SubscribeJobsReq, SubscribeProcessDetailReq, SubscribeProcessModulesReq,
    SubscribeProcessResourcesInUseReq, SubscribeProcessSocketsInUseReq, SubscribeSchedulesReq,
    SubscribeWindowDetailReq, TakeTerminalControlReq, TerminalEvent, TerminalSessionsTableEvent,
    TrashItem, TrashItemsSubscriptionCloseReason, TrashItemsTableEvent, UpdateScheduleReq,
    WindowDetail, WindowDetailEvent, WindowInfo, WindowsTableEvent, WriteFileChunk, WriteFileReq,
    WriteTerminalInputReq, MAX_U53,
};
use rieul_daemon_core::traits::{
    BoxFutureResult, FileService, ProcessModulesService, ProcessResourcesInUseService,
    ProcessSocketsInUseService, ServiceError, WindowService, WriteFileChunkSource,
};
use rieul_daemon_core::wire::{
    DatagramMessage, PairedSecretCredential, ReqResMessage, RpcErrorKind, SessionAuthErrorCode,
    MAX_MESSAGE_SEQUENCE_SIZE, PAIRED_SECRET_AUTH_MECHANISM,
};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use sysinfo::{
    Pid as SysPid, Process as SysProcess, ProcessRefreshKind, ProcessStatus as SysProcessStatus,
    ProcessesToUpdate, System as SysinfoSystem, UpdateKind,
};
use time::OffsetDateTime;
use tokio::sync::{watch, Mutex};
use tracing::{info, warn};
use web_transport_quinn::proto::ConnectResponse;

use crate::agent::{
    projects_patch, provider_rows, providers_patch, AgentError, AgentErrorKind, AgentManager,
};
use crate::cert::{
    configured_certificate_paths, prepare_server_certificate, uses_scheduled_certificate_refresh,
};
use crate::command::{CommandError, CommandErrorKind, CommandManager};
use crate::state_db::DaemonStateDb;
use crate::terminal::{AttachedTerminal, TerminalBackend, TerminalManager};

const CERT_RELOAD_DEBOUNCE: Duration = Duration::from_millis(250);
const CONFIG_STARTUP_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const SCHEDULED_CERT_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SUBSCRIPTION_DEBOUNCE: Duration = Duration::from_millis(150);
const PROCESSES_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROCESS_DETAIL_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_RESOURCES_IN_USE_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROCESS_SOCKETS_IN_USE_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROCESS_MODULES_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const WINDOWS_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const ROOTS_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const TRASH_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const QUIC_MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const QUIC_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const READ_FILE_CHUNK_SIZE: usize = 64 * 1024;

type SharedSystemConfig = Arc<Mutex<SystemConfig>>;
type SharedClientCredentials = Arc<Mutex<ClientCredentials>>;
type SharedClientCredentialsEvents = watch::Sender<ClientCredentials>;
type RpcSessionId = u64;
type PairingAttemptId = u64;
type SharedPairingChallenge = Arc<Mutex<PairingState>>;
type SharedRpcSessionState = Arc<Mutex<RpcSessionState>>;
type SharedFileService = Arc<dyn FileService>;
type SharedWindowService = Arc<dyn WindowService>;
type SharedProcessResourcesInUseService = Arc<dyn ProcessResourcesInUseService>;
type SharedProcessSocketsInUseService = Arc<dyn ProcessSocketsInUseService>;
type SharedProcessModulesService = Arc<dyn ProcessModulesService>;
type SharedHostRpcHandlers = Arc<HostRpcHandlers>;
type SharedPairingNotifier = Arc<dyn PairingNotifier>;
type SharedTerminalManager = Arc<TerminalManager>;
type SharedTerminalBackend = Arc<dyn TerminalBackend>;
type SharedCommandManager = Arc<CommandManager>;
type SharedAgentManager = Arc<AgentManager>;
type SharedTrashEvents = watch::Sender<u64>;
type SharedSendStream = Arc<Mutex<web_transport_quinn::SendStream>>;

static NEXT_RPC_SESSION_ID: AtomicU64 = AtomicU64::new(1);

struct AbortTaskOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PairingAttemptKey {
    ClientId(String),
    Anonymous,
}

#[derive(Debug, Default)]
struct PairingState {
    next_attempt_id: PairingAttemptId,
    current_attempts: HashMap<PairingAttemptKey, PairingAttemptId>,
    active_challenge: Option<ActivePairingChallenge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivePairingChallenge {
    attempt_id: PairingAttemptId,
    attempt_key: PairingAttemptKey,
    owner_session_id: RpcSessionId,
    record: PairingRecord,
    client_label: String,
    client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingConfirmationRequest {
    pub daemon_url: String,
    pub confirmation_code: String,
    pub client_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCodeNotification {
    pub daemon_url: String,
    pub pairing_code: String,
    pub expires_in_seconds: i64,
}

pub trait PairingNotifier: Send + Sync {
    fn confirm_pairing_request(
        &self,
        request: PairingConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    fn notify_pairing_code(
        &self,
        notification: PairingCodeNotification,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    fn notify_pairing_completed(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct RpcSessionState {
    session_id: RpcSessionId,
    authenticated_client_id: Option<String>,
}

pub async fn run_system_server(
    listen_override: Option<SocketAddr>,
    config_path: PathBuf,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    pairing_notifier: Option<SharedPairingNotifier>,
    terminal_backend: Option<SharedTerminalBackend>,
    log_label: &'static str,
) -> Result<()> {
    loop {
        write_daemon_status(&config_path, DaemonStatus::NotReady("starting"));
        match run_system_server_once(
            listen_override,
            config_path.clone(),
            files.clone(),
            windows.clone(),
            process_resources_in_use.clone(),
            process_sockets_in_use.clone(),
            process_modules.clone(),
            pairing_notifier.clone(),
            terminal_backend.clone(),
            log_label,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err) => {
                write_daemon_status(&config_path, DaemonStatus::NotReady(&err.to_string()));
                warn!(
                    ?err,
                    config = %config_path.display(),
                    "system daemon config is not ready; waiting for config changes"
                );
                wait_for_startup_config_change(&config_path).await?;
            }
        }
    }
}

async fn run_system_server_once(
    listen_override: Option<SocketAddr>,
    config_path: PathBuf,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    pairing_notifier: Option<SharedPairingNotifier>,
    terminal_backend: Option<SharedTerminalBackend>,
    log_label: &'static str,
) -> Result<()> {
    let provider = web_transport_quinn::crypto::default_provider();
    let startup_config = load_startup_config(&config_path, listen_override)?;
    let mut config = startup_config.config;
    let addr = startup_config.listen_addr;
    let certificate = prepare_server_certificate(&mut config, addr, &config_path, &provider)?;
    let config_state = Arc::new(Mutex::new(config));
    let credentials_path = client_credentials_path(&config_path);
    let state_db_path = daemon_state_database_path(&config_path);
    let commands = Arc::new(CommandManager::open(DaemonStateDb::open(&state_db_path)?)?);
    let agents = Arc::new(AgentManager::open(
        DaemonStateDb::open(&state_db_path)?,
        daemon_agent_workspaces_path(&config_path),
    )?);
    info!(
        state_db = %state_db_path.display(),
        "daemon state database ready"
    );
    let initial_client_credentials = load_client_credentials_or_default(&credentials_path)?;
    let client_credentials = Arc::new(Mutex::new(initial_client_credentials.clone()));
    let (client_credentials_events, _) = watch::channel(initial_client_credentials);
    let pairing_challenge = Arc::new(Mutex::new(PairingState::default()));
    let terminals = Arc::new(match terminal_backend {
        Some(backend) => TerminalManager::with_backend(backend),
        None => TerminalManager::new(shell_integration_dir(&config_path)),
    });
    let (trash_events, _) = watch::channel(0);
    let rpc_handlers = Arc::new(build_rpc_handlers(
        windows.as_ref(),
        process_resources_in_use.as_ref(),
        process_sockets_in_use.as_ref(),
        process_modules.as_ref(),
    ));

    let resolver = Arc::new(ReloadingCertResolver::new(certificate.certified_key));
    let mut server = build_reloadable_server(addr, provider.clone(), resolver.clone())?;
    let _scheduler = AbortTaskOnDrop(commands.start_scheduler());
    write_daemon_status(&config_path, DaemonStatus::Ready);

    tokio::spawn(reload_certificates(
        config_path.clone(),
        addr,
        provider,
        resolver,
    ));

    info!(%addr, daemon = log_label, "rieul system daemon listening");

    while let Some(request) = server.accept().await {
        let config_path = config_path.clone();
        let credentials_path = credentials_path.clone();
        let config_state = config_state.clone();
        let client_credentials = client_credentials.clone();
        let client_credentials_events = client_credentials_events.clone();
        let pairing_challenge = pairing_challenge.clone();
        let files = files.clone();
        let windows = windows.clone();
        let process_resources_in_use = process_resources_in_use.clone();
        let process_sockets_in_use = process_sockets_in_use.clone();
        let process_modules = process_modules.clone();
        let rpc_handlers = rpc_handlers.clone();
        let terminals = terminals.clone();
        let commands = commands.clone();
        let agents = agents.clone();
        let trash_events = trash_events.clone();
        let pairing_notifier = pairing_notifier.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_request(
                request,
                config_path,
                credentials_path,
                config_state,
                client_credentials,
                client_credentials_events,
                pairing_challenge,
                files,
                windows,
                process_resources_in_use,
                process_sockets_in_use,
                process_modules,
                rpc_handlers,
                terminals,
                commands,
                agents,
                trash_events,
                pairing_notifier,
            )
            .await
            {
                warn!(?err, "WebTransport request failed");
            }
        });
    }
    Ok(())
}

async fn wait_for_startup_config_change(config_path: &Path) -> Result<()> {
    let (reload_tx, mut reload_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = match create_certificate_watcher(reload_tx) {
        Ok(watcher) => watcher,
        Err(err) => {
            warn!(
                ?err,
                config = %config_path.display(),
                "failed to watch config; retrying startup later"
            );
            tokio::time::sleep(CONFIG_STARTUP_RETRY_INTERVAL).await;
            return Ok(());
        }
    };

    let mut watch_state = CertificateWatchState::default();
    watch_config_parent(&mut watcher, &mut watch_state, config_path);
    match load_or_default(config_path) {
        Ok(config) => {
            if let Err(err) =
                update_certificate_watches(&mut watcher, &mut watch_state, config_path, &config)
            {
                warn!(?err, "failed to watch startup certificate paths");
            }
        }
        Err(err) => {
            warn!(
                ?err,
                config = %config_path.display(),
                "failed to read startup config while setting watches"
            );
        }
    }

    loop {
        tokio::select! {
            trigger = reload_rx.recv() => {
                let Some(trigger) = trigger else {
                    warn!(
                        config = %config_path.display(),
                        "config watcher stopped; retrying startup later"
                    );
                    tokio::time::sleep(CONFIG_STARTUP_RETRY_INTERVAL).await;
                    return Ok(());
                };
                let Some(trigger) = collect_reload_triggers(trigger, &mut reload_rx).await else {
                    continue;
                };
                match trigger {
                    CertificateReloadTrigger::Filesystem(paths) => {
                        if paths.is_empty()
                            || watch_state.config_changed(&paths)
                            || watch_state.certificate_changed(&paths)
                        {
                            return Ok(());
                        }
                    }
                    CertificateReloadTrigger::Scheduled => return Ok(()),
                }
            }
            _ = tokio::time::sleep(CONFIG_STARTUP_RETRY_INTERVAL) => return Ok(()),
        }
    }
}

struct StartupConfig {
    config: SystemConfig,
    listen_addr: SocketAddr,
}

fn load_startup_config(
    config_path: &Path,
    listen_override: Option<SocketAddr>,
) -> Result<StartupConfig> {
    let should_create = !config_path.exists();
    let mut config = load_or_generated_default(config_path)?;
    let listen_addr = match listen_override {
        Some(addr) => {
            config.listen_addr = addr.to_string();
            addr
        }
        None => parse_listen_addr(&config)?,
    };
    if should_create || listen_override.is_some() {
        save(config_path, &config)?;
    }
    Ok(StartupConfig {
        config,
        listen_addr,
    })
}

fn parse_listen_addr(config: &SystemConfig) -> Result<SocketAddr> {
    config
        .listen_addr
        .parse()
        .with_context(|| format!("invalid listenAddr `{}`", config.listen_addr))
}

enum DaemonStatus<'a> {
    Ready,
    NotReady(&'a str),
}

fn write_daemon_status(config_path: &Path, status: DaemonStatus<'_>) {
    let status_path = daemon_status_path(config_path);
    if let Some(parent) = status_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            warn!(
                ?err,
                path = %parent.display(),
                "failed to create daemon status directory"
            );
            return;
        }
    }

    let text = match status {
        DaemonStatus::Ready => "ready\n".to_string(),
        DaemonStatus::NotReady(reason) => format!("not-ready\n{reason}\n"),
    };
    if let Err(err) = fs::write(&status_path, text) {
        warn!(
            ?err,
            path = %status_path.display(),
            "failed to write daemon status"
        );
    }
}

fn shell_integration_dir(config_path: &Path) -> PathBuf {
    config_path.with_file_name("shell-integration")
}

#[derive(Debug)]
struct ReloadingCertResolver {
    current: RwLock<Arc<CertifiedKey>>,
}

impl ReloadingCertResolver {
    fn new(initial: Arc<CertifiedKey>) -> Self {
        Self {
            current: RwLock::new(initial),
        }
    }

    fn current_fingerprint(&self) -> Option<Vec<u8>> {
        self.current
            .read()
            .ok()
            .and_then(|current| current.cert.first().map(|cert| cert.to_vec()))
    }

    fn replace(&self, next: Arc<CertifiedKey>) -> Result<bool> {
        let Some(next_fingerprint) = next.cert.first().map(|cert| cert.to_vec()) else {
            bail!("certificate chain is empty");
        };
        let mut current = self
            .current
            .write()
            .map_err(|_| anyhow::anyhow!("certificate resolver lock is poisoned"))?;
        let changed = current
            .cert
            .first()
            .map(|cert| cert.as_ref() != next_fingerprint.as_slice())
            .unwrap_or(true);
        if changed {
            *current = next;
        }
        Ok(changed)
    }
}

impl ResolvesServerCert for ReloadingCertResolver {
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current.read().ok().map(|current| current.clone())
    }
}

fn build_reloadable_server(
    addr: SocketAddr,
    provider: web_transport_quinn::crypto::Provider,
    resolver: Arc<ReloadingCertResolver>,
) -> Result<web_transport_quinn::Server> {
    let mut tls_config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    tls_config.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];

    let quic_config: web_transport_quinn::quinn::crypto::rustls::QuicServerConfig = tls_config
        .try_into()
        .context("failed to build QUIC TLS config")?;
    let mut server_config =
        web_transport_quinn::quinn::ServerConfig::with_crypto(Arc::new(quic_config));
    let mut transport_config = web_transport_quinn::quinn::TransportConfig::default();
    transport_config
        .max_idle_timeout(Some(QUIC_MAX_IDLE_TIMEOUT.try_into()?))
        .keep_alive_interval(Some(QUIC_KEEP_ALIVE_INTERVAL));
    server_config.transport_config(Arc::new(transport_config));
    let endpoint = web_transport_quinn::quinn::Endpoint::server(server_config, addr)
        .context("failed to bind QUIC endpoint")?;
    Ok(web_transport_quinn::Server::new(endpoint))
}

async fn reload_certificates(
    config_path: PathBuf,
    addr: SocketAddr,
    provider: web_transport_quinn::crypto::Provider,
    resolver: Arc<ReloadingCertResolver>,
) {
    let (reload_tx, mut reload_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = match create_certificate_watcher(reload_tx.clone()) {
        Ok(watcher) => watcher,
        Err(err) => {
            warn!(
                ?err,
                "failed to create certificate watcher; scheduled refresh remains active"
            );
            schedule_certificate_refreshes(reload_tx);
            return scheduled_reload_loop(config_path, addr, provider, resolver, &mut reload_rx)
                .await;
        }
    };

    let mut watch_state = CertificateWatchState::default();
    match load_or_default(&config_path) {
        Ok(config) => {
            if let Err(err) =
                update_certificate_watches(&mut watcher, &mut watch_state, &config_path, &config)
            {
                warn!(?err, "failed to initialize certificate watches");
            }
        }
        Err(err) => {
            warn!(?err, config = %config_path.display(), "failed to read config for certificate watcher setup");
            watch_config_parent(&mut watcher, &mut watch_state, &config_path);
        }
    }

    schedule_certificate_refreshes(reload_tx);

    loop {
        let Some(trigger) = reload_rx.recv().await else {
            break;
        };

        let trigger = match collect_reload_triggers(trigger, &mut reload_rx).await {
            Some(trigger) => trigger,
            None => continue,
        };

        let mut config = match load_or_default(&config_path) {
            Ok(config) => config,
            Err(err) => {
                write_daemon_status(&config_path, DaemonStatus::NotReady(&err.to_string()));
                warn!(?err, config = %config_path.display(), "failed to read config for certificate reload");
                continue;
            }
        };
        let next_reload_key = certificate_reload_key(&config);
        let filesystem_trigger = matches!(&trigger, CertificateReloadTrigger::Filesystem(_));
        let should_reload = match trigger {
            CertificateReloadTrigger::Scheduled => uses_scheduled_certificate_refresh(&config),
            CertificateReloadTrigger::Filesystem(paths) => {
                if watch_state.config_changed(&paths) {
                    next_reload_key != watch_state.reload_key
                } else {
                    watch_state.certificate_changed(&paths) || paths.is_empty()
                }
            }
        };

        if !should_reload {
            if let Err(err) =
                update_certificate_watches(&mut watcher, &mut watch_state, &config_path, &config)
            {
                warn!(?err, "failed to update certificate watches");
            }
            continue;
        }

        let response = (|| -> Result<()> {
            let certificate =
                prepare_server_certificate(&mut config, addr, &config_path, &provider)?;
            save(&config_path, &config)?;
            if resolver.replace(certificate.certified_key)? {
                info!("reloaded WebTransport TLS certificate");
            }
            Ok(())
        })();

        if let Err(err) = response {
            if filesystem_trigger {
                write_daemon_status(&config_path, DaemonStatus::NotReady(&err.to_string()));
            }
            warn!(
                ?err,
                "certificate reload failed; keeping previous certificate"
            );
            if resolver.current_fingerprint().is_none() {
                warn!("certificate resolver has no usable certificate");
            }
        } else {
            write_daemon_status(&config_path, DaemonStatus::Ready);
            if let Err(err) =
                update_certificate_watches(&mut watcher, &mut watch_state, &config_path, &config)
            {
                warn!(?err, "failed to update certificate watches");
            }
        }
    }
}

async fn scheduled_reload_loop(
    config_path: PathBuf,
    addr: SocketAddr,
    provider: web_transport_quinn::crypto::Provider,
    resolver: Arc<ReloadingCertResolver>,
    reload_rx: &mut tokio::sync::mpsc::UnboundedReceiver<CertificateReloadTrigger>,
) {
    while let Some(trigger) = reload_rx.recv().await {
        if !matches!(trigger, CertificateReloadTrigger::Scheduled) {
            continue;
        }
        let response = (|| -> Result<()> {
            let mut config = load_or_default(&config_path)?;
            if !uses_scheduled_certificate_refresh(&config) {
                return Ok(());
            }
            let certificate =
                prepare_server_certificate(&mut config, addr, &config_path, &provider)?;
            save(&config_path, &config)?;
            resolver.replace(certificate.certified_key)?;
            Ok(())
        })();
        if let Err(err) = response {
            warn!(?err, "scheduled certificate reload failed");
        }
    }
}

fn create_certificate_watcher(
    reload_tx: tokio::sync::mpsc::UnboundedSender<CertificateReloadTrigger>,
) -> Result<notify::RecommendedWatcher> {
    Ok(notify::recommended_watcher(
        move |event: notify::Result<Event>| match event {
            Ok(event) => {
                let _ = reload_tx.send(CertificateReloadTrigger::Filesystem(event.paths));
            }
            Err(err) => {
                warn!(?err, "certificate watcher event failed");
            }
        },
    )?)
}

fn schedule_certificate_refreshes(
    reload_tx: tokio::sync::mpsc::UnboundedSender<CertificateReloadTrigger>,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SCHEDULED_CERT_REFRESH_INTERVAL).await;
            if reload_tx.send(CertificateReloadTrigger::Scheduled).is_err() {
                break;
            }
        }
    });
}

async fn collect_reload_triggers(
    first: CertificateReloadTrigger,
    reload_rx: &mut tokio::sync::mpsc::UnboundedReceiver<CertificateReloadTrigger>,
) -> Option<CertificateReloadTrigger> {
    if matches!(first, CertificateReloadTrigger::Scheduled) {
        return Some(first);
    }

    tokio::time::sleep(CERT_RELOAD_DEBOUNCE).await;
    let mut paths = match first {
        CertificateReloadTrigger::Filesystem(paths) => paths,
        CertificateReloadTrigger::Scheduled => return Some(CertificateReloadTrigger::Scheduled),
    };

    while let Ok(trigger) = reload_rx.try_recv() {
        match trigger {
            CertificateReloadTrigger::Filesystem(next_paths) => paths.extend(next_paths),
            CertificateReloadTrigger::Scheduled => {
                return Some(CertificateReloadTrigger::Scheduled);
            }
        }
    }

    Some(CertificateReloadTrigger::Filesystem(paths))
}

#[derive(Debug)]
enum CertificateReloadTrigger {
    Filesystem(Vec<PathBuf>),
    Scheduled,
}

#[derive(Default)]
struct CertificateWatchState {
    watched_dirs: HashSet<PathBuf>,
    config_file: PathBuf,
    certificate_files: HashSet<PathBuf>,
    reload_key: String,
}

impl CertificateWatchState {
    fn config_changed(&self, paths: &[PathBuf]) -> bool {
        paths
            .iter()
            .any(|path| normalized_path_key(path) == normalized_path_key(&self.config_file))
    }

    fn certificate_changed(&self, paths: &[PathBuf]) -> bool {
        paths.iter().any(|path| {
            let path = normalized_path_key(path);
            self.certificate_files
                .iter()
                .any(|cert_path| path == normalized_path_key(cert_path))
        })
    }
}

fn update_certificate_watches(
    watcher: &mut notify::RecommendedWatcher,
    state: &mut CertificateWatchState,
    config_path: &Path,
    config: &SystemConfig,
) -> Result<()> {
    state.config_file = absolute_path(config_path);
    state.reload_key = certificate_reload_key(config);
    state.certificate_files = configured_certificate_paths(config, config_path)?
        .into_iter()
        .map(|path| absolute_path(&path))
        .collect();

    watch_config_parent(watcher, state, config_path);
    for path in state.certificate_files.clone() {
        watch_parent_dir(watcher, state, &path);
    }

    Ok(())
}

fn watch_config_parent(
    watcher: &mut notify::RecommendedWatcher,
    state: &mut CertificateWatchState,
    config_path: &Path,
) {
    state.config_file = absolute_path(config_path);
    watch_parent_dir(watcher, state, config_path);
}

fn watch_parent_dir(
    watcher: &mut notify::RecommendedWatcher,
    state: &mut CertificateWatchState,
    path: &Path,
) {
    let Some(parent) = path.parent() else {
        return;
    };
    let parent = absolute_path(parent);
    if !state.watched_dirs.insert(parent.clone()) {
        return;
    }
    if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
        warn!(?err, path = %parent.display(), "failed to watch certificate directory");
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn normalized_path_key(path: &Path) -> String {
    absolute_path(path)
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn certificate_reload_key(config: &SystemConfig) -> String {
    if let Some(tls) = &config.tls {
        format!("tls:{}:{}", tls.cert_file, tls.key_file)
    } else if let Some(domain) = config.domain.as_deref() {
        format!(
            "domain:{}",
            domain.trim().trim_end_matches('.').to_ascii_lowercase()
        )
    } else {
        "unconfigured".to_string()
    }
}

async fn handle_request(
    request: web_transport_quinn::Request,
    config_path: PathBuf,
    credentials_path: PathBuf,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    rpc_handlers: SharedHostRpcHandlers,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    agents: SharedAgentManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    let path = request.url.path().to_string();
    match path.as_str() {
        "/rieul/rpc" => {
            let session = request.respond(ConnectResponse::OK).await?;
            run_rpc_session(
                session,
                config_path,
                credentials_path,
                config_state,
                client_credentials,
                client_credentials_events,
                pairing_challenge,
                files,
                windows,
                process_resources_in_use,
                process_sockets_in_use,
                process_modules,
                rpc_handlers,
                terminals,
                commands,
                agents,
                trash_events,
                pairing_notifier,
            )
            .await
        }
        _ => {
            request.reject(http::StatusCode::NOT_FOUND).await?;
            Ok(())
        }
    }
}

async fn run_rpc_session(
    session: web_transport_quinn::Session,
    config_path: PathBuf,
    credentials_path: PathBuf,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    rpc_handlers: SharedHostRpcHandlers,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    agents: SharedAgentManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    let session_state = Arc::new(Mutex::new(RpcSessionState {
        session_id: next_rpc_session_id(),
        authenticated_client_id: None,
    }));
    loop {
        tokio::select! {
            stream = session.accept_bi() => {
                let (send, recv) = stream?;
                let config_path = config_path.clone();
                let credentials_path = credentials_path.clone();
                let config_state = config_state.clone();
                let client_credentials = client_credentials.clone();
                let client_credentials_events = client_credentials_events.clone();
                let pairing_challenge = pairing_challenge.clone();
                let session_state = session_state.clone();
                let files = files.clone();
                let windows = windows.clone();
                let process_resources_in_use = process_resources_in_use.clone();
                let process_sockets_in_use = process_sockets_in_use.clone();
                let process_modules = process_modules.clone();
                let rpc_handlers = rpc_handlers.clone();
                let terminals = terminals.clone();
                let commands = commands.clone();
                let agents = agents.clone();
                let trash_events = trash_events.clone();
                let pairing_notifier = pairing_notifier.clone();
                let stream_session = session.clone();
                tokio::spawn(async move {
                    let response = async {
                        handle_reqres_stream(
                            recv,
                            send,
                            stream_session,
                            config_path,
                            credentials_path,
                            config_state,
                            client_credentials,
                            client_credentials_events,
                            pairing_challenge,
                            session_state,
                            files,
                            windows,
                            process_resources_in_use,
                            process_sockets_in_use,
                            process_modules,
                            rpc_handlers,
                            terminals,
                            commands,
                            agents,
                            trash_events,
                            pairing_notifier,
                        )
                        .await?;
                        Result::<()>::Ok(())
                    }
                    .await;
                    if let Err(err) = response {
                        warn!(?err, "RPC stream failed");
                    }
                });
            }
            datagram = session.read_datagram() => {
                let datagram = datagram?;
                if let Some(response) = handle_wire_datagram(&datagram) {
                    if response.len() > session.max_datagram_size() {
                        warn!(size = response.len(), max = session.max_datagram_size(), "datagram response exceeds transport limit");
                    } else if let Err(err) = session.send_datagram(response.into()) {
                        warn!(?err, "failed to send datagram response");
                    }
                }
            }
        }
    }
}

fn handle_wire_datagram(bytes: &[u8]) -> Option<Vec<u8>> {
    match DatagramMessage::decode(bytes) {
        Ok(DatagramMessage::Ping { ping_id }) => Some(DatagramMessage::Pong { ping_id }.encode()),
        Ok(DatagramMessage::Pong { .. }) => None,
        Err(err) => {
            warn!(?err, "ignoring malformed datagram message");
            None
        }
    }
}

type HostRpcHandlers =
    RpcHandlers<HostRpcHandler, Result<UnaryRpcOutcome>, Result<()>, Result<Vec<ReqResMessage>>>;

fn build_rpc_handlers(
    windows: Option<&SharedWindowService>,
    process_resources_in_use: Option<&SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<&SharedProcessSocketsInUseService>,
    process_modules: Option<&SharedProcessModulesService>,
) -> HostRpcHandlers {
    let mut handlers = RpcHandlers::new()
        .get_daemon_info(HostRpcHandler::get_daemon_info_rpc)
        .get_daemon_environment(HostRpcHandler::get_daemon_environment_rpc)
        .start_pairing(HostRpcHandler::start_pairing_rpc)
        .complete_pairing(HostRpcHandler::complete_pairing_rpc)
        .renew_client_credential(HostRpcHandler::renew_client_credential_rpc)
        .remove_client(HostRpcHandler::remove_client_rpc)
        .subscribe_roots(HostRpcHandler::subscribe_roots_rpc)
        .subscribe_directory(HostRpcHandler::subscribe_directory_rpc)
        .read_file(HostRpcHandler::read_file_rpc)
        .write_file(HostRpcHandler::write_file_rpc)
        .create_nodes(HostRpcHandler::create_nodes_rpc)
        .rename_paths(HostRpcHandler::rename_paths_rpc)
        .delete_paths(HostRpcHandler::delete_paths_rpc)
        .create_terminal_session(HostRpcHandler::create_terminal_session_rpc)
        .subscribe_terminal_sessions(HostRpcHandler::subscribe_terminal_sessions_rpc)
        .subscribe_available_shells(HostRpcHandler::subscribe_available_shells_rpc)
        .attach_terminal_session(HostRpcHandler::attach_terminal_session_rpc)
        .take_terminal_control(HostRpcHandler::take_terminal_control_rpc)
        .write_terminal_input(HostRpcHandler::write_terminal_input_rpc)
        .close_terminal_session(HostRpcHandler::close_terminal_session_rpc)
        .subscribe_clients(HostRpcHandler::subscribe_clients_rpc)
        .subscribe_trash_items(HostRpcHandler::subscribe_trash_items_rpc)
        .restore_trash_items(HostRpcHandler::restore_trash_items_rpc)
        .purge_trash_items(HostRpcHandler::purge_trash_items_rpc)
        .subscribe_processes(HostRpcHandler::subscribe_processes_rpc)
        .kill_process(HostRpcHandler::kill_process_rpc)
        .subscribe_process_detail(HostRpcHandler::subscribe_process_detail_rpc)
        .run_command(HostRpcHandler::run_command_rpc)
        .create_job(HostRpcHandler::create_job_rpc)
        .subscribe_jobs(HostRpcHandler::subscribe_jobs_rpc)
        .subscribe_job_output(HostRpcHandler::subscribe_job_output_rpc)
        .kill_job(HostRpcHandler::kill_job_rpc)
        .delete_jobs(HostRpcHandler::delete_jobs_rpc)
        .clear_jobs(HostRpcHandler::clear_jobs_rpc)
        .create_schedule(HostRpcHandler::create_schedule_rpc)
        .update_schedule(HostRpcHandler::update_schedule_rpc)
        .subscribe_schedules(HostRpcHandler::subscribe_schedules_rpc)
        .delete_schedules(HostRpcHandler::delete_schedules_rpc)
        .get_schedule_next_runs(HostRpcHandler::get_schedule_next_runs_rpc)
        .subscribe_agent_providers(HostRpcHandler::subscribe_agent_providers_rpc)
        .create_agent_project(HostRpcHandler::create_agent_project_rpc)
        .subscribe_agent_projects(HostRpcHandler::subscribe_agent_projects_rpc)
        .list_agent_sessions(HostRpcHandler::list_agent_sessions_rpc)
        .create_agent_session(HostRpcHandler::create_agent_session_rpc)
        .attach_agent_session(HostRpcHandler::attach_agent_session_rpc)
        .subscribe_agent_session(HostRpcHandler::subscribe_agent_session_rpc)
        .create_agent_turn(HostRpcHandler::create_agent_turn_rpc)
        .list_agent_session_turns(HostRpcHandler::list_agent_session_turns_rpc)
        .set_agent_session_config(HostRpcHandler::set_agent_session_config_rpc)
        .update_agent_session(HostRpcHandler::update_agent_session_rpc)
        .remove_agent_project(HostRpcHandler::remove_agent_project_rpc);

    if process_resources_in_use.is_some() {
        handlers = handlers.subscribe_process_resources_in_use(
            HostRpcHandler::subscribe_process_resources_in_use_rpc,
        );
    }
    if process_sockets_in_use.is_some() {
        handlers = handlers
            .subscribe_process_sockets_in_use(HostRpcHandler::subscribe_process_sockets_in_use_rpc);
    }
    if process_modules.is_some() {
        handlers =
            handlers.subscribe_process_modules(HostRpcHandler::subscribe_process_modules_rpc);
    }
    if windows.is_some() {
        handlers = handlers
            .subscribe_windows(HostRpcHandler::subscribe_windows_rpc)
            .subscribe_window_detail(HostRpcHandler::subscribe_window_detail_rpc);
    }
    handlers
}

fn is_server_stream_proc(proc_id: u64) -> bool {
    ProcId::from_u64(proc_id).is_some_and(|proc| proc.stream() == ProcStream::Server)
}

fn is_client_stream_proc(proc_id: u64) -> bool {
    ProcId::from_u64(proc_id).is_some_and(|proc| proc.stream() == ProcStream::Client)
}

struct ReqResMessageReader {
    recv: web_transport_quinn::RecvStream,
    buffer: Vec<u8>,
    bytes_read: usize,
    finished: bool,
}

impl ReqResMessageReader {
    fn new(recv: web_transport_quinn::RecvStream) -> Self {
        Self {
            recv,
            buffer: Vec::new(),
            bytes_read: 0,
            finished: false,
        }
    }

    async fn next(&mut self) -> Result<Option<ReqResMessage>> {
        loop {
            if let Some((message, consumed)) = ReqResMessage::decode_prefix(&self.buffer)? {
                self.buffer.drain(..consumed);
                return Ok(Some(message));
            }
            if self.finished {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                bail!("reqres message sequence ended with an incomplete message");
            }
            self.read_more().await?;
        }
    }

    async fn read_more(&mut self) -> Result<()> {
        let mut chunk = [0u8; 8192];
        let Some(len) = self.recv.read(&mut chunk).await? else {
            self.finished = true;
            return Ok(());
        };
        if len == 0 {
            self.finished = true;
            return Ok(());
        }
        self.bytes_read += len;
        if self.bytes_read > MAX_MESSAGE_SEQUENCE_SIZE {
            bail!("reqres message sequence exceeds implementation limit");
        }
        self.buffer.extend_from_slice(&chunk[..len]);
        Ok(())
    }
}

async fn read_remaining_reqres_messages(
    reader: &mut ReqResMessageReader,
) -> Result<Vec<ReqResMessage>> {
    let mut messages = Vec::new();
    while let Some(message) = reader.next().await? {
        messages.push(message);
    }
    Ok(messages)
}

#[cfg(test)]
async fn handle_reqres_messages(
    messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    terminals: SharedTerminalManager,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    let client_credentials_events = temporary_client_credentials_events(&client_credentials).await;
    let (trash_events, _) = watch::channel(0);
    let commands = test_commands();
    handle_reqres_messages_with_events(
        messages,
        config_path,
        credentials_path,
        config_state,
        client_credentials,
        client_credentials_events,
        pairing_challenge,
        session_state,
        files,
        None,
        terminals,
        commands,
        trash_events,
        pairing_notifier,
    )
    .await
}

#[cfg(test)]
async fn handle_reqres_messages_with_events(
    messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    let rpc_handlers = Arc::new(build_rpc_handlers(windows.as_ref(), None, None, None));
    let context = HostRpcContext {
        session: None,
        config_path: config_path.to_path_buf(),
        credentials_path: credentials_path.to_path_buf(),
        config_state,
        client_credentials,
        client_credentials_events,
        pairing_challenge,
        session_state,
        files,
        windows,
        process_resources_in_use: None,
        process_sockets_in_use: None,
        process_modules: None,
        rpc_handlers,
        terminals,
        commands,
        agents: test_agents(),
        trash_events,
        pairing_notifier,
    };
    dispatch_buffered_reqres_invocation(messages, context).await
}

async fn handle_reqres_stream(
    recv: web_transport_quinn::RecvStream,
    mut send: web_transport_quinn::SendStream,
    session: web_transport_quinn::Session,
    config_path: PathBuf,
    credentials_path: PathBuf,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    rpc_handlers: SharedHostRpcHandlers,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    agents: SharedAgentManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    let context = HostRpcContext {
        session: Some(session),
        config_path,
        credentials_path,
        config_state,
        client_credentials,
        client_credentials_events,
        pairing_challenge,
        session_state,
        files,
        windows,
        process_resources_in_use,
        process_sockets_in_use,
        process_modules,
        rpc_handlers,
        terminals,
        commands,
        agents,
        trash_events,
        pairing_notifier,
    };
    let mut reader = ReqResMessageReader::new(recv);
    let Some(first) = reader.next().await.context("invalid reqres message")? else {
        write_reqres_message(
            &mut send,
            generic_error_message(
                0,
                RpcErrorCode::BadMessage,
                "reqres message sequence is empty",
            ),
        )
        .await?;
        return Ok(());
    };

    match first {
        message if message.is_session_control() => {
            let mut messages = vec![message];
            messages.extend(read_remaining_reqres_messages(&mut reader).await?);
            let responses = handle_session_control_messages(
                messages,
                context.client_credentials,
                context.session_state,
            )
            .await?;
            write_reqres_messages(&mut send, &responses).await
        }
        ReqResMessage::RequestUnary { proc_id, payload } if is_server_stream_proc(proc_id) => {
            if reader.next().await?.is_some() {
                write_reqres_message(
                    &mut send,
                    stream_generic_error_message(
                        proc_id,
                        RpcErrorCode::BadMessage,
                        "server-stream request sequence may contain only one message",
                    ),
                )
                .await?;
                send.finish()?;
                return Ok(());
            }
            let shared_send = Arc::new(Mutex::new(send));
            dispatch_server_stream_rpc(proc_id, payload, shared_send.clone(), context).await?;
            shared_send.lock().await.finish()?;
            Ok(())
        }
        ReqResMessage::RequestUnary { proc_id, payload } => {
            if reader.next().await?.is_some() {
                write_reqres_message(
                    &mut send,
                    generic_error_message(
                        proc_id,
                        RpcErrorCode::BadMessage,
                        "unary request sequence may contain only one message",
                    ),
                )
                .await?;
                return Ok(());
            }
            let responses = dispatch_unary_rpc(proc_id, payload, context).await?;
            write_reqres_messages(&mut send, &responses).await?;
            send.finish()?;
            Ok(())
        }
        ReqResMessage::RequestStreamStart { proc_id, payload } => {
            let responses = dispatch_client_stream_rpc(
                proc_id,
                payload,
                RequestStreamSource::Live(reader),
                context,
            )
            .await?;
            write_reqres_messages(&mut send, &responses).await?;
            send.finish()?;
            Ok(())
        }
        _ => {
            write_reqres_message(
                &mut send,
                generic_error_message(
                    0,
                    RpcErrorCode::BadMessage,
                    "reqres stream must start with a request message",
                ),
            )
            .await?;
            send.finish()?;
            Ok(())
        }
    }
}

#[derive(Clone)]
struct HostRpcContext {
    session: Option<web_transport_quinn::Session>,
    config_path: PathBuf,
    credentials_path: PathBuf,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    rpc_handlers: SharedHostRpcHandlers,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    agents: SharedAgentManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
}

#[cfg(test)]
enum ReqResInvocation {
    SessionControl(Vec<ReqResMessage>),
    Unary {
        proc_id: u64,
        payload: Option<Vec<u8>>,
    },
    ServerStream {
        proc_id: u64,
    },
    ClientStream {
        proc_id: u64,
        payload: Option<Vec<u8>>,
        chunks: Vec<ReqResMessage>,
    },
}

#[cfg(test)]
fn parse_reqres_invocation(
    mut messages: Vec<ReqResMessage>,
) -> Result<ReqResInvocation, Vec<ReqResMessage>> {
    let Some(first) = messages.first() else {
        return Err(vec![generic_error_message(
            0,
            RpcErrorCode::BadMessage,
            "reqres message sequence is empty",
        )]);
    };
    if first.is_session_control() {
        return Ok(ReqResInvocation::SessionControl(messages));
    }

    match messages.remove(0) {
        ReqResMessage::RequestUnary { proc_id, payload } => {
            if !messages.is_empty() {
                return Err(vec![generic_error_message(
                    proc_id,
                    RpcErrorCode::BadMessage,
                    "unary request sequence may contain only one message",
                )]);
            }
            if is_server_stream_proc(proc_id) {
                Ok(ReqResInvocation::ServerStream { proc_id })
            } else {
                Ok(ReqResInvocation::Unary { proc_id, payload })
            }
        }
        ReqResMessage::RequestStreamStart { proc_id, payload } => {
            Ok(ReqResInvocation::ClientStream {
                proc_id,
                payload,
                chunks: messages,
            })
        }
        _ => Err(vec![generic_error_message(
            0,
            RpcErrorCode::BadMessage,
            "reqres stream must start with a request message",
        )]),
    }
}

#[cfg(test)]
async fn dispatch_buffered_reqres_invocation(
    messages: Vec<ReqResMessage>,
    context: HostRpcContext,
) -> Result<Vec<ReqResMessage>> {
    match parse_reqres_invocation(messages) {
        Ok(invocation) => dispatch_buffered_invocation(invocation, context).await,
        Err(responses) => Ok(responses),
    }
}

#[cfg(test)]
async fn dispatch_buffered_invocation(
    invocation: ReqResInvocation,
    context: HostRpcContext,
) -> Result<Vec<ReqResMessage>> {
    match invocation {
        ReqResInvocation::SessionControl(messages) => {
            handle_session_control_messages(
                messages,
                context.client_credentials,
                context.session_state,
            )
            .await
        }
        ReqResInvocation::Unary { proc_id, payload } => {
            dispatch_unary_rpc(proc_id, payload, context).await
        }
        ReqResInvocation::ClientStream {
            proc_id,
            payload,
            chunks,
        } => {
            dispatch_client_stream_rpc(
                proc_id,
                payload,
                RequestStreamSource::Buffered(chunks.into_iter()),
                context,
            )
            .await
        }
        ReqResInvocation::ServerStream { proc_id, .. } => Ok(vec![stream_generic_error_message(
            proc_id,
            RpcErrorCode::BadMessage,
            "server-streaming RPCs must be handled by the reqres stream handler",
        )]),
    }
}

async fn dispatch_server_stream_rpc(
    proc_id: u64,
    payload: Option<Vec<u8>>,
    send: SharedSendStream,
    context: HostRpcContext,
) -> Result<()> {
    if requires_authentication(proc_id) && !is_authenticated(&context.session_state).await {
        let mut send = send.lock().await;
        write_reqres_message(
            &mut send,
            stream_generic_error_message(
                proc_id,
                RpcErrorCode::Unauthorized,
                "valid paired client credentials are required",
            ),
        )
        .await?;
        return Ok(());
    }

    let payload = payload.as_deref();
    let request = match RpcRequest::decode(proc_id, payload) {
        Ok(request) => request,
        Err(err) => {
            let mut send = send.lock().await;
            write_reqres_message(
                &mut send,
                stream_rpc_request_decode_error_message(proc_id, err),
            )
            .await?;
            return Ok(());
        }
    };
    let request_proc_id = request.proc_id().as_u64();
    let rpc_handlers = context.rpc_handlers.clone();
    let mut handler = HostRpcHandler::new(context, Some(send.clone()), None);
    let Some(result) = rpc_handlers
        .dispatch_server_stream(&mut handler, request)
        .await
    else {
        let mut send = send.lock().await;
        write_reqres_message(
            &mut send,
            stream_error_message(
                request_proc_id,
                "not_implemented",
                "this RPC is reserved but not implemented in the first cut",
            ),
        )
        .await?;
        return Ok(());
    };
    result
}

async fn dispatch_client_stream_rpc(
    proc_id: u64,
    payload: Option<Vec<u8>>,
    request_stream: RequestStreamSource,
    context: HostRpcContext,
) -> Result<Vec<ReqResMessage>> {
    if !is_client_stream_proc(proc_id) {
        return Ok(vec![generic_error_message(
            proc_id,
            RpcErrorCode::BadMessage,
            "this RPC does not accept a request stream",
        )]);
    }
    if requires_authentication(proc_id) && !is_authenticated(&context.session_state).await {
        return Ok(vec![unauthorized_message(proc_id)]);
    }
    let request = match RpcRequest::decode(proc_id, payload.as_deref()) {
        Ok(request) => request,
        Err(err) => return Ok(vec![rpc_request_decode_error_message(proc_id, err)]),
    };
    let request_proc_id = request.proc_id().as_u64();
    let rpc_handlers = context.rpc_handlers.clone();
    let mut handler = HostRpcHandler::new(context, None, Some(request_stream));
    let Some(result) = rpc_handlers
        .dispatch_client_stream(&mut handler, request)
        .await
    else {
        return Ok(vec![error_message(
            request_proc_id,
            "not_implemented",
            "this RPC is reserved but not implemented in the first cut",
        )]);
    };
    result
}

struct HostRpcHandler {
    session: Option<web_transport_quinn::Session>,
    config_path: PathBuf,
    credentials_path: PathBuf,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    windows: Option<SharedWindowService>,
    process_resources_in_use: Option<SharedProcessResourcesInUseService>,
    process_sockets_in_use: Option<SharedProcessSocketsInUseService>,
    process_modules: Option<SharedProcessModulesService>,
    rpc_handlers: SharedHostRpcHandlers,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    agents: SharedAgentManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
    send: Option<SharedSendStream>,
    request_stream: Option<RequestStreamSource>,
}

enum RequestStreamSource {
    Live(ReqResMessageReader),
    #[cfg(test)]
    Buffered(std::vec::IntoIter<ReqResMessage>),
}

impl RequestStreamSource {
    async fn next(&mut self) -> Result<Option<ReqResMessage>> {
        match self {
            Self::Live(reader) => reader.next().await,
            #[cfg(test)]
            Self::Buffered(messages) => Ok(messages.next()),
        }
    }
}

impl HostRpcHandler {
    fn new(
        context: HostRpcContext,
        send: Option<SharedSendStream>,
        request_stream: Option<RequestStreamSource>,
    ) -> Self {
        HostRpcHandler {
            session: context.session,
            config_path: context.config_path,
            credentials_path: context.credentials_path,
            config_state: context.config_state,
            client_credentials: context.client_credentials,
            client_credentials_events: context.client_credentials_events,
            pairing_challenge: context.pairing_challenge,
            session_state: context.session_state,
            files: context.files,
            windows: context.windows,
            process_resources_in_use: context.process_resources_in_use,
            process_sockets_in_use: context.process_sockets_in_use,
            process_modules: context.process_modules,
            rpc_handlers: context.rpc_handlers,
            terminals: context.terminals,
            commands: context.commands,
            agents: context.agents,
            trash_events: context.trash_events,
            pairing_notifier: context.pairing_notifier,
            send,
            request_stream,
        }
    }

    fn response_stream(&self) -> SharedSendStream {
        self.send
            .as_ref()
            .expect("server-stream RPC requires a response stream")
            .clone()
    }

    async fn next_request_stream_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        let Some(source) = self.request_stream.as_mut() else {
            bail!("client-stream RPC requires a request stream");
        };
        match source.next().await? {
            Some(ReqResMessage::RequestStreamChunk { payload }) => Ok(Some(payload)),
            Some(_) => bail!("client-stream RPC received a non-chunk request message"),
            None => Ok(None),
        }
    }
}

impl HostRpcHandler {
    async fn subscribe_roots(&mut self, _: ()) -> Result<()> {
        let files = self.files.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_roots_subscription(&mut send, files, ProcId::SubscribeRoots.as_u64()).await
    }

    async fn subscribe_directory(&mut self, request: SubscribeDirectoryReq) -> Result<()> {
        let files = self.files.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_directory_subscription(
            &mut send,
            files,
            ProcId::SubscribeDirectory.as_u64(),
            request.path,
        )
        .await
    }

    async fn read_file(&mut self, request: ReadFileReq) -> Result<()> {
        let files = self.files.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_read_file(&mut send, files, ProcId::ReadFile.as_u64(), request).await
    }

    async fn subscribe_terminal_sessions(&mut self, _: ()) -> Result<()> {
        let terminals = self.terminals.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_terminal_sessions_subscription(
            &mut send,
            terminals,
            ProcId::SubscribeTerminalSessions.as_u64(),
        )
        .await
    }

    async fn subscribe_available_shells(&mut self, _: ()) -> Result<()> {
        let terminals = self.terminals.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_available_shells_subscription(
            &mut send,
            terminals,
            ProcId::SubscribeAvailableShells.as_u64(),
        )
        .await
    }

    async fn attach_terminal_session(&mut self, request: AttachTerminalSessionReq) -> Result<()> {
        let terminals = self.terminals.clone();
        let session_state = self.session_state.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_attach_terminal_session(
            &mut send,
            terminals,
            session_state,
            ProcId::AttachTerminalSession.as_u64(),
            request,
        )
        .await
    }

    async fn subscribe_clients(&mut self, _: ()) -> Result<()> {
        let client_credentials_events = self.client_credentials_events.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_clients_subscription(&mut send, client_credentials_events).await
    }

    async fn subscribe_trash_items(&mut self, _: ()) -> Result<()> {
        let files = self.files.clone();
        let trash_events = self.trash_events.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_trash_items_subscription(
            &mut send,
            files,
            trash_events,
            ProcId::SubscribeTrashItems.as_u64(),
        )
        .await
    }

    async fn subscribe_processes(&mut self, _: ()) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_processes_subscription(&mut send, ProcId::SubscribeProcesses.as_u64()).await
    }

    async fn subscribe_process_detail(&mut self, request: SubscribeProcessDetailReq) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_process_detail_subscription(
            &mut send,
            ProcId::SubscribeProcessDetail.as_u64(),
            request.pid,
        )
        .await
    }

    async fn subscribe_process_resources_in_use(
        &mut self,
        request: SubscribeProcessResourcesInUseReq,
    ) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        let Some(process_resources_in_use) = self.process_resources_in_use.clone() else {
            write_reqres_message(
                &mut send,
                stream_service_error_message(
                    ProcId::SubscribeProcessResourcesInUse.as_u64(),
                    ServiceError::Unsupported,
                ),
            )
            .await?;
            return Ok(());
        };
        stream_process_resources_in_use_subscription(
            &mut send,
            process_resources_in_use,
            ProcId::SubscribeProcessResourcesInUse.as_u64(),
            request.pid,
        )
        .await
    }

    async fn subscribe_process_sockets_in_use(
        &mut self,
        request: SubscribeProcessSocketsInUseReq,
    ) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        let Some(process_sockets_in_use) = self.process_sockets_in_use.clone() else {
            write_reqres_message(
                &mut send,
                stream_service_error_message(
                    ProcId::SubscribeProcessSocketsInUse.as_u64(),
                    ServiceError::Unsupported,
                ),
            )
            .await?;
            return Ok(());
        };
        stream_process_sockets_in_use_subscription(
            &mut send,
            process_sockets_in_use,
            ProcId::SubscribeProcessSocketsInUse.as_u64(),
            request.pid,
        )
        .await
    }

    async fn subscribe_process_modules(
        &mut self,
        request: SubscribeProcessModulesReq,
    ) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        let Some(process_modules) = self.process_modules.clone() else {
            write_reqres_message(
                &mut send,
                stream_service_error_message(
                    ProcId::SubscribeProcessModules.as_u64(),
                    ServiceError::Unsupported,
                ),
            )
            .await?;
            return Ok(());
        };
        stream_process_modules_subscription(
            &mut send,
            process_modules,
            ProcId::SubscribeProcessModules.as_u64(),
            request.pid,
        )
        .await
    }

    async fn subscribe_windows(&mut self, _: ()) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        let Some(windows) = self.windows.clone() else {
            write_reqres_message(
                &mut send,
                stream_service_error_message(
                    ProcId::SubscribeWindows.as_u64(),
                    ServiceError::Unsupported,
                ),
            )
            .await?;
            return Ok(());
        };
        stream_windows_subscription(&mut send, windows, ProcId::SubscribeWindows.as_u64()).await
    }

    async fn subscribe_window_detail(&mut self, request: SubscribeWindowDetailReq) -> Result<()> {
        let send = self.response_stream();
        let mut send = send.lock().await;
        let Some(windows) = self.windows.clone() else {
            write_reqres_message(
                &mut send,
                stream_service_error_message(
                    ProcId::SubscribeWindowDetail.as_u64(),
                    ServiceError::Unsupported,
                ),
            )
            .await?;
            return Ok(());
        };
        stream_window_detail_subscription(
            &mut send,
            windows,
            ProcId::SubscribeWindowDetail.as_u64(),
            request.window_id,
        )
        .await
    }

    async fn subscribe_jobs(&mut self, request: SubscribeJobsReq) -> Result<()> {
        let commands = self.commands.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_jobs_subscription(&mut send, commands, request).await
    }

    async fn subscribe_job_output(&mut self, request: SubscribeJobOutputReq) -> Result<()> {
        let commands = self.commands.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_job_output_subscription(
            &mut send,
            commands,
            ProcId::SubscribeJobOutput.as_u64(),
            request,
        )
        .await
    }

    async fn subscribe_schedules(&mut self, request: SubscribeSchedulesReq) -> Result<()> {
        let commands = self.commands.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_schedules_subscription(&mut send, commands, request).await
    }

    async fn subscribe_agent_providers(&mut self, _: ()) -> Result<()> {
        let config_path = self.config_path.clone();
        let config_state = self.config_state.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_agent_providers_subscription(&mut send, config_path, config_state).await
    }

    async fn subscribe_agent_projects(&mut self, _: ()) -> Result<()> {
        let agents = self.agents.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_agent_projects_subscription(&mut send, agents).await
    }

    async fn subscribe_agent_session(&mut self, request: SubscribeAgentSessionReq) -> Result<()> {
        let agents = self.agents.clone();
        let send = self.response_stream();
        let mut send = send.lock().await;
        stream_agent_session_subscription(&mut send, agents, request).await
    }

    fn subscribe_roots_rpc<'a>(&'a mut self, request: ()) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_roots(request))
    }

    fn subscribe_directory_rpc<'a>(
        &'a mut self,
        request: SubscribeDirectoryReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_directory(request))
    }

    fn read_file_rpc<'a>(&'a mut self, request: ReadFileReq) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.read_file(request))
    }

    fn subscribe_terminal_sessions_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_terminal_sessions(request))
    }

    fn subscribe_available_shells_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_available_shells(request))
    }

    fn attach_terminal_session_rpc<'a>(
        &'a mut self,
        request: AttachTerminalSessionReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.attach_terminal_session(request))
    }

    fn subscribe_clients_rpc<'a>(&'a mut self, request: ()) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_clients(request))
    }

    fn subscribe_trash_items_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_trash_items(request))
    }

    fn subscribe_processes_rpc<'a>(&'a mut self, request: ()) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_processes(request))
    }

    fn subscribe_process_detail_rpc<'a>(
        &'a mut self,
        request: SubscribeProcessDetailReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_process_detail(request))
    }

    fn subscribe_process_resources_in_use_rpc<'a>(
        &'a mut self,
        request: SubscribeProcessResourcesInUseReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_process_resources_in_use(request))
    }

    fn subscribe_process_sockets_in_use_rpc<'a>(
        &'a mut self,
        request: SubscribeProcessSocketsInUseReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_process_sockets_in_use(request))
    }

    fn subscribe_process_modules_rpc<'a>(
        &'a mut self,
        request: SubscribeProcessModulesReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_process_modules(request))
    }

    fn subscribe_windows_rpc<'a>(&'a mut self, request: ()) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_windows(request))
    }

    fn subscribe_window_detail_rpc<'a>(
        &'a mut self,
        request: SubscribeWindowDetailReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_window_detail(request))
    }

    fn subscribe_jobs_rpc<'a>(
        &'a mut self,
        request: SubscribeJobsReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_jobs(request))
    }

    fn subscribe_job_output_rpc<'a>(
        &'a mut self,
        request: SubscribeJobOutputReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_job_output(request))
    }

    fn subscribe_schedules_rpc<'a>(
        &'a mut self,
        request: SubscribeSchedulesReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_schedules(request))
    }

    fn subscribe_agent_providers_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_agent_providers(request))
    }

    fn subscribe_agent_projects_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_agent_projects(request))
    }

    fn subscribe_agent_session_rpc<'a>(
        &'a mut self,
        request: SubscribeAgentSessionReq,
    ) -> RpcHandlerFuture<'a, Result<()>> {
        Box::pin(self.subscribe_agent_session(request))
    }
}

impl HostRpcHandler {
    async fn write_file(&mut self, request: GeneratedWriteFileReq) -> Result<Vec<ReqResMessage>> {
        let request = WriteFileReq::from_generated(request)?;
        handle_write_file_stream(ProcId::WriteFile.as_u64(), request, self).await
    }

    async fn write_terminal_input(
        &mut self,
        request: GeneratedWriteTerminalInputReq,
    ) -> Result<Vec<ReqResMessage>> {
        let request = WriteTerminalInputReq::from_generated(request)?;
        handle_write_terminal_input_stream(ProcId::WriteTerminalInput.as_u64(), request, self).await
    }

    fn write_file_rpc<'a>(
        &'a mut self,
        request: GeneratedWriteFileReq,
    ) -> RpcHandlerFuture<'a, Result<Vec<ReqResMessage>>> {
        Box::pin(self.write_file(request))
    }

    fn write_terminal_input_rpc<'a>(
        &'a mut self,
        request: GeneratedWriteTerminalInputReq,
    ) -> RpcHandlerFuture<'a, Result<Vec<ReqResMessage>>> {
        Box::pin(self.write_terminal_input(request))
    }
}

async fn handle_write_file_stream(
    proc_id: u64,
    request: WriteFileReq,
    handler: &mut HostRpcHandler,
) -> Result<Vec<ReqResMessage>> {
    let start = match request {
        WriteFileReq::Start(start) => start,
        WriteFileReq::Chunk(_) => {
            return Ok(vec![generic_error_message(
                proc_id,
                RpcErrorCode::MalformedPayload,
                "WriteFile first payload must be WriteFileStart",
            )]);
        }
    };

    let files = handler.files.clone();
    let malformed_chunk = Arc::new(AtomicBool::new(false));
    let chunks = HostWriteFileChunkSource {
        handler,
        malformed_chunk: malformed_chunk.clone(),
    };
    let result = match files.write_file(start, Box::new(chunks)).await {
        Ok(result) => result,
        Err(_) if malformed_chunk.load(Ordering::Relaxed) => {
            return Ok(vec![generic_error_message(
                proc_id,
                RpcErrorCode::MalformedPayload,
                "WriteFile chunk payload must be WriteFileChunk",
            )]);
        }
        Err(err) => return Ok(vec![service_error_message(proc_id, err)]),
    };
    Ok(vec![ok_payload_message(proc_id, result.encode())])
}

struct HostWriteFileChunkSource<'a> {
    handler: &'a mut HostRpcHandler,
    malformed_chunk: Arc<AtomicBool>,
}

impl WriteFileChunkSource for HostWriteFileChunkSource<'_> {
    fn next_chunk(&mut self) -> BoxFutureResult<'_, Option<WriteFileChunk>> {
        Box::pin(async move {
            let Some(payload) = self
                .handler
                .next_request_stream_chunk()
                .await
                .map_err(|err| ServiceError::OperationFailed(err.to_string()))?
            else {
                return Ok(None);
            };
            match WriteFileReq::decode(&payload) {
                Ok(WriteFileReq::Chunk(chunk)) => Ok(Some(chunk)),
                Ok(WriteFileReq::Start(_)) | Err(_) => {
                    self.malformed_chunk.store(true, Ordering::Relaxed);
                    Err(ServiceError::InvalidPath)
                }
            }
        })
    }
}

async fn handle_write_terminal_input_stream(
    proc_id: u64,
    request: WriteTerminalInputReq,
    handler: &mut HostRpcHandler,
) -> Result<Vec<ReqResMessage>> {
    let (terminal_session_id, attach_id) = match request {
        WriteTerminalInputReq::Start {
            terminal_session_id,
            attach_id,
        } => (terminal_session_id, attach_id),
        WriteTerminalInputReq::Chunk { .. } => {
            return Ok(vec![generic_error_message(
                proc_id,
                RpcErrorCode::MalformedPayload,
                "WriteTerminalInput first payload must be WriteTerminalInputStart",
            )]);
        }
    };
    let rpc_session_id = rpc_session_id(&handler.session_state).await;

    while let Some(payload) = handler.next_request_stream_chunk().await? {
        let bytes = match WriteTerminalInputReq::decode(&payload) {
            Ok(WriteTerminalInputReq::Chunk { bytes }) => bytes,
            Ok(WriteTerminalInputReq::Start { .. }) | Err(_) => {
                return Ok(vec![generic_error_message(
                    proc_id,
                    RpcErrorCode::MalformedPayload,
                    "WriteTerminalInput chunk payload must be WriteTerminalInputChunk",
                )]);
            }
        };
        if let Err(err) =
            handler
                .terminals
                .write_input(&terminal_session_id, &attach_id, &bytes, rpc_session_id)
        {
            return Ok(vec![terminal_service_error_message(proc_id, err)]);
        }
    }

    Ok(vec![ok_void_message(proc_id)])
}

async fn stream_read_file(
    send: &mut web_transport_quinn::SendStream,
    files: SharedFileService,
    proc_id: u64,
    request: ReadFileReq,
) -> Result<()> {
    let start_offset = request.offset.unwrap_or(0);
    let bytes = match files.read_file(request).await {
        Ok(bytes) => bytes,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    for (index, bytes) in bytes.chunks(READ_FILE_CHUNK_SIZE).enumerate() {
        let offset = start_offset + (index * READ_FILE_CHUNK_SIZE) as u64;
        let chunk = ReadFileChunk {
            offset,
            bytes: bytes.to_vec(),
        }
        .encode();
        let message = if index == 0 {
            stream_start_payload_message(chunk)
        } else {
            stream_chunk_payload_message(chunk)
        };
        write_reqres_message(send, message).await?;
    }
    Ok(())
}

async fn stream_terminal_sessions_subscription(
    send: &mut web_transport_quinn::SendStream,
    terminals: SharedTerminalManager,
    _proc_id: u64,
) -> Result<()> {
    write_reqres_message(
        send,
        stream_start_payload_message(
            TerminalSessionsTableEvent::Snapshot {
                rows: terminals.sessions_snapshot(),
            }
            .encode(),
        ),
    )
    .await?;

    let mut receiver = terminals.subscribe_sessions();
    loop {
        match receiver.recv().await {
            Ok(event) => {
                write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(
                        TerminalSessionsTableEvent::Snapshot {
                            rows: terminals.sessions_snapshot(),
                        }
                        .encode(),
                    ),
                )
                .await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

async fn stream_clients_subscription(
    send: &mut web_transport_quinn::SendStream,
    client_credentials_events: SharedClientCredentialsEvents,
) -> Result<()> {
    let mut receiver = client_credentials_events.subscribe();
    let mut rows = client_infos_from_credentials(&receiver.borrow().clone());
    write_reqres_message(
        send,
        stream_start_payload_message(ClientsTableEvent::Snapshot { rows: rows.clone() }.encode()),
    )
    .await?;

    loop {
        if receiver.changed().await.is_err() {
            return Ok(());
        }
        let next_rows = client_infos_from_credentials(&receiver.borrow().clone());
        if let Some(event) = clients_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_jobs_subscription(
    send: &mut web_transport_quinn::SendStream,
    commands: SharedCommandManager,
    request: SubscribeJobsReq,
) -> Result<()> {
    let mut receiver = commands.subscribe_jobs_events();
    let snapshot = commands.jobs_snapshot(&request).await;
    write_reqres_message(
        send,
        stream_start_payload_message(
            JobsTableEvent::Snapshot {
                rows: snapshot.rows,
            }
            .encode(),
        ),
    )
    .await?;

    loop {
        if receiver.changed().await.is_err() {
            return Ok(());
        }
        let snapshot = commands.jobs_snapshot(&request).await;
        write_reqres_message(
            send,
            stream_chunk_payload_message(
                JobsTableEvent::Snapshot {
                    rows: snapshot.rows,
                }
                .encode(),
            ),
        )
        .await?;
    }
}

async fn stream_job_output_subscription(
    send: &mut web_transport_quinn::SendStream,
    commands: SharedCommandManager,
    proc_id: u64,
    request: SubscribeJobOutputReq,
) -> Result<()> {
    let mut receiver = commands.subscribe_output_events();
    let mut last_seq = request.after_seq.unwrap_or(0);
    let (_job, _state, events) = match commands.output_attached(&request).await {
        Ok(attached) => attached,
        Err(err) => {
            write_reqres_message(send, stream_command_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };

    let mut events = events.into_iter();
    let Some(first) = events.next() else {
        return Ok(());
    };
    if let JobOutputEvent::Attached { latest_seq, .. } = &first {
        last_seq = last_seq.max(*latest_seq);
    }
    write_reqres_message(send, stream_start_payload_message(first.encode())).await?;
    let mut exited = false;
    for event in events {
        update_job_output_cursor(&mut last_seq, &mut exited, &event);
        write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
    }
    if exited {
        return Ok(());
    }

    loop {
        if receiver.changed().await.is_err() {
            return Ok(());
        }
        let (_job, events) = match commands
            .output_chunks_after(&request.job_id, request.stream, last_seq)
            .await
        {
            Ok(events) => events,
            Err(err) => {
                write_reqres_message(send, stream_command_error_message(proc_id, err)).await?;
                return Ok(());
            }
        };
        for event in events {
            update_job_output_cursor(&mut last_seq, &mut exited, &event);
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            if exited {
                return Ok(());
            }
        }
    }
}

fn update_job_output_cursor(last_seq: &mut u64, exited: &mut bool, event: &JobOutputEvent) {
    match event {
        JobOutputEvent::Attached { latest_seq, .. } => {
            *last_seq = (*last_seq).max(*latest_seq);
        }
        JobOutputEvent::Chunk { seq, .. } => {
            *last_seq = (*last_seq).max(*seq);
        }
        JobOutputEvent::JobExited { .. } => {
            *exited = true;
        }
        JobOutputEvent::HistoryGap { next_seq } => {
            *last_seq = (*last_seq).max(*next_seq);
        }
        JobOutputEvent::Truncated => {}
    }
}

async fn stream_schedules_subscription(
    send: &mut web_transport_quinn::SendStream,
    commands: SharedCommandManager,
    request: SubscribeSchedulesReq,
) -> Result<()> {
    let mut receiver = commands.subscribe_schedules_events();
    let snapshot = commands.schedules_snapshot(&request).await;
    write_reqres_message(
        send,
        stream_start_payload_message(
            SchedulesTableEvent::Snapshot {
                rows: snapshot.rows,
            }
            .encode(),
        ),
    )
    .await?;

    loop {
        if receiver.changed().await.is_err() {
            return Ok(());
        }
        let snapshot = commands.schedules_snapshot(&request).await;
        write_reqres_message(
            send,
            stream_chunk_payload_message(
                SchedulesTableEvent::Snapshot {
                    rows: snapshot.rows,
                }
                .encode(),
            ),
        )
        .await?;
    }
}

async fn stream_agent_providers_subscription(
    send: &mut web_transport_quinn::SendStream,
    config_path: PathBuf,
    config_state: SharedSystemConfig,
) -> Result<()> {
    let mut rows = provider_rows(&config_state.lock().await.clone());
    write_reqres_message(
        send,
        stream_start_payload_message(
            AgentProvidersTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let config = match load_or_default(&config_path) {
            Ok(config) => config,
            Err(error) => {
                warn!(?error, config = %config_path.display(), "failed to reload agent providers");
                continue;
            }
        };
        let next_rows = provider_rows(&config);
        if let Some(event) = providers_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_agent_projects_subscription(
    send: &mut web_transport_quinn::SendStream,
    agents: SharedAgentManager,
) -> Result<()> {
    let proc_id = ProcId::SubscribeAgentProjects.as_u64();
    let mut receiver = agents.subscribe_project_events();
    let mut rows = match agents.projects_snapshot() {
        Ok(rows) => rows,
        Err(error) => {
            write_reqres_message(send, stream_agent_error_message(proc_id, error)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            AgentProjectsTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    loop {
        if receiver.changed().await.is_err() {
            return Ok(());
        }
        let next_rows = match agents.projects_snapshot() {
            Ok(rows) => rows,
            Err(error) => {
                write_reqres_message(send, stream_agent_error_message(proc_id, error)).await?;
                return Ok(());
            }
        };
        if let Some(event) = projects_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_agent_session_subscription(
    send: &mut web_transport_quinn::SendStream,
    agents: SharedAgentManager,
    request: SubscribeAgentSessionReq,
) -> Result<()> {
    let proc_id = ProcId::SubscribeAgentSession.as_u64();
    let subscription = match agents.subscribe_session(&request.session_id) {
        Ok(subscription) => subscription,
        Err(error) => {
            write_reqres_message(send, stream_agent_error_message(proc_id, error)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            AgentSessionEvent::Snapshot {
                snapshot: subscription.snapshot,
            }
            .encode(),
        ),
    )
    .await?;

    let Some(mut events) = subscription.events else {
        std::future::pending::<()>().await;
        unreachable!("a dormant agent session subscription remains open")
    };
    loop {
        match events.recv().await {
            Ok(event) => {
                write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                write_reqres_message(
                    send,
                    stream_agent_error_message(
                        proc_id,
                        AgentError {
                            kind: AgentErrorKind::Failed,
                            message: format!(
                                "agent session subscription fell behind by {skipped} events"
                            ),
                        },
                    ),
                )
                .await?;
                return Ok(());
            }
        }
    }
}

async fn stream_available_shells_subscription(
    send: &mut web_transport_quinn::SendStream,
    terminals: SharedTerminalManager,
    _proc_id: u64,
) -> Result<()> {
    let mut rows = terminals.available_shells_snapshot();
    write_reqres_message(
        send,
        stream_start_payload_message(
            AvailableShellsTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let next_rows = terminals.available_shells_snapshot();
        if next_rows == rows {
            continue;
        }
        rows = next_rows;
        write_reqres_message(
            send,
            stream_chunk_payload_message(
                AvailableShellsTableEvent::Snapshot { rows: rows.clone() }.encode(),
            ),
        )
        .await?;
    }
}

async fn stream_attach_terminal_session(
    send: &mut web_transport_quinn::SendStream,
    terminals: SharedTerminalManager,
    session_state: SharedRpcSessionState,
    proc_id: u64,
    request: AttachTerminalSessionReq,
) -> Result<()> {
    let rpc_session_id = rpc_session_id(&session_state).await;
    let mut attached = match terminals.attach(request, rpc_session_id) {
        Ok(attached) => attached,
        Err(err) => {
            write_reqres_message(send, terminal_stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };

    let terminal_session_id = attached.terminal_session_id.clone();
    let attach_id = attached.attach_id.clone();
    let result = stream_attached_terminal(send, proc_id, &mut attached).await;
    terminals.detach(&terminal_session_id, &attach_id);
    result
}

async fn stream_attached_terminal(
    send: &mut web_transport_quinn::SendStream,
    _proc_id: u64,
    attached: &mut AttachedTerminal,
) -> Result<()> {
    write_reqres_message(
        send,
        stream_start_payload_message(
            TerminalEvent::Attached {
                attach_id: attached.attach_id.clone(),
                primary_attach_id: attached.primary_attach_id.clone(),
                session: attached.session.clone(),
            }
            .encode(),
        ),
    )
    .await?;

    for event in attached.replay.drain(..) {
        write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
    }

    if let Some(exit) = attached.session.exit.clone() {
        write_reqres_message(
            send,
            stream_chunk_payload_message(TerminalEvent::SessionExited { exit }.encode()),
        )
        .await?;
        return Ok(());
    }

    loop {
        match attached.receiver.recv().await {
            Ok(event) => {
                let terminal_done = matches!(
                    event,
                    TerminalEvent::SessionExited { .. } | TerminalEvent::SessionClosed { .. }
                );
                write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
                if terminal_done {
                    return Ok(());
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let next_seq = attached.session.latest_output_seq.saturating_add(1);
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(TerminalEvent::HistoryGap { next_seq }.encode()),
                )
                .await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

async fn stream_processes_subscription(
    send: &mut web_transport_quinn::SendStream,
    _proc_id: u64,
) -> Result<()> {
    let mut system = SysinfoSystem::new();
    let mut rows = process_rows_snapshot(&mut system);
    write_reqres_message(
        send,
        stream_start_payload_message(ProcessesTableEvent::Snapshot { rows: rows.clone() }.encode()),
    )
    .await?;

    let mut interval = tokio::time::interval(PROCESS_DETAIL_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let next_rows = process_rows_snapshot(&mut system);
        if let Some(event) = processes_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_process_detail_subscription(
    send: &mut web_transport_quinn::SendStream,
    proc_id: u64,
    pid: u64,
) -> Result<()> {
    let mut system = SysinfoSystem::new();
    let Some(mut detail) = process_detail_snapshot(&mut system, pid) else {
        write_reqres_message(
            send,
            stream_error_message(proc_id, "not_found", "process not found"),
        )
        .await?;
        return Ok(());
    };

    write_reqres_message(
        send,
        stream_start_payload_message(
            ProcessDetailEvent::Snapshot {
                detail: detail.clone(),
            }
            .encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(PROCESSES_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let Some(next_detail) = process_detail_snapshot(&mut system, pid) else {
            write_reqres_message(
                send,
                stream_chunk_payload_message(ProcessDetailEvent::Exited.encode()),
            )
            .await?;
            return Ok(());
        };
        if next_detail.info != detail.info {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    ProcessDetailEvent::InfoChanged {
                        info: next_detail.info.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.info = next_detail.info.clone();
        }

        if next_detail.metadata != detail.metadata {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    ProcessDetailEvent::MetadataChanged {
                        metadata: next_detail.metadata.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.metadata = next_detail.metadata.clone();
        }

        if next_detail.usage != detail.usage {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    ProcessDetailEvent::UsageChanged {
                        usage: next_detail.usage.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.usage = next_detail.usage;
        }
    }
}

async fn stream_process_resources_in_use_subscription(
    send: &mut web_transport_quinn::SendStream,
    service: SharedProcessResourcesInUseService,
    proc_id: u64,
    pid: u64,
) -> Result<()> {
    let mut rows = match service.resources_in_use(pid).await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            ProcessResourcesInUseTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(PROCESS_RESOURCES_IN_USE_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let next_rows = match service.resources_in_use(pid).await {
            Ok(rows) => rows,
            Err(ServiceError::NotFound) => {
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(ProcessResourcesInUseTableEvent::Exited.encode()),
                )
                .await?;
                return Ok(());
            }
            Err(err) => {
                write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
                return Ok(());
            }
        };
        if let Some(event) = process_resources_in_use_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_process_sockets_in_use_subscription(
    send: &mut web_transport_quinn::SendStream,
    service: SharedProcessSocketsInUseService,
    proc_id: u64,
    pid: u64,
) -> Result<()> {
    let mut rows = match service.sockets_in_use(pid).await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            ProcessSocketsInUseTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(PROCESS_SOCKETS_IN_USE_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let next_rows = match service.sockets_in_use(pid).await {
            Ok(rows) => rows,
            Err(ServiceError::NotFound) => {
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(ProcessSocketsInUseTableEvent::Exited.encode()),
                )
                .await?;
                return Ok(());
            }
            Err(err) => {
                write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
                return Ok(());
            }
        };
        if let Some(event) = process_sockets_in_use_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_process_modules_subscription(
    send: &mut web_transport_quinn::SendStream,
    service: SharedProcessModulesService,
    proc_id: u64,
    pid: u64,
) -> Result<()> {
    let mut rows = match service.modules(pid).await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            ProcessModulesTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(PROCESS_MODULES_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let next_rows = match service.modules(pid).await {
            Ok(rows) => rows,
            Err(ServiceError::NotFound) => {
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(ProcessModulesTableEvent::Exited.encode()),
                )
                .await?;
                return Ok(());
            }
            Err(err) => {
                write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
                return Ok(());
            }
        };
        if let Some(event) = process_modules_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_windows_subscription(
    send: &mut web_transport_quinn::SendStream,
    windows: SharedWindowService,
    proc_id: u64,
) -> Result<()> {
    let details = match windows.windows().await {
        Ok(details) => details,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    let mut rows = window_info_rows(&details);
    write_reqres_message(
        send,
        stream_start_payload_message(WindowsTableEvent::Snapshot { rows: rows.clone() }.encode()),
    )
    .await?;

    let mut interval = tokio::time::interval(WINDOWS_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let next_details = match windows.windows().await {
            Ok(details) => details,
            Err(err) => {
                write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
                return Ok(());
            }
        };
        let next_rows = window_info_rows(&next_details);
        if let Some(event) = windows_patch(&rows, &next_rows) {
            write_reqres_message(send, stream_chunk_payload_message(event.encode())).await?;
            rows = next_rows;
        }
    }
}

async fn stream_window_detail_subscription(
    send: &mut web_transport_quinn::SendStream,
    windows: SharedWindowService,
    proc_id: u64,
    window_id: String,
) -> Result<()> {
    let Some(mut detail) = window_detail_snapshot(&windows, &window_id).await? else {
        write_reqres_message(
            send,
            stream_error_message(proc_id, "not_found", "window not found"),
        )
        .await?;
        return Ok(());
    };

    write_reqres_message(
        send,
        stream_start_payload_message(
            WindowDetailEvent::Snapshot {
                detail: detail.clone(),
            }
            .encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(WINDOWS_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let Some(next_detail) = window_detail_snapshot(&windows, &window_id).await? else {
            write_reqres_message(
                send,
                stream_chunk_payload_message(WindowDetailEvent::Closed.encode()),
            )
            .await?;
            return Ok(());
        };

        if next_detail.info != detail.info {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    WindowDetailEvent::InfoChanged {
                        info: next_detail.info.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.info = next_detail.info.clone();
        }

        if next_detail.state != detail.state {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    WindowDetailEvent::StateChanged {
                        state: next_detail.state.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.state = next_detail.state.clone();
        }

        if next_detail.bounds != detail.bounds {
            write_reqres_message(
                send,
                stream_chunk_payload_message(
                    WindowDetailEvent::BoundsChanged {
                        bounds: next_detail.bounds.clone(),
                    }
                    .encode(),
                ),
            )
            .await?;
            detail.bounds = next_detail.bounds.clone();
        }
    }
}

async fn stream_roots_subscription(
    send: &mut web_transport_quinn::SendStream,
    files: SharedFileService,
    proc_id: u64,
) -> Result<()> {
    let mut rows = match files.roots().await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(RootsTableEvent::Snapshot { rows: rows.clone() }.encode()),
    )
    .await?;

    let mut interval = tokio::time::interval(ROOTS_SUBSCRIPTION_POLL_INTERVAL);
    loop {
        interval.tick().await;
        match files.roots().await {
            Ok(next_rows) => {
                if let Some(event) = roots_patch(&rows, &next_rows) {
                    write_reqres_message(send, stream_chunk_payload_message(event.encode()))
                        .await?;
                    rows = next_rows;
                }
            }
            Err(err) => {
                let reason = roots_close_reason_for_error(&err);
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(RootsTableEvent::Closed { reason }.encode()),
                )
                .await?;
                return Ok(());
            }
        }
    }
}

async fn stream_trash_items_subscription(
    send: &mut web_transport_quinn::SendStream,
    files: SharedFileService,
    trash_events: SharedTrashEvents,
    proc_id: u64,
) -> Result<()> {
    let mut rows = match files.trash_items().await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(
            TrashItemsTableEvent::Snapshot { rows: rows.clone() }.encode(),
        ),
    )
    .await?;

    let mut interval = tokio::time::interval(TRASH_SUBSCRIPTION_POLL_INTERVAL);
    let mut trash_events = trash_events.subscribe();
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            changed = trash_events.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
        }
        match files.trash_items().await {
            Ok(next_rows) => {
                if let Some(event) = trash_items_patch(&rows, &next_rows) {
                    write_reqres_message(send, stream_chunk_payload_message(event.encode()))
                        .await?;
                    rows = next_rows;
                }
            }
            Err(err) => {
                let reason = trash_items_close_reason_for_error(&err);
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(TrashItemsTableEvent::Closed { reason }.encode()),
                )
                .await?;
                return Ok(());
            }
        }
    }
}

async fn stream_directory_subscription(
    send: &mut web_transport_quinn::SendStream,
    files: SharedFileService,
    proc_id: u64,
    path: String,
) -> Result<()> {
    if let Err(err) = files.list_directory(path.clone()).await {
        write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
        return Ok(());
    }

    let (_watcher, mut events) = match create_subscription_watcher(Path::new(&path)) {
        Ok(watcher) => watcher,
        Err(err) => {
            write_reqres_message(
                send,
                stream_error_message(proc_id, "failed", &err.to_string()),
            )
            .await?;
            return Ok(());
        }
    };

    let mut rows = match files.list_directory(path.clone()).await {
        Ok(rows) => rows,
        Err(err) => {
            write_reqres_message(send, stream_service_error_message(proc_id, err)).await?;
            return Ok(());
        }
    };
    write_reqres_message(
        send,
        stream_start_payload_message(DirectoryTableEvent::Snapshot { rows: rows.clone() }.encode()),
    )
    .await?;

    loop {
        match events.recv().await {
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                warn!(?err, path, "filesystem subscription watcher failed");
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(
                        DirectoryTableEvent::Closed {
                            reason: DirectorySubscriptionCloseReason::Failed,
                        }
                        .encode(),
                    ),
                )
                .await?;
                return Ok(());
            }
            None => {
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(
                        DirectoryTableEvent::Closed {
                            reason: DirectorySubscriptionCloseReason::Failed,
                        }
                        .encode(),
                    ),
                )
                .await?;
                return Ok(());
            }
        }

        tokio::time::sleep(SUBSCRIPTION_DEBOUNCE).await;
        while let Ok(event) = events.try_recv() {
            if let Err(err) = event {
                warn!(
                    ?err,
                    path, "filesystem subscription watcher failed during debounce"
                );
            }
        }

        match files.list_directory(path.clone()).await {
            Ok(next_rows) => {
                if let Some(event) = directory_patch(&rows, &next_rows) {
                    write_reqres_message(send, stream_chunk_payload_message(event.encode()))
                        .await?;
                    rows = next_rows;
                }
            }
            Err(err) => {
                let reason = directory_close_reason_for_error(&err);
                write_reqres_message(
                    send,
                    stream_chunk_payload_message(DirectoryTableEvent::Closed { reason }.encode()),
                )
                .await?;
                return Ok(());
            }
        }
    }
}

fn create_subscription_watcher(
    path: &Path,
) -> Result<(
    notify::RecommendedWatcher,
    tokio::sync::mpsc::UnboundedReceiver<notify::Result<Event>>,
)> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })?;
    watcher.watch(path, RecursiveMode::NonRecursive)?;
    Ok((watcher, rx))
}

fn process_rows_snapshot(system: &mut SysinfoSystem) -> Vec<ProcessInfo> {
    system.refresh_processes(ProcessesToUpdate::All, true);
    let mut rows = system
        .processes()
        .values()
        .map(process_info_from_sysinfo)
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.pid);
    rows
}

fn process_detail_snapshot(system: &mut SysinfoSystem, pid: u64) -> Option<ProcessDetail> {
    let pid = u32::try_from(pid).ok().map(SysPid::from_u32)?;
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        process_detail_refresh_kind(),
    );
    system.process(pid).map(process_detail_from_sysinfo)
}

#[derive(Debug, PartialEq, Eq)]
enum KillHostProcessError {
    NotFound,
    PermissionDenied,
}

fn kill_host_process(pid: u64) -> Result<(), KillHostProcessError> {
    let pid = u32::try_from(pid)
        .ok()
        .map(SysPid::from_u32)
        .ok_or(KillHostProcessError::NotFound)?;
    if pid.as_u32() == std::process::id() {
        return Err(KillHostProcessError::PermissionDenied);
    }

    let mut system = SysinfoSystem::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let process = system.process(pid).ok_or(KillHostProcessError::NotFound)?;
    if process.kill() {
        Ok(())
    } else {
        Err(KillHostProcessError::PermissionDenied)
    }
}

fn process_detail_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_disk_usage()
        .with_memory()
        .with_cwd(UpdateKind::Always)
        .with_root(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_exe(UpdateKind::OnlyIfNotSet)
}

fn process_detail_from_sysinfo(process: &SysProcess) -> ProcessDetail {
    ProcessDetail {
        info: process_info_from_sysinfo(process),
        metadata: process_metadata_from_sysinfo(process),
        usage: process_resource_usage_from_sysinfo(process),
    }
}

fn process_metadata_from_sysinfo(process: &SysProcess) -> ProcessMetadata {
    ProcessMetadata {
        command: process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect(),
        executable_path: process
            .exe()
            .map(|path| path.to_string_lossy().into_owned()),
        cwd: process
            .cwd()
            .map(|path| path.to_string_lossy().into_owned()),
        start_time_unix: u53_saturating(process.start_time()),
    }
}

fn process_resource_usage_from_sysinfo(process: &SysProcess) -> ProcessResourceUsage {
    let io_usage = process.disk_usage();
    ProcessResourceUsage {
        memory_bytes: u53_saturating(process.memory()),
        virtual_memory_bytes: u53_saturating(process.virtual_memory()),
        cpu_usage_percent: cpu_usage_percent(process.cpu_usage()),
        accumulated_cpu_time_ms: u53_saturating(process.accumulated_cpu_time()),
        io_usage: ProcessIoUsage {
            read_bytes: u53_saturating(io_usage.read_bytes),
            written_bytes: u53_saturating(io_usage.written_bytes),
            total_read_bytes: u53_saturating(io_usage.total_read_bytes),
            total_written_bytes: u53_saturating(io_usage.total_written_bytes),
        },
    }
}

fn process_info_from_sysinfo(process: &SysProcess) -> ProcessInfo {
    ProcessInfo {
        pid: u64::from(process.pid().as_u32()),
        ppid: process.parent().map(|pid| u64::from(pid.as_u32())),
        name: process.name().to_string_lossy().into_owned(),
        status: process_status_from_sysinfo(process.status()),
    }
}

fn process_status_from_sysinfo(status: SysProcessStatus) -> ProcessStatus {
    match status {
        SysProcessStatus::Idle => ProcessStatus::Idle,
        SysProcessStatus::Run => ProcessStatus::Run,
        SysProcessStatus::Sleep => ProcessStatus::Sleep,
        SysProcessStatus::Stop => ProcessStatus::Stop,
        SysProcessStatus::Zombie => ProcessStatus::Zombie,
        SysProcessStatus::Tracing => ProcessStatus::Tracing,
        SysProcessStatus::Dead => ProcessStatus::Dead,
        SysProcessStatus::Wakekill => ProcessStatus::Wakekill,
        SysProcessStatus::Waking => ProcessStatus::Waking,
        SysProcessStatus::Parked => ProcessStatus::Parked,
        SysProcessStatus::LockBlocked => ProcessStatus::LockBlocked,
        SysProcessStatus::UninterruptibleDiskSleep => ProcessStatus::UninterruptibleDiskSleep,
        SysProcessStatus::Suspended => ProcessStatus::Suspended,
        SysProcessStatus::Unknown(code) => ProcessStatus::Unknown {
            code: u64::from(code),
        },
    }
}

fn window_info_rows(details: &[WindowDetail]) -> Vec<WindowInfo> {
    details.iter().map(|detail| detail.info.clone()).collect()
}

async fn window_detail_snapshot(
    windows: &SharedWindowService,
    window_id: &str,
) -> Result<Option<WindowDetail>, ServiceError> {
    Ok(windows
        .windows()
        .await?
        .into_iter()
        .find(|detail| detail.info.window_id == window_id))
}

fn windows_patch(previous: &[WindowInfo], next: &[WindowInfo]) -> Option<WindowsTableEvent> {
    let previous_by_id: BTreeMap<&str, &WindowInfo> = previous
        .iter()
        .map(|entry| (entry.window_id.as_str(), entry))
        .collect();
    let next_by_id: BTreeMap<&str, &WindowInfo> = next
        .iter()
        .map(|entry| (entry.window_id.as_str(), entry))
        .collect();

    let removes = previous_by_id
        .keys()
        .filter(|id| !next_by_id.contains_key(**id))
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    let upserts = next_by_id
        .iter()
        .filter_map(|(id, entry)| {
            if previous_by_id.get(id).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(WindowsTableEvent::Patch { removes, upserts })
    }
}

fn cpu_usage_percent(cpu_usage: f32) -> f64 {
    if !cpu_usage.is_finite() || cpu_usage <= 0.0 {
        return 0.0;
    }
    f64::from(cpu_usage)
}

fn u53_saturating(value: u64) -> u64 {
    value.min(MAX_U53)
}

fn processes_patch(previous: &[ProcessInfo], next: &[ProcessInfo]) -> Option<ProcessesTableEvent> {
    let previous_by_pid: BTreeMap<u64, &ProcessInfo> =
        previous.iter().map(|entry| (entry.pid, entry)).collect();
    let next_by_pid: BTreeMap<u64, &ProcessInfo> =
        next.iter().map(|entry| (entry.pid, entry)).collect();

    let remove_pids = previous_by_pid
        .keys()
        .filter(|pid| !next_by_pid.contains_key(pid))
        .copied()
        .collect::<Vec<_>>();
    let upserts = next_by_pid
        .iter()
        .filter_map(|(pid, entry)| {
            if previous_by_pid.get(pid).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if remove_pids.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(ProcessesTableEvent::Patch {
            remove_pids,
            upserts,
        })
    }
}

fn process_resources_in_use_patch(
    previous: &[ProcessResourceInUseInfo],
    next: &[ProcessResourceInUseInfo],
) -> Option<ProcessResourcesInUseTableEvent> {
    let previous_by_id: BTreeMap<&str, &ProcessResourceInUseInfo> = previous
        .iter()
        .map(|entry| (entry.resource_id.as_str(), entry))
        .collect();
    let next_by_id: BTreeMap<&str, &ProcessResourceInUseInfo> = next
        .iter()
        .map(|entry| (entry.resource_id.as_str(), entry))
        .collect();

    let removes = previous_by_id
        .keys()
        .filter(|id| !next_by_id.contains_key(**id))
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    let upserts = next_by_id
        .iter()
        .filter_map(|(id, entry)| {
            if previous_by_id.get(id).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(ProcessResourcesInUseTableEvent::Patch { removes, upserts })
    }
}

fn process_sockets_in_use_patch(
    previous: &[ProcessSocketInUseInfo],
    next: &[ProcessSocketInUseInfo],
) -> Option<ProcessSocketsInUseTableEvent> {
    let previous_by_id: BTreeMap<&str, &ProcessSocketInUseInfo> = previous
        .iter()
        .map(|entry| (entry.socket_id.as_str(), entry))
        .collect();
    let next_by_id: BTreeMap<&str, &ProcessSocketInUseInfo> = next
        .iter()
        .map(|entry| (entry.socket_id.as_str(), entry))
        .collect();

    let removes = previous_by_id
        .keys()
        .filter(|id| !next_by_id.contains_key(**id))
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    let upserts = next_by_id
        .iter()
        .filter_map(|(id, entry)| {
            if previous_by_id.get(id).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(ProcessSocketsInUseTableEvent::Patch { removes, upserts })
    }
}

fn process_modules_patch(
    previous: &[ProcessModuleInfo],
    next: &[ProcessModuleInfo],
) -> Option<ProcessModulesTableEvent> {
    let previous_by_id: BTreeMap<&str, &ProcessModuleInfo> = previous
        .iter()
        .map(|entry| (entry.module_id.as_str(), entry))
        .collect();
    let next_by_id: BTreeMap<&str, &ProcessModuleInfo> = next
        .iter()
        .map(|entry| (entry.module_id.as_str(), entry))
        .collect();

    let removes = previous_by_id
        .keys()
        .filter(|id| !next_by_id.contains_key(**id))
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    let upserts = next_by_id
        .iter()
        .filter_map(|(id, entry)| {
            if previous_by_id.get(id).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(ProcessModulesTableEvent::Patch { removes, upserts })
    }
}

fn roots_patch(previous: &[FsEntry], next: &[FsEntry]) -> Option<RootsTableEvent> {
    let previous_by_path: BTreeMap<&str, &FsEntry> = previous
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let next_by_path: BTreeMap<&str, &FsEntry> = next
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();

    let removes = previous_by_path
        .keys()
        .filter(|path| !next_by_path.contains_key(**path))
        .map(|path| RootEntryKey {
            path: (*path).to_string(),
        })
        .collect::<Vec<_>>();
    let upserts = next_by_path
        .iter()
        .filter_map(|(path, entry)| {
            if previous_by_path.get(path).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(RootsTableEvent::Patch { removes, upserts })
    }
}

fn directory_patch(previous: &[FsEntry], next: &[FsEntry]) -> Option<DirectoryTableEvent> {
    let previous_by_name: BTreeMap<&str, &FsEntry> = previous
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect();
    let next_by_name: BTreeMap<&str, &FsEntry> = next
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect();

    let removes = previous_by_name
        .keys()
        .filter(|name| !next_by_name.contains_key(**name))
        .map(|name| DirectoryEntryKey {
            name: (*name).to_string(),
        })
        .collect::<Vec<_>>();
    let upserts = next_by_name
        .iter()
        .filter_map(|(name, entry)| {
            if previous_by_name.get(name).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(DirectoryTableEvent::Patch { removes, upserts })
    }
}

fn trash_items_patch(previous: &[TrashItem], next: &[TrashItem]) -> Option<TrashItemsTableEvent> {
    let previous_by_id: BTreeMap<&str, &TrashItem> = previous
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let next_by_id: BTreeMap<&str, &TrashItem> = next
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();

    let removes = previous_by_id
        .keys()
        .filter(|id| !next_by_id.contains_key(**id))
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    let upserts = next_by_id
        .iter()
        .filter_map(|(id, entry)| {
            if previous_by_id.get(id).copied() == Some(*entry) {
                None
            } else {
                Some((*entry).clone())
            }
        })
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(TrashItemsTableEvent::Patch { removes, upserts })
    }
}

fn roots_close_reason_for_error(err: &ServiceError) -> RootsSubscriptionCloseReason {
    match err {
        ServiceError::PermissionDenied => RootsSubscriptionCloseReason::PermissionLost,
        ServiceError::OperationFailed(_) => RootsSubscriptionCloseReason::Failed,
        _ => RootsSubscriptionCloseReason::Unknown,
    }
}

fn directory_close_reason_for_error(err: &ServiceError) -> DirectorySubscriptionCloseReason {
    match err {
        ServiceError::NotFound => DirectorySubscriptionCloseReason::Deleted,
        ServiceError::NotDirectory | ServiceError::NotFile => {
            DirectorySubscriptionCloseReason::ReplacedByNonDirectory
        }
        ServiceError::PermissionDenied => DirectorySubscriptionCloseReason::PermissionLost,
        ServiceError::OperationFailed(_) => DirectorySubscriptionCloseReason::Failed,
        _ => DirectorySubscriptionCloseReason::Unknown,
    }
}

fn trash_items_close_reason_for_error(err: &ServiceError) -> TrashItemsSubscriptionCloseReason {
    match err {
        ServiceError::PermissionDenied => TrashItemsSubscriptionCloseReason::PermissionLost,
        ServiceError::OperationFailed(_) | ServiceError::Unsupported => {
            TrashItemsSubscriptionCloseReason::Failed
        }
        _ => TrashItemsSubscriptionCloseReason::Unknown,
    }
}

async fn write_reqres_messages(
    send: &mut web_transport_quinn::SendStream,
    messages: &[ReqResMessage],
) -> Result<()> {
    let encoded = ReqResMessage::encode_sequence(messages);
    send.write_all(&encoded).await?;
    Ok(())
}

async fn write_reqres_message(
    send: &mut web_transport_quinn::SendStream,
    message: ReqResMessage,
) -> Result<()> {
    write_reqres_messages(send, &[message]).await
}

async fn handle_session_control_messages(
    mut messages: Vec<ReqResMessage>,
    client_credentials: SharedClientCredentials,
    session_state: SharedRpcSessionState,
) -> Result<Vec<ReqResMessage>> {
    if messages.len() != 1 {
        return Ok(vec![session_auth_error_message(
            SessionAuthErrorCode::MalformedPayload,
            "session authentication expects exactly one control message",
        )]);
    }

    match messages.remove(0) {
        ReqResMessage::SessionAuthenticate { mechanism, payload } => Ok(vec![
            authenticate_session_control(client_credentials, session_state, mechanism, payload)
                .await?,
        ]),
        _ => Ok(vec![session_auth_error_message(
            SessionAuthErrorCode::MalformedPayload,
            "client must send SessionAuthenticate on a session-control stream",
        )]),
    }
}

async fn authenticate_session_control(
    client_credentials: SharedClientCredentials,
    session_state: SharedRpcSessionState,
    mechanism: String,
    payload: Vec<u8>,
) -> Result<ReqResMessage> {
    if is_authenticated(&session_state).await {
        return Ok(session_auth_error_message(
            SessionAuthErrorCode::AlreadyAuthenticated,
            "session is already authenticated",
        ));
    }
    if mechanism != PAIRED_SECRET_AUTH_MECHANISM {
        return Ok(session_auth_error_message(
            SessionAuthErrorCode::UnsupportedMechanism,
            "unsupported session authentication mechanism",
        ));
    }
    let credential = match PairedSecretCredential::decode(&payload) {
        Ok(credential) => credential,
        Err(_) => {
            return Ok(session_auth_error_message(
                SessionAuthErrorCode::MalformedPayload,
                "session authentication payload is malformed",
            ));
        }
    };
    if !verify_session_credentials(&credential, &client_credentials).await {
        return Ok(session_auth_error_message(
            SessionAuthErrorCode::InvalidCredentials,
            "paired credential verification failed",
        ));
    }
    session_state.lock().await.authenticated_client_id = Some(credential.credential_id);
    Ok(ReqResMessage::SessionAuthenticated)
}

#[cfg(test)]
async fn handle_rpc_messages(
    messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    terminals: SharedTerminalManager,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    let client_credentials_events = temporary_client_credentials_events(&client_credentials).await;
    let (trash_events, _) = watch::channel(0);
    let commands = test_commands();
    handle_rpc_messages_with_events(
        messages,
        config_path,
        credentials_path,
        config_state,
        client_credentials,
        client_credentials_events,
        pairing_challenge,
        session_state,
        files,
        terminals,
        commands,
        trash_events,
        pairing_notifier,
    )
    .await
}

#[cfg(test)]
async fn handle_rpc_messages_with_events(
    messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    client_credentials_events: SharedClientCredentialsEvents,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    terminals: SharedTerminalManager,
    commands: SharedCommandManager,
    trash_events: SharedTrashEvents,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    let rpc_handlers = Arc::new(build_rpc_handlers(None, None, None, None));
    let context = HostRpcContext {
        session: None,
        config_path: config_path.to_path_buf(),
        credentials_path: credentials_path.to_path_buf(),
        config_state,
        client_credentials,
        client_credentials_events,
        pairing_challenge,
        session_state,
        files,
        windows: None,
        process_resources_in_use: None,
        process_sockets_in_use: None,
        process_modules: None,
        rpc_handlers,
        terminals,
        commands,
        agents: test_agents(),
        trash_events,
        pairing_notifier,
    };
    dispatch_buffered_reqres_invocation(messages, context).await
}

async fn dispatch_unary_rpc(
    proc_id: u64,
    payload: Option<Vec<u8>>,
    context: HostRpcContext,
) -> Result<Vec<ReqResMessage>> {
    if requires_authentication(proc_id) && !is_authenticated(&context.session_state).await {
        return Ok(vec![unauthorized_message(proc_id)]);
    }
    let request = match RpcRequest::decode(proc_id, payload.as_deref()) {
        Ok(request) => request,
        Err(err) => return Ok(vec![rpc_request_decode_error_message(proc_id, err)]),
    };
    let request_proc_id = request.proc_id().as_u64();
    let rpc_handlers = context.rpc_handlers.clone();
    let mut handler = HostRpcHandler::new(context, None, None);
    let Some(outcome) = rpc_handlers.dispatch_unary(&mut handler, request).await else {
        return Ok(vec![error_message(
            request_proc_id,
            "not_implemented",
            "this RPC is reserved but not implemented in the first cut",
        )]);
    };
    Ok(vec![outcome?.into_message()])
}

enum UnaryRpcOutcome {
    Response(RpcResponse),
    Message(ReqResMessage),
}

impl UnaryRpcOutcome {
    fn into_message(self) -> ReqResMessage {
        match self {
            Self::Response(response) => {
                let payload = response.encode_payload();
                match payload {
                    Some(payload) => ok_payload_message(response.proc_id().as_u64(), payload),
                    None => ok_void_message(response.proc_id().as_u64()),
                }
            }
            Self::Message(message) => message,
        }
    }
}

impl From<RpcResponse> for UnaryRpcOutcome {
    fn from(value: RpcResponse) -> Self {
        Self::Response(value)
    }
}

impl HostRpcHandler {
    async fn get_daemon_info(&mut self, _: ()) -> Result<UnaryRpcOutcome> {
        Ok(
            RpcResponse::GetDaemonInfo(DaemonInfo::current_with_supported_procs(
                self.rpc_handlers.supported_procs(),
            ))
            .into(),
        )
    }

    async fn get_daemon_environment(&mut self, _: ()) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::GetDaemonEnvironment(DaemonEnvironment::current()).into())
    }

    async fn start_pairing(&mut self, request: StartPairingRequest) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::StartPairing.as_u64();
        let Some(confirmation_code) = normalize_confirmation_code(&request.confirmation_code)
        else {
            return Ok(UnaryRpcOutcome::Message(generic_error_message(
                proc_id,
                RpcErrorCode::MalformedPayload,
                "StartPairing confirmationCode must be two ASCII digits",
            )));
        };
        let client_label = pairing_client_label(&request.client_label);
        let requested_client_id = normalize_pairing_client_id(request.client_id.as_deref());
        let Some(notifier) = self.pairing_notifier.as_ref() else {
            return Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "failed",
                "daemon failed to start pairing",
            )));
        };
        let current_session_id = rpc_session_id(&self.session_state).await;
        let attempt_key = pairing_attempt_key(requested_client_id.as_deref());
        let attempt_id = begin_pairing_attempt(&self.pairing_challenge, attempt_key.clone()).await;
        let config = load_runtime_config(&self.config_path, &self.config_state).await?;
        let daemon_url = pairing_daemon_url(&config);
        if let Err(err) = notifier
            .confirm_pairing_request(PairingConfirmationRequest {
                daemon_url: daemon_url.clone(),
                confirmation_code,
                client_label: client_label.clone(),
            })
            .await
        {
            warn!(?err, "local pairing confirmation was rejected");
            let mut state = self.pairing_challenge.lock().await;
            if state
                .current_attempts
                .get(&attempt_key)
                .is_some_and(|current| *current == attempt_id)
            {
                state.current_attempts.remove(&attempt_key);
            }
            return Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "failed",
                "daemon failed to confirm pairing",
            )));
        }
        if !is_current_pairing_attempt(&self.pairing_challenge, &attempt_key, attempt_id).await {
            return Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "failed",
                "pairing request was superseded",
            )));
        }

        let now = now_unix();
        let pairing = create_pairing_code(now);
        let pairing_code_expires_at_unix = pairing.record.expires_at_unix;
        {
            let mut state = self.pairing_challenge.lock().await;
            if !state
                .current_attempts
                .get(&attempt_key)
                .is_some_and(|current| *current == attempt_id)
            {
                return Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "failed",
                    "pairing request was superseded",
                )));
            }
            state.active_challenge = Some(ActivePairingChallenge {
                attempt_id,
                attempt_key: attempt_key.clone(),
                owner_session_id: current_session_id,
                record: pairing.record,
                client_label,
                client_id: requested_client_id,
            });
        }

        let notification = PairingCodeNotification {
            daemon_url,
            pairing_code: pairing.code,
            expires_in_seconds: pairing_code_expires_at_unix - now,
        };
        if let Err(err) = notifier.notify_pairing_code(notification).await {
            warn!(?err, "failed to notify local pairing UI");
            let mut state = self.pairing_challenge.lock().await;
            if state.active_challenge.as_ref().is_some_and(|challenge| {
                challenge.attempt_key == attempt_key && challenge.attempt_id == attempt_id
            }) {
                state.active_challenge = None;
                state.current_attempts.remove(&attempt_key);
            }
            return Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "failed",
                "daemon failed to start pairing",
            )));
        }

        Ok(RpcResponse::StartPairing(StartPairingResponse {
            pairing_code_expires_at_unix,
        })
        .into())
    }

    async fn complete_pairing(
        &mut self,
        request: CompletePairingRequest,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CompletePairing.as_u64();
        let now = now_unix();
        let current_session_id = rpc_session_id(&self.session_state).await;
        let pairing = {
            let mut state = self.pairing_challenge.lock().await;
            let Some(pairing) = state.active_challenge.as_ref() else {
                return Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "pairing_not_started",
                    "create a local pairing code before completing pairing",
                )));
            };
            if pairing.owner_session_id != current_session_id {
                return Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "pairing_not_started",
                    "start pairing on this session before completing pairing",
                )));
            }
            if now >= pairing.record.expires_at_unix {
                let attempt_key = pairing.attempt_key.clone();
                state.active_challenge = None;
                state.current_attempts.remove(&attempt_key);
                return Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "pairing_expired",
                    "pairing code expired",
                )));
            }
            if !verify_pairing_code(&pairing.record, request.code.trim(), now) {
                return Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "invalid_pairing_code",
                    "pairing code is invalid",
                )));
            }
            let pairing = state
                .active_challenge
                .take()
                .expect("active pairing challenge exists after validation");
            state.current_attempts.remove(&pairing.attempt_key);
            pairing
        };

        let mut state =
            load_runtime_client_credentials(&self.credentials_path, &self.client_credentials)
                .await?;
        let existing_record = pairing.client_id.as_deref().and_then(|client_id| {
            state
                .clients
                .iter()
                .find(|record| record.client_id == client_id)
                .cloned()
        });
        let issued = match existing_record {
            Some(record) => reissue_client_secret(&record, &pairing.client_label, now),
            None => issue_client_secret(&pairing.client_label, now),
        };
        let client_id = issued.client_id.clone();
        let client_credential_expires_at_unix = issued.record.expires_at_unix;
        state.clients.retain(|record| record.client_id != client_id);
        state.clients.push(issued.record);
        store_runtime_client_credentials(
            &self.credentials_path,
            &self.client_credentials,
            &self.client_credentials_events,
            state,
        )
        .await?;

        if let Some(notifier) = self.pairing_notifier.as_ref() {
            if let Err(err) = notifier.notify_pairing_completed().await {
                warn!(
                    ?err,
                    "failed to notify local pairing UI that pairing completed"
                );
            }
        }

        Ok(RpcResponse::CompletePairing(CompletePairingResponse {
            client_id,
            client_secret: issued.client_secret,
            client_credential_expires_at_unix,
        })
        .into())
    }

    async fn renew_client_credential(&mut self, _: ()) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::RenewClientCredential.as_u64();
        let Some(client_id) = authenticated_client_id(&self.session_state).await else {
            return Ok(UnaryRpcOutcome::Message(unauthorized_message(proc_id)));
        };
        let now = now_unix();
        let mut state =
            load_runtime_client_credentials(&self.credentials_path, &self.client_credentials)
                .await?;
        let Some(record) = state
            .clients
            .iter_mut()
            .find(|record| record.client_id == client_id)
        else {
            return Ok(UnaryRpcOutcome::Message(unauthorized_message(proc_id)));
        };
        renew_client_credential(record, now);
        let client_credential_expires_at_unix = record.expires_at_unix;
        store_runtime_client_credentials(
            &self.credentials_path,
            &self.client_credentials,
            &self.client_credentials_events,
            state,
        )
        .await?;
        Ok(
            RpcResponse::RenewClientCredential(RenewClientCredentialResponse {
                client_credential_expires_at_unix,
            })
            .into(),
        )
    }

    async fn remove_client(&mut self, request: RemoveClientReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::RemoveClient.as_u64();
        let mut state =
            load_runtime_client_credentials(&self.credentials_path, &self.client_credentials)
                .await?;
        let previous_len = state.clients.len();
        state
            .clients
            .retain(|record| record.client_id != request.client_id);
        if state.clients.len() == previous_len {
            return Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "not_found",
                "paired client not found",
            )));
        }
        store_runtime_client_credentials(
            &self.credentials_path,
            &self.client_credentials,
            &self.client_credentials_events,
            state,
        )
        .await?;
        Ok(RpcResponse::RemoveClient.into())
    }

    async fn kill_process(&mut self, request: KillProcessReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::KillProcess.as_u64();
        match kill_host_process(request.pid) {
            Ok(()) => Ok(RpcResponse::KillProcess.into()),
            Err(KillHostProcessError::NotFound) => Ok(UnaryRpcOutcome::Message(error_message(
                proc_id,
                "not_found",
                "process not found",
            ))),
            Err(KillHostProcessError::PermissionDenied) => {
                Ok(UnaryRpcOutcome::Message(error_message(
                    proc_id,
                    "permission_denied",
                    "process could not be terminated",
                )))
            }
        }
    }

    async fn create_nodes(&mut self, request: CreateNodesReq) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::CreateNodes(create_nodes(self.files.as_ref(), request).await).into())
    }

    async fn rename_paths(&mut self, request: RenamePathsReq) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::RenamePaths(rename_paths(self.files.as_ref(), request).await).into())
    }

    async fn delete_paths(&mut self, request: DeletePathsReq) -> Result<UnaryRpcOutcome> {
        let mode = request.mode;
        let response = delete_paths(self.files.as_ref(), request).await;
        if mode == DeleteMode::Trash && bulk_mutation_has_ok(&response) {
            notify_trash_changed(&self.trash_events);
        }
        Ok(RpcResponse::DeletePaths(response).into())
    }

    async fn restore_trash_items(
        &mut self,
        request: RestoreTrashItemsReq,
    ) -> Result<UnaryRpcOutcome> {
        let response = restore_trash_items(self.files.as_ref(), request).await;
        if bulk_mutation_has_ok(&response) {
            notify_trash_changed(&self.trash_events);
        }
        Ok(RpcResponse::RestoreTrashItems(response).into())
    }

    async fn purge_trash_items(&mut self, request: PurgeTrashItemsReq) -> Result<UnaryRpcOutcome> {
        let response = purge_trash_items(self.files.as_ref(), request).await;
        if bulk_mutation_has_ok(&response) {
            notify_trash_changed(&self.trash_events);
        }
        Ok(RpcResponse::PurgeTrashItems(response).into())
    }

    async fn create_terminal_session(
        &mut self,
        request: CreateTerminalSessionReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateTerminalSession.as_u64();
        let Some(client_id) = authenticated_client_id(&self.session_state).await else {
            return Ok(UnaryRpcOutcome::Message(unauthorized_message(proc_id)));
        };
        match self.terminals.create_session(request, client_id) {
            Ok(session) => Ok(RpcResponse::CreateTerminalSession(session).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(terminal_service_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn take_terminal_control(
        &mut self,
        request: TakeTerminalControlReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::TakeTerminalControl.as_u64();
        match self
            .terminals
            .take_control(request, rpc_session_id(&self.session_state).await)
        {
            Ok(response) => Ok(RpcResponse::TakeTerminalControl(response).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(terminal_service_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn close_terminal_session(
        &mut self,
        request: CloseTerminalSessionReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CloseTerminalSession.as_u64();
        match self.terminals.close_session(&request.terminal_session_id) {
            Ok(()) => Ok(RpcResponse::CloseTerminalSession.into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(terminal_service_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn run_command(&mut self, request: RunCommandReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::RunCommand.as_u64();
        let result = if let Some(session) = self.session.clone() {
            self.commands
                .run_command_until(request, async move {
                    let _ = session.closed().await;
                })
                .await
        } else {
            self.commands.run_command(request).await
        };
        match result {
            Ok(response) => Ok(RpcResponse::RunCommand(response).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn create_job(&mut self, request: CreateJobReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateJob.as_u64();
        match self.commands.create_job(request).await {
            Ok(job) => Ok(RpcResponse::CreateJob(job).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn kill_job(&mut self, request: KillJobReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::KillJob.as_u64();
        match self.commands.kill_job(request).await {
            Ok(()) => Ok(RpcResponse::KillJob.into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn delete_jobs(&mut self, request: DeleteJobsReq) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::DeleteJobs(self.commands.delete_jobs(request).await).into())
    }

    async fn clear_jobs(&mut self, request: ClearJobsReq) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::ClearJobs(self.commands.clear_jobs(request).await).into())
    }

    async fn create_schedule(&mut self, request: CreateScheduleReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateSchedule.as_u64();
        match self.commands.create_schedule(request).await {
            Ok(schedule) => Ok(RpcResponse::CreateSchedule(schedule).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn update_schedule(&mut self, request: UpdateScheduleReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::UpdateSchedule.as_u64();
        match self.commands.update_schedule(request).await {
            Ok(schedule) => Ok(RpcResponse::UpdateSchedule(schedule).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn delete_schedules(&mut self, request: DeleteSchedulesReq) -> Result<UnaryRpcOutcome> {
        Ok(RpcResponse::DeleteSchedules(self.commands.delete_schedules(request).await).into())
    }

    async fn get_schedule_next_runs(
        &mut self,
        request: GetScheduleNextRunsReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::GetScheduleNextRuns.as_u64();
        match self.commands.get_schedule_next_runs(request).await {
            Ok(response) => Ok(RpcResponse::GetScheduleNextRuns(response).into()),
            Err(err) => Ok(UnaryRpcOutcome::Message(command_error_message(
                proc_id, err,
            ))),
        }
    }

    async fn create_agent_project(
        &mut self,
        request: CreateAgentProjectReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateAgentProject.as_u64();
        match self.agents.create_project(request) {
            Ok(project) => Ok(RpcResponse::CreateAgentProject(project).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn list_agent_sessions(
        &mut self,
        request: ListAgentSessionsReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::ListAgentSessions.as_u64();
        match self.agents.list_sessions(request) {
            Ok(response) => Ok(RpcResponse::ListAgentSessions(response).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn create_agent_session(
        &mut self,
        request: CreateAgentSessionReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateAgentSession.as_u64();
        let config = match load_runtime_config(&self.config_path, &self.config_state).await {
            Ok(config) => config,
            Err(error) => {
                return Ok(UnaryRpcOutcome::Message(agent_error_message(
                    proc_id,
                    AgentError {
                        kind: AgentErrorKind::Failed,
                        message: format!("load agent configuration: {error:#}"),
                    },
                )));
            }
        };
        let Some(provider) = config.agent_servers.get(&request.provider_id).cloned() else {
            return Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id,
                AgentError {
                    kind: AgentErrorKind::InvalidArgument,
                    message: "providerId is not configured in agentServers".to_string(),
                },
            )));
        };
        match self
            .agents
            .create_and_attach_session(request, provider)
            .await
        {
            Ok(session) => Ok(RpcResponse::CreateAgentSession(session).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn attach_agent_session(
        &mut self,
        request: AttachAgentSessionReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::AttachAgentSession.as_u64();
        let config = match load_runtime_config(&self.config_path, &self.config_state).await {
            Ok(config) => config,
            Err(error) => {
                return Ok(UnaryRpcOutcome::Message(agent_error_message(
                    proc_id,
                    AgentError {
                        kind: AgentErrorKind::Failed,
                        message: format!("load agent configuration: {error:#}"),
                    },
                )));
            }
        };
        match self
            .agents
            .attach_session(&request.session_id, &config.agent_servers)
            .await
        {
            Ok(session) => Ok(RpcResponse::AttachAgentSession(session).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn create_agent_turn(&mut self, request: CreateAgentTurnReq) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::CreateAgentTurn.as_u64();
        match self.agents.create_turn(request) {
            Ok(turn) => Ok(RpcResponse::CreateAgentTurn(turn).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn list_agent_session_turns(
        &mut self,
        request: ListAgentSessionTurnsReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::ListAgentSessionTurns.as_u64();
        match self.agents.list_session_turns(request) {
            Ok(response) => Ok(RpcResponse::ListAgentSessionTurns(response).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn set_agent_session_config(
        &mut self,
        request: SetAgentSessionConfigReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::SetAgentSessionConfig.as_u64();
        match self.agents.set_session_config(request).await {
            Ok(response) => Ok(RpcResponse::SetAgentSessionConfig(response).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn update_agent_session(
        &mut self,
        request: UpdateAgentSessionReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::UpdateAgentSession.as_u64();
        match self.agents.update_session(request) {
            Ok(session) => Ok(RpcResponse::UpdateAgentSession(session).into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    async fn remove_agent_project(
        &mut self,
        request: RemoveAgentProjectReq,
    ) -> Result<UnaryRpcOutcome> {
        let proc_id = ProcId::RemoveAgentProject.as_u64();
        match self.agents.remove_project(&request.project_id) {
            Ok(()) => Ok(RpcResponse::RemoveAgentProject.into()),
            Err(error) => Ok(UnaryRpcOutcome::Message(agent_error_message(
                proc_id, error,
            ))),
        }
    }

    fn get_daemon_info_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.get_daemon_info(request))
    }

    fn get_daemon_environment_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.get_daemon_environment(request))
    }

    fn start_pairing_rpc<'a>(
        &'a mut self,
        request: StartPairingRequest,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.start_pairing(request))
    }

    fn complete_pairing_rpc<'a>(
        &'a mut self,
        request: CompletePairingRequest,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.complete_pairing(request))
    }

    fn renew_client_credential_rpc<'a>(
        &'a mut self,
        request: (),
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.renew_client_credential(request))
    }

    fn remove_client_rpc<'a>(
        &'a mut self,
        request: RemoveClientReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.remove_client(request))
    }

    fn create_nodes_rpc<'a>(
        &'a mut self,
        request: CreateNodesReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_nodes(request))
    }

    fn rename_paths_rpc<'a>(
        &'a mut self,
        request: RenamePathsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.rename_paths(request))
    }

    fn delete_paths_rpc<'a>(
        &'a mut self,
        request: DeletePathsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.delete_paths(request))
    }

    fn create_terminal_session_rpc<'a>(
        &'a mut self,
        request: CreateTerminalSessionReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_terminal_session(request))
    }

    fn take_terminal_control_rpc<'a>(
        &'a mut self,
        request: TakeTerminalControlReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.take_terminal_control(request))
    }

    fn close_terminal_session_rpc<'a>(
        &'a mut self,
        request: CloseTerminalSessionReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.close_terminal_session(request))
    }

    fn kill_process_rpc<'a>(
        &'a mut self,
        request: KillProcessReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.kill_process(request))
    }

    fn restore_trash_items_rpc<'a>(
        &'a mut self,
        request: RestoreTrashItemsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.restore_trash_items(request))
    }

    fn purge_trash_items_rpc<'a>(
        &'a mut self,
        request: PurgeTrashItemsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.purge_trash_items(request))
    }

    fn run_command_rpc<'a>(
        &'a mut self,
        request: RunCommandReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.run_command(request))
    }

    fn create_job_rpc<'a>(
        &'a mut self,
        request: CreateJobReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_job(request))
    }

    fn kill_job_rpc<'a>(
        &'a mut self,
        request: KillJobReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.kill_job(request))
    }

    fn delete_jobs_rpc<'a>(
        &'a mut self,
        request: DeleteJobsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.delete_jobs(request))
    }

    fn clear_jobs_rpc<'a>(
        &'a mut self,
        request: ClearJobsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.clear_jobs(request))
    }

    fn create_schedule_rpc<'a>(
        &'a mut self,
        request: CreateScheduleReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_schedule(request))
    }

    fn update_schedule_rpc<'a>(
        &'a mut self,
        request: UpdateScheduleReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.update_schedule(request))
    }

    fn delete_schedules_rpc<'a>(
        &'a mut self,
        request: DeleteSchedulesReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.delete_schedules(request))
    }

    fn get_schedule_next_runs_rpc<'a>(
        &'a mut self,
        request: GetScheduleNextRunsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.get_schedule_next_runs(request))
    }

    fn create_agent_project_rpc<'a>(
        &'a mut self,
        request: CreateAgentProjectReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_agent_project(request))
    }

    fn list_agent_sessions_rpc<'a>(
        &'a mut self,
        request: ListAgentSessionsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.list_agent_sessions(request))
    }

    fn create_agent_session_rpc<'a>(
        &'a mut self,
        request: CreateAgentSessionReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_agent_session(request))
    }

    fn attach_agent_session_rpc<'a>(
        &'a mut self,
        request: AttachAgentSessionReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.attach_agent_session(request))
    }

    fn create_agent_turn_rpc<'a>(
        &'a mut self,
        request: CreateAgentTurnReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.create_agent_turn(request))
    }

    fn list_agent_session_turns_rpc<'a>(
        &'a mut self,
        request: ListAgentSessionTurnsReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.list_agent_session_turns(request))
    }

    fn set_agent_session_config_rpc<'a>(
        &'a mut self,
        request: SetAgentSessionConfigReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.set_agent_session_config(request))
    }

    fn update_agent_session_rpc<'a>(
        &'a mut self,
        request: UpdateAgentSessionReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.update_agent_session(request))
    }

    fn remove_agent_project_rpc<'a>(
        &'a mut self,
        request: RemoveAgentProjectReq,
    ) -> RpcHandlerFuture<'a, Result<UnaryRpcOutcome>> {
        Box::pin(self.remove_agent_project(request))
    }
}

fn requires_authentication(proc_id: u64) -> bool {
    proc_id != ProcId::GetDaemonInfo.as_u64()
        && proc_id != ProcId::StartPairing.as_u64()
        && proc_id != ProcId::CompletePairing.as_u64()
}

async fn create_nodes(files: &dyn FileService, request: CreateNodesReq) -> BulkMutationRes {
    let mut results = Vec::with_capacity(request.nodes.len());
    for (index, op) in request.nodes.into_iter().enumerate() {
        match files.create_node(op).await {
            Ok(()) => results.push(BulkMutationItemResult::ok(index)),
            Err(err) => results.push(BulkMutationItemResult::failed(index, err)),
        }
    }
    BulkMutationRes { results }
}

async fn rename_paths(files: &dyn FileService, request: RenamePathsReq) -> BulkMutationRes {
    let mut results = Vec::with_capacity(request.ops.len());
    for (index, op) in request.ops.into_iter().enumerate() {
        match files.rename_path(op.from, op.to).await {
            Ok(()) => results.push(BulkMutationItemResult::ok(index)),
            Err(err) => results.push(BulkMutationItemResult::failed(index, err)),
        }
    }
    BulkMutationRes { results }
}

async fn delete_paths(files: &dyn FileService, request: DeletePathsReq) -> BulkMutationRes {
    let mut results = Vec::with_capacity(request.paths.len());
    for (index, path) in request.paths.into_iter().enumerate() {
        match files.delete_path(path, request.mode).await {
            Ok(()) => results.push(BulkMutationItemResult::ok(index)),
            Err(err) => results.push(BulkMutationItemResult::failed(index, err)),
        }
    }
    BulkMutationRes { results }
}

async fn restore_trash_items(
    files: &dyn FileService,
    request: RestoreTrashItemsReq,
) -> BulkMutationRes {
    let mut results = Vec::with_capacity(request.item_ids.len());
    for (index, item_id) in request.item_ids.into_iter().enumerate() {
        match files.restore_trash_item(item_id).await {
            Ok(()) => results.push(BulkMutationItemResult::ok(index)),
            Err(err) => results.push(BulkMutationItemResult::failed(index, err)),
        }
    }
    BulkMutationRes { results }
}

async fn purge_trash_items(
    files: &dyn FileService,
    request: PurgeTrashItemsReq,
) -> BulkMutationRes {
    let mut results = Vec::with_capacity(request.item_ids.len());
    for (index, item_id) in request.item_ids.into_iter().enumerate() {
        match files.purge_trash_item(item_id).await {
            Ok(()) => results.push(BulkMutationItemResult::ok(index)),
            Err(err) => results.push(BulkMutationItemResult::failed(index, err)),
        }
    }
    BulkMutationRes { results }
}

fn bulk_mutation_has_ok(response: &BulkMutationRes) -> bool {
    response
        .results
        .iter()
        .any(|result| matches!(result, BulkMutationItemResult::Ok { .. }))
}

fn notify_trash_changed(trash_events: &SharedTrashEvents) {
    let next = {
        let current = *trash_events.borrow();
        current.wrapping_add(1)
    };
    trash_events.send_replace(next);
}

async fn is_authenticated(session_state: &SharedRpcSessionState) -> bool {
    session_state.lock().await.authenticated_client_id.is_some()
}

async fn authenticated_client_id(session_state: &SharedRpcSessionState) -> Option<String> {
    session_state.lock().await.authenticated_client_id.clone()
}

async fn rpc_session_id(session_state: &SharedRpcSessionState) -> RpcSessionId {
    session_state.lock().await.session_id
}

async fn begin_pairing_attempt(
    pairing_state: &SharedPairingChallenge,
    attempt_key: PairingAttemptKey,
) -> PairingAttemptId {
    let mut state = pairing_state.lock().await;
    state.next_attempt_id += 1;
    let attempt_id = state.next_attempt_id;
    state
        .current_attempts
        .insert(attempt_key.clone(), attempt_id);
    if state
        .active_challenge
        .as_ref()
        .is_some_and(|challenge| challenge.attempt_key == attempt_key)
    {
        state.active_challenge = None;
    }
    attempt_id
}

async fn is_current_pairing_attempt(
    pairing_state: &SharedPairingChallenge,
    attempt_key: &PairingAttemptKey,
    attempt_id: PairingAttemptId,
) -> bool {
    pairing_state
        .lock()
        .await
        .current_attempts
        .get(attempt_key)
        .is_some_and(|current| *current == attempt_id)
}

fn pairing_attempt_key(client_id: Option<&str>) -> PairingAttemptKey {
    match client_id {
        Some(client_id) => PairingAttemptKey::ClientId(client_id.to_string()),
        None => PairingAttemptKey::Anonymous,
    }
}

fn next_rpc_session_id() -> RpcSessionId {
    NEXT_RPC_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

async fn verify_session_credentials(
    credential: &PairedSecretCredential,
    client_credentials: &SharedClientCredentials,
) -> bool {
    let now = now_unix();
    let state = client_credentials.lock().await;
    state.clients.iter().any(|record| {
        record.client_id == credential.credential_id
            && verify_client_credential(record, &credential.credential_secret, now)
    })
}

async fn load_runtime_config(
    config_path: &Path,
    config_state: &SharedSystemConfig,
) -> Result<SystemConfig> {
    let config = load_or_default(config_path)?;
    *config_state.lock().await = config.clone();
    Ok(config)
}

async fn load_runtime_client_credentials(
    credentials_path: &Path,
    client_credentials: &SharedClientCredentials,
) -> Result<ClientCredentials> {
    let state = load_client_credentials_or_default(credentials_path)?;
    *client_credentials.lock().await = state.clone();
    Ok(state)
}

#[cfg(test)]
async fn temporary_client_credentials_events(
    client_credentials: &SharedClientCredentials,
) -> SharedClientCredentialsEvents {
    let state = client_credentials.lock().await.clone();
    let (tx, _) = watch::channel(state);
    tx
}

#[cfg(test)]
fn test_commands() -> SharedCommandManager {
    Arc::new(CommandManager::open(DaemonStateDb::open_in_memory_for_tests().unwrap()).unwrap())
}

#[cfg(test)]
fn test_agents() -> SharedAgentManager {
    Arc::new(
        AgentManager::open(
            DaemonStateDb::open_in_memory_for_tests().unwrap(),
            std::env::temp_dir().join("Rieul-test-agent-workspaces"),
        )
        .unwrap(),
    )
}

fn client_info_from_record(record: &ClientCredentialRecord) -> ClientInfo {
    ClientInfo {
        client_id: record.client_id.clone(),
        label: record.label.clone(),
        created_at_unix: record.created_at_unix,
        expires_at_unix: record.expires_at_unix,
    }
}

fn client_infos_from_credentials(credentials: &ClientCredentials) -> Vec<ClientInfo> {
    credentials
        .clients
        .iter()
        .map(client_info_from_record)
        .collect()
}

fn clients_patch(previous: &[ClientInfo], next: &[ClientInfo]) -> Option<ClientsTableEvent> {
    let previous_by_id: BTreeMap<String, ClientInfo> = previous
        .iter()
        .map(|row| (row.client_id.clone(), row.clone()))
        .collect();
    let next_by_id: BTreeMap<String, ClientInfo> = next
        .iter()
        .map(|row| (row.client_id.clone(), row.clone()))
        .collect();
    let removes = previous_by_id
        .keys()
        .filter(|client_id| !next_by_id.contains_key(*client_id))
        .map(|client_id| ClientKey {
            client_id: client_id.clone(),
        })
        .collect::<Vec<_>>();
    let upserts = next
        .iter()
        .filter(|row| previous_by_id.get(&row.client_id) != Some(*row))
        .cloned()
        .collect::<Vec<_>>();

    if removes.is_empty() && upserts.is_empty() {
        None
    } else {
        Some(ClientsTableEvent::Patch { removes, upserts })
    }
}

async fn store_runtime_client_credentials(
    credentials_path: &Path,
    client_credentials: &SharedClientCredentials,
    client_credentials_events: &SharedClientCredentialsEvents,
    state: ClientCredentials,
) -> Result<()> {
    save_client_credentials(credentials_path, &state)?;
    *client_credentials.lock().await = state.clone();
    let _ = client_credentials_events.send(state);
    Ok(())
}

fn unauthorized_message(proc_id: u64) -> ReqResMessage {
    generic_error_message(
        proc_id,
        RpcErrorCode::Unauthorized,
        "valid paired client credentials are required",
    )
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn normalize_confirmation_code(raw: &str) -> Option<String> {
    let code = raw.trim();
    if code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(code.to_string())
    } else {
        None
    }
}

fn pairing_client_label(raw: &str) -> String {
    let label = raw.trim();
    if label.is_empty() {
        "browser".to_string()
    } else {
        label.to_string()
    }
}

fn normalize_pairing_client_id(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|client_id| !client_id.is_empty())
        .map(ToOwned::to_owned)
}

fn pairing_daemon_url(config: &SystemConfig) -> String {
    let port = config
        .listen_addr
        .parse::<SocketAddr>()
        .map(|addr| addr.port())
        .unwrap_or(9012);
    if let Some(domain) = config
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
    {
        if port == 443 {
            return format!("https://{domain}");
        }
        return format!("https://{domain}:{port}");
    }
    format!("https://localhost:{port}")
}

fn ok_payload_message(_proc_id: u64, payload: Vec<u8>) -> ReqResMessage {
    ReqResMessage::ResponseUnaryOk {
        payload: Some(payload),
    }
}

fn ok_void_message(_proc_id: u64) -> ReqResMessage {
    ReqResMessage::ResponseUnaryOk { payload: None }
}

fn stream_start_payload_message(payload: Vec<u8>) -> ReqResMessage {
    ReqResMessage::ResponseStreamStart {
        payload: Some(payload),
    }
}

fn stream_chunk_payload_message(payload: Vec<u8>) -> ReqResMessage {
    ReqResMessage::ResponseStreamChunk { payload }
}

fn service_error_message(proc_id: u64, err: ServiceError) -> ReqResMessage {
    let code = service_error_code(&err);
    error_message(proc_id, code, &err.to_string())
}

fn stream_service_error_message(proc_id: u64, err: ServiceError) -> ReqResMessage {
    let code = service_error_code(&err);
    stream_error_message(proc_id, code, &err.to_string())
}

fn terminal_service_error_message(proc_id: u64, err: ServiceError) -> ReqResMessage {
    let code = terminal_service_error_code(&err);
    error_message(proc_id, code, &err.to_string())
}

fn terminal_stream_service_error_message(proc_id: u64, err: ServiceError) -> ReqResMessage {
    let code = terminal_service_error_code(&err);
    stream_error_message(proc_id, code, &err.to_string())
}

fn command_error_message(proc_id: u64, err: CommandError) -> ReqResMessage {
    error_message(proc_id, command_error_code(err.kind), &err.message)
}

fn stream_command_error_message(proc_id: u64, err: CommandError) -> ReqResMessage {
    stream_error_message(proc_id, command_error_code(err.kind), &err.message)
}

fn agent_error_message(proc_id: u64, error: AgentError) -> ReqResMessage {
    error_message(proc_id, agent_error_code(error.kind), &error.message)
}

fn stream_agent_error_message(proc_id: u64, error: AgentError) -> ReqResMessage {
    stream_error_message(proc_id, agent_error_code(error.kind), &error.message)
}

fn agent_error_code(kind: AgentErrorKind) -> &'static str {
    match kind {
        AgentErrorKind::Failed => "failed",
        AgentErrorKind::NotFound => "not_found",
        AgentErrorKind::InvalidArgument => "invalid_argument",
        AgentErrorKind::Conflict => "conflict",
        AgentErrorKind::Unavailable => "unavailable",
        AgentErrorKind::PermissionDenied => "permission_denied",
    }
}

fn command_error_code(kind: CommandErrorKind) -> &'static str {
    match kind {
        CommandErrorKind::Failed => "failed",
        CommandErrorKind::NotFound => "not_found",
        CommandErrorKind::PermissionDenied => "permission_denied",
        CommandErrorKind::ElevationUnavailable => "elevation_unavailable",
        CommandErrorKind::InvalidLaunch => "invalid_launch",
        CommandErrorKind::InvalidRRuleSet => "invalid_rrule_set",
        CommandErrorKind::LogDisabled => "log_disabled",
    }
}

fn terminal_service_error_code(err: &ServiceError) -> &'static str {
    match err {
        ServiceError::NotFound => "not_found",
        ServiceError::PermissionDenied => "permission_denied",
        ServiceError::OperationFailed(message) if message.contains("attach not found") => {
            "attach_not_found"
        }
        ServiceError::OperationFailed(message) if message.contains("not the live primary") => {
            "not_primary_attach"
        }
        ServiceError::OperationFailed(message) if message.contains("terminal size is invalid") => {
            "invalid_size"
        }
        ServiceError::OperationFailed(message) if message.contains("command is empty") => {
            "invalid_launch"
        }
        ServiceError::OperationFailed(message) if message.contains("no shell is available") => {
            "shell_not_found"
        }
        _ => service_error_code(err),
    }
}

fn service_error_code(err: &ServiceError) -> &'static str {
    match err {
        ServiceError::PermissionDenied => "permission_denied",
        ServiceError::NotFound => "not_found",
        ServiceError::AlreadyExists => "already_exists",
        ServiceError::NotDirectory => "not_directory",
        ServiceError::NotFile => "not_file",
        ServiceError::InvalidPath => "invalid_path",
        ServiceError::Unsupported => "unsupported",
        ServiceError::OperationFailed(_) => "failed",
    }
}

fn error_message(proc_id: u64, code: &str, message: &str) -> ReqResMessage {
    let (error_kind, error) = match method_error_payload(proc_id, code, message) {
        Some(error) => (RpcErrorKind::Method, error),
        None => (
            RpcErrorKind::System,
            RpcErrorPayload {
                code: rpc_error_code(code),
                message: message.to_string(),
            }
            .encode(),
        ),
    };

    ReqResMessage::ResponseUnaryError { error, error_kind }
}

fn stream_error_message(proc_id: u64, code: &str, message: &str) -> ReqResMessage {
    let (error_kind, error) = match method_error_payload(proc_id, code, message) {
        Some(error) => (RpcErrorKind::Method, error),
        None => (
            RpcErrorKind::System,
            RpcErrorPayload {
                code: rpc_error_code(code),
                message: message.to_string(),
            }
            .encode(),
        ),
    };

    ReqResMessage::ResponseStreamErrorEnd { error, error_kind }
}

fn generic_error_message(_proc_id: u64, code: RpcErrorCode, message: &str) -> ReqResMessage {
    ReqResMessage::ResponseUnaryError {
        error_kind: RpcErrorKind::System,
        error: RpcErrorPayload {
            code,
            message: message.to_string(),
        }
        .encode(),
    }
}

fn rpc_request_decode_error_message(proc_id: u64, err: RpcRequestDecodeError) -> ReqResMessage {
    match err {
        RpcRequestDecodeError::UnknownProcId(_) => generic_error_message(
            proc_id,
            RpcErrorCode::NotImplemented,
            "this RPC is reserved but not implemented in the first cut",
        ),
        RpcRequestDecodeError::MissingPayload { proc } => generic_error_message(
            proc_id,
            RpcErrorCode::MissingPayload,
            &format!("{} requires a payload", proc.name()),
        ),
        RpcRequestDecodeError::MalformedPayload { proc, .. } => generic_error_message(
            proc_id,
            RpcErrorCode::MalformedPayload,
            &format!("{} payload is malformed", proc.name()),
        ),
    }
}

fn stream_rpc_request_decode_error_message(
    proc_id: u64,
    err: RpcRequestDecodeError,
) -> ReqResMessage {
    match err {
        RpcRequestDecodeError::UnknownProcId(_) => stream_generic_error_message(
            proc_id,
            RpcErrorCode::NotImplemented,
            "this RPC is reserved but not implemented in the first cut",
        ),
        RpcRequestDecodeError::MissingPayload { proc } => stream_generic_error_message(
            proc_id,
            RpcErrorCode::MissingPayload,
            &format!("{} requires a payload", proc.name()),
        ),
        RpcRequestDecodeError::MalformedPayload { proc, .. } => stream_generic_error_message(
            proc_id,
            RpcErrorCode::MalformedPayload,
            &format!("{} payload is malformed", proc.name()),
        ),
    }
}

fn stream_generic_error_message(_proc_id: u64, code: RpcErrorCode, message: &str) -> ReqResMessage {
    ReqResMessage::ResponseStreamErrorEnd {
        error_kind: RpcErrorKind::System,
        error: RpcErrorPayload {
            code,
            message: message.to_string(),
        }
        .encode(),
    }
}

fn session_auth_error_message(code: SessionAuthErrorCode, message: &str) -> ReqResMessage {
    ReqResMessage::SessionAuthError {
        code,
        message: message.to_string(),
    }
}

fn rpc_error_code(code: &str) -> RpcErrorCode {
    match code {
        "bad_message" => RpcErrorCode::BadMessage,
        "unauthorized" => RpcErrorCode::Unauthorized,
        "missing_payload" => RpcErrorCode::MissingPayload,
        "not_implemented" => RpcErrorCode::NotImplemented,
        "permission_denied" => RpcErrorCode::PermissionDenied,
        "not_found" => RpcErrorCode::NotFound,
        "failed" | "operation_failed" => RpcErrorCode::OperationFailed,
        "malformed_payload" => RpcErrorCode::MalformedPayload,
        _ => RpcErrorCode::OperationFailed,
    }
}

fn method_error_payload(proc_id: u64, code: &str, message: &str) -> Option<Vec<u8>> {
    ProcId::from_u64(proc_id)?.method_error_payload(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rieul_daemon_core::cbor::Value;
    use rieul_daemon_core::pairing::{
        create_pairing_code, issue_client_secret, verify_client_secret,
        CLIENT_CREDENTIAL_TTL_SECONDS, PAIRING_TTL_SECONDS,
    };
    use rieul_daemon_core::rpc::{
        CompletePairingRequest, CreateNodeOp, DeleteMode, FsEntryKind, ReadFileReq,
        StartPairingRequest, WriteFileResult, WriteFileStart,
    };

    #[test]
    fn creates_startup_config_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nested").join("rieul.yaml");
        let addr: SocketAddr = "127.0.0.1:9012".parse().unwrap();

        let startup = load_startup_config(&config_path, Some(addr)).unwrap();

        assert_eq!(startup.listen_addr, addr);
        assert_eq!(startup.config.listen_addr, "127.0.0.1:9012");
        assert_eq!(load_or_default(&config_path).unwrap(), startup.config);
    }

    #[test]
    fn startup_config_preserves_existing_listen_without_override() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let config = SystemConfig {
            listen_addr: "127.0.0.1:7777".to_string(),
            ..SystemConfig::default()
        };
        save(&config_path, &config).unwrap();

        let startup = load_startup_config(&config_path, None).unwrap();

        assert_eq!(startup.listen_addr, "127.0.0.1:7777".parse().unwrap());
        assert_eq!(startup.config.listen_addr, "127.0.0.1:7777");
        assert_eq!(
            load_or_default(&config_path).unwrap().listen_addr,
            "127.0.0.1:7777"
        );
    }

    #[test]
    fn startup_config_uses_explicit_listen_override() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let config = SystemConfig {
            listen_addr: "127.0.0.1:7777".to_string(),
            ..SystemConfig::default()
        };
        save(&config_path, &config).unwrap();
        let override_addr: SocketAddr = "127.0.0.1:8888".parse().unwrap();

        let startup = load_startup_config(&config_path, Some(override_addr)).unwrap();

        assert_eq!(startup.listen_addr, override_addr);
        assert_eq!(startup.config.listen_addr, "127.0.0.1:8888");
        assert_eq!(
            load_or_default(&config_path).unwrap().listen_addr,
            "127.0.0.1:8888"
        );
    }

    #[tokio::test]
    async fn get_daemon_info_reports_registered_supported_procs() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);

        let responses = handle_rpc_messages(
            vec![request_message(ProcId::GetDaemonInfo, None)],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            ReqResMessage::ResponseUnaryOk { .. }
        ));
        let daemon_info = DaemonInfo::decode(payload(&responses[0])).unwrap();
        let expected_handlers = build_rpc_handlers(None, None, None, None);
        assert_eq!(
            daemon_info.supported_proc_ids,
            expected_handlers
                .supported_procs()
                .iter()
                .map(|proc| proc.as_u64())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn create_agent_session_reports_unavailable_agent_process() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let config = SystemConfig {
            agent_servers: BTreeMap::from([(
                "test-agent".to_string(),
                rieul_daemon_core::config::AgentServerConfig {
                    command: "test-agent".to_string(),
                    args: Vec::new(),
                    env: BTreeMap::new(),
                },
            )]),
            ..SystemConfig::default()
        };
        save(&config_path, &config).unwrap();
        let request = CreateAgentSessionReq {
            provider_id: "test-agent".to_string(),
            workspace: rieul_daemon_core::generated::rpc::CreateAgentWorkspace::Task {
                source: rieul_daemon_core::generated::rpc::AgentTaskWorkspaceSource::Empty,
            },
            title: Some("Test session".to_string()),
            creation_request_id: "globally-unique-request".to_string(),
        };

        let responses = handle_rpc_messages(
            vec![request_message(
                ProcId::CreateAgentSession,
                Some(request.encode()),
            )],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(config)),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            Arc::new(Mutex::new(RpcSessionState {
                session_id: 1,
                authenticated_client_id: Some("arbitrary-client".to_string()),
            })),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert!(matches!(
            responses.as_slice(),
            [ReqResMessage::ResponseUnaryError {
                error_kind: RpcErrorKind::Method,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn get_daemon_environment_reports_home_directory() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);

        let responses = handle_rpc_messages(
            vec![request_message(ProcId::GetDaemonEnvironment, None)],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            Arc::new(Mutex::new(RpcSessionState {
                session_id: 1,
                authenticated_client_id: Some("test-client".to_string()),
            })),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        let environment = DaemonEnvironment::decode(payload(&responses[0])).unwrap();
        assert_eq!(
            environment.home_directory,
            DaemonEnvironment::current().home_directory
        );
    }

    #[tokio::test]
    async fn complete_pairing_reads_config_written_after_server_start() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let pairing = create_pairing_code(now_unix());
        save(&config_path, &SystemConfig::default()).unwrap();

        let config_state = Arc::new(Mutex::new(SystemConfig::default()));
        let client_credentials = Arc::new(Mutex::new(ClientCredentials::default()));
        let pairing_challenge =
            test_pairing_challenge_with_label(pairing.record.clone(), "test-browser", None);
        let notifier = RecordingPairingNotifier::default();
        let request = request_message(
            ProcId::CompletePairing,
            Some(CompletePairingRequest { code: pairing.code }.encode()),
        );
        let session_state = Arc::new(Mutex::new(RpcSessionState::default()));
        let responses = handle_rpc_messages(
            vec![request],
            &config_path,
            &credentials_path,
            config_state,
            client_credentials.clone(),
            pairing_challenge.clone(),
            session_state.clone(),
            test_files(),
            test_terminals(),
            Some(Arc::new(notifier.clone())),
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 1);
        let response = &responses[0];

        assert!(matches!(response, ReqResMessage::ResponseUnaryOk { .. }));
        let credentials = CompletePairingResponse::decode(payload(response)).unwrap();
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(active_pairing_challenge(&pairing_challenge).await, None);
        assert_eq!(stored.clients.len(), 1);
        assert_eq!(
            stored.clients[0].expires_at_unix,
            credentials.client_credential_expires_at_unix
        );
        assert!(verify_client_secret(
            &stored.clients[0],
            &credentials.client_secret
        ));
        assert_eq!(client_credentials.lock().await.clients.len(), 1);
        assert_eq!(session_state.lock().await.authenticated_client_id, None);
        assert_eq!(*notifier.completed.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn complete_pairing_reuses_requested_existing_client_id() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let pairing = create_pairing_code(now_unix());
        let existing = issue_client_secret("test-browser", now_unix() - 10);
        let existing_client_id = existing.client_id.clone();
        let existing_created_at_unix = existing.record.created_at_unix;
        let old_secret = existing.client_secret.clone();
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![existing.record],
            },
        )
        .unwrap();

        let config_state = Arc::new(Mutex::new(SystemConfig::default()));
        let client_credentials = Arc::new(Mutex::new(ClientCredentials::default()));
        let pairing_challenge = test_pairing_challenge_with_label(
            pairing.record.clone(),
            "test-browser",
            Some(existing_client_id.clone()),
        );
        let request = request_message(
            ProcId::CompletePairing,
            Some(CompletePairingRequest { code: pairing.code }.encode()),
        );
        let responses = handle_rpc_messages(
            vec![request],
            &config_path,
            &credentials_path,
            config_state,
            client_credentials,
            pairing_challenge.clone(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        let response = &responses[0];
        assert!(matches!(response, ReqResMessage::ResponseUnaryOk { .. }));
        let credentials = CompletePairingResponse::decode(payload(response)).unwrap();
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(credentials.client_id, existing_client_id);
        assert_eq!(active_pairing_challenge(&pairing_challenge).await, None);
        assert_eq!(stored.clients.len(), 1);
        assert_eq!(stored.clients[0].client_id, existing_client_id);
        assert_eq!(stored.clients[0].created_at_unix, existing_created_at_unix);
        assert_eq!(
            stored.clients[0].expires_at_unix,
            credentials.client_credential_expires_at_unix
        );
        assert!(verify_client_secret(
            &stored.clients[0],
            &credentials.client_secret
        ));
        assert!(!verify_client_secret(&stored.clients[0], &old_secret));
    }

    #[tokio::test]
    async fn complete_pairing_does_not_reuse_client_id_by_label() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let pairing = create_pairing_code(now_unix());
        let existing = issue_client_secret("test-browser", now_unix() - 10);
        let existing_client_id = existing.client_id.clone();
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![existing.record],
            },
        )
        .unwrap();
        let pairing_challenge =
            test_pairing_challenge_with_label(pairing.record.clone(), "test-browser", None);

        let request = request_message(
            ProcId::CompletePairing,
            Some(CompletePairingRequest { code: pairing.code }.encode()),
        );
        let responses = handle_rpc_messages(
            vec![request],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            pairing_challenge.clone(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        let response = &responses[0];
        assert!(matches!(response, ReqResMessage::ResponseUnaryOk { .. }));
        let credentials = CompletePairingResponse::decode(payload(response)).unwrap();
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(active_pairing_challenge(&pairing_challenge).await, None);
        assert_ne!(credentials.client_id, existing_client_id);
        assert_eq!(stored.clients.len(), 2);
        assert!(stored
            .clients
            .iter()
            .any(|record| record.client_id == existing_client_id));
        assert!(stored
            .clients
            .iter()
            .any(|record| record.client_id == credentials.client_id));
        assert!(stored.clients.iter().any(|record| {
            record.client_id == credentials.client_id
                && record.expires_at_unix == credentials.client_credential_expires_at_unix
        }));
    }

    #[tokio::test]
    async fn complete_pairing_rejects_matching_code_from_different_session() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let pairing = create_pairing_code(now_unix());
        save(&config_path, &SystemConfig::default()).unwrap();

        let pairing_challenge = test_pairing_challenge_with_session(7, pairing.record.clone());
        let request = request_message(
            ProcId::CompletePairing,
            Some(CompletePairingRequest { code: pairing.code }.encode()),
        );
        let responses = handle_rpc_messages(
            vec![request],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            pairing_challenge.clone(),
            test_session_state(8),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            ReqResMessage::ResponseUnaryError {
                error_kind: RpcErrorKind::Method,
                ..
            }
        ));
        let Value::Array(error_items) = Value::decode(error(&responses[0])).unwrap() else {
            panic!("expected method error union");
        };
        assert_eq!(error_items.first(), Some(&Value::U64(1)));
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(stored.clients.len(), 0);
        let active = active_pairing_challenge(&pairing_challenge).await.unwrap();
        assert_eq!(active.owner_session_id, 7);
        assert_eq!(active.record, pairing.record);
        assert_eq!(active.client_label, "browser");
        assert_eq!(active.client_id, None);
    }

    #[tokio::test]
    async fn start_pairing_creates_runtime_pairing_code() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        save(&config_path, &SystemConfig::default()).unwrap();

        let config_state = Arc::new(Mutex::new(SystemConfig::default()));
        let client_credentials = Arc::new(Mutex::new(ClientCredentials::default()));
        let pairing_challenge = test_pairing_challenge();
        let notifier = RecordingPairingNotifier::default();
        let responses = handle_rpc_messages(
            vec![start_pairing_request_message("42")],
            &config_path,
            &credentials_path,
            config_state,
            client_credentials.clone(),
            pairing_challenge.clone(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            Some(Arc::new(notifier.clone())),
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 1);
        let response = &responses[0];

        assert!(matches!(response, ReqResMessage::ResponseUnaryOk { .. }));
        let Value::Map(response_payload) = Value::decode(payload(response)).unwrap() else {
            panic!("expected StartPairing response map");
        };
        let pairing_code_expires_at_unix = match response_payload.get(&1) {
            Some(Value::I64(value)) => *value,
            Some(Value::U64(value)) => *value as i64,
            _ => panic!("expected StartPairing pairing_code_expires_at_unix"),
        };
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(stored.clients.len(), 0);
        let pairing = active_pairing_challenge(&pairing_challenge).await.unwrap();
        assert_eq!(pairing_code_expires_at_unix, pairing.record.expires_at_unix);
        assert_eq!(pairing.client_label, "test-browser");
        assert_eq!(pairing.client_id, Some("existing-client".to_string()));
        assert_eq!(client_credentials.lock().await.clients.len(), 0);
    }

    #[tokio::test]
    async fn start_pairing_notifies_local_gui() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        save(&config_path, &SystemConfig::default()).unwrap();

        let notifier = RecordingPairingNotifier::default();
        let pairing_challenge = test_pairing_challenge();
        let responses = handle_rpc_messages(
            vec![start_pairing_request_message("42")],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            pairing_challenge.clone(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            Some(Arc::new(notifier.clone())),
        )
        .await
        .unwrap();

        assert!(matches!(
            responses.first(),
            Some(ReqResMessage::ResponseUnaryOk { .. })
        ));
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(stored.clients.len(), 0);
        let pairing = active_pairing_challenge(&pairing_challenge).await.unwrap();
        let confirmations = notifier.confirmations.lock().unwrap();
        assert_eq!(confirmations.len(), 1);
        assert_eq!(confirmations[0].daemon_url, "https://localhost:9012");
        assert_eq!(confirmations[0].confirmation_code, "42");
        assert_eq!(confirmations[0].client_label, "test-browser");
        drop(confirmations);
        let notifications = notifier.notifications.lock().unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].daemon_url, "https://localhost:9012");
        assert!(verify_pairing_code(
            &pairing.record,
            &notifications[0].pairing_code,
            now_unix()
        ));
        assert!(notifications[0].expires_in_seconds > 0);
        assert!(notifications[0].expires_in_seconds <= PAIRING_TTL_SECONDS);
    }

    #[tokio::test]
    async fn newer_start_pairing_supersedes_pending_attempt_for_same_client_id() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        save(&config_path, &SystemConfig::default()).unwrap();

        let pairing_challenge = test_pairing_challenge();
        let config_state = Arc::new(Mutex::new(SystemConfig::default()));
        let client_credentials = Arc::new(Mutex::new(ClientCredentials::default()));
        let (notifier, first_started, release_first) = BlockingFirstPairingNotifier::new();
        let first_config_path = config_path.clone();
        let first_credentials_path = credentials_path.clone();
        let first_pairing_challenge = pairing_challenge.clone();
        let first_config_state = config_state.clone();
        let first_client_credentials = client_credentials.clone();
        let first_notifier = notifier.clone();
        let first_task = tokio::spawn(async move {
            handle_rpc_messages(
                vec![start_pairing_request_message("42")],
                &first_config_path,
                &first_credentials_path,
                first_config_state,
                first_client_credentials,
                first_pairing_challenge,
                test_session_state(1),
                test_files(),
                test_terminals(),
                Some(Arc::new(first_notifier)),
            )
            .await
            .unwrap()
        });
        first_started.await.unwrap();

        let second_responses = handle_rpc_messages(
            vec![start_pairing_request_message("43")],
            &config_path,
            &credentials_path,
            config_state,
            client_credentials,
            pairing_challenge.clone(),
            test_session_state(2),
            test_files(),
            test_terminals(),
            Some(Arc::new(notifier.clone())),
        )
        .await
        .unwrap();
        assert!(matches!(
            second_responses.first(),
            Some(ReqResMessage::ResponseUnaryOk { .. })
        ));

        release_first.send(()).unwrap();
        let first_responses = first_task.await.unwrap();
        assert!(matches!(
            first_responses.first(),
            Some(ReqResMessage::ResponseUnaryError {
                error_kind: RpcErrorKind::Method,
                ..
            })
        ));

        let confirmations = notifier.confirmations.lock().unwrap();
        assert_eq!(confirmations.len(), 2);
        assert_eq!(confirmations[0].confirmation_code, "42");
        assert_eq!(confirmations[1].confirmation_code, "43");
        drop(confirmations);

        let notifications = notifier.notifications.lock().unwrap();
        assert_eq!(notifications.len(), 1);
        let active = active_pairing_challenge(&pairing_challenge).await.unwrap();
        assert_eq!(active.owner_session_id, 2);
        assert!(verify_pairing_code(
            &active.record,
            &notifications[0].pairing_code,
            now_unix()
        ));
    }

    #[tokio::test]
    async fn start_pairing_fails_when_daemon_cannot_show_pairing_code() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        save(&config_path, &SystemConfig::default()).unwrap();
        let pairing_challenge = test_pairing_challenge();

        let responses = handle_rpc_messages(
            vec![start_pairing_request_message("42")],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            pairing_challenge.clone(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        let response = &responses[0];
        assert!(matches!(
            response,
            ReqResMessage::ResponseUnaryError {
                error_kind: RpcErrorKind::Method,
                ..
            }
        ));
        let Value::Array(error_items) = Value::decode(error(response)).unwrap() else {
            panic!("expected method error union");
        };
        assert_eq!(error_items.first(), Some(&Value::U64(0)));
        assert_eq!(active_pairing_challenge(&pairing_challenge).await, None);
    }

    #[tokio::test]
    async fn filesystem_rpc_requires_paired_client_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        save(&config_path, &SystemConfig::default()).unwrap();

        let responses = handle_rpc_messages(
            vec![request_message(ProcId::CreateNodes, None)],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            Arc::new(Mutex::new(RpcSessionState::default())),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 1);
        let response = &responses[0];

        assert!(matches!(response, ReqResMessage::ResponseUnaryError { .. }));
        let error = RpcErrorPayload::decode(error(response)).unwrap();
        assert_eq!(error.code, RpcErrorCode::Unauthorized);
    }

    #[test]
    fn filesystem_method_errors_use_schema_variant_ids() {
        let response = service_error_message(ProcId::ReadFile.as_u64(), ServiceError::NotFile);

        assert_eq!(response.error_kind(), Some(RpcErrorKind::Method));
        let Value::Array(error_items) = Value::decode(error(&response)).unwrap() else {
            panic!("expected method error union");
        };
        assert_eq!(error_items.first(), Some(&Value::U64(3)));
    }

    #[test]
    fn wire_datagram_ping_returns_pong() {
        let request = DatagramMessage::Ping { ping_id: 7 }.encode();
        let response = handle_wire_datagram(&request).unwrap();

        assert_eq!(
            DatagramMessage::decode(&response).unwrap(),
            DatagramMessage::Pong { ping_id: 7 }
        );
    }

    #[test]
    fn wire_datagram_pong_is_consumed() {
        let request = DatagramMessage::Pong { ping_id: 7 }.encode();

        assert_eq!(handle_wire_datagram(&request), None);
    }

    #[test]
    fn roots_patch_reports_removed_and_changed_rows() {
        let previous = vec![
            fs_entry("System", "C:\\", FsEntryKind::Directory, Some(10)),
            fs_entry("Data", "D:\\", FsEntryKind::Directory, Some(20)),
        ];
        let next = vec![
            fs_entry("Data", "D:\\", FsEntryKind::Directory, Some(21)),
            fs_entry("Backup", "E:\\", FsEntryKind::Directory, Some(30)),
        ];

        let Some(RootsTableEvent::Patch { removes, upserts }) = roots_patch(&previous, &next)
        else {
            panic!("expected roots patch");
        };

        assert_eq!(
            removes,
            vec![RootEntryKey {
                path: "C:\\".to_string()
            }]
        );
        assert_eq!(upserts, next);
    }

    #[test]
    fn directory_patch_reports_removed_and_changed_rows() {
        let previous = vec![
            fs_entry("a.txt", "C:\\dir\\a.txt", FsEntryKind::File, Some(10)),
            fs_entry("b.txt", "C:\\dir\\b.txt", FsEntryKind::File, Some(20)),
        ];
        let next = vec![
            fs_entry("b.txt", "C:\\dir\\b.txt", FsEntryKind::File, Some(21)),
            fs_entry("c.txt", "C:\\dir\\c.txt", FsEntryKind::File, Some(30)),
        ];

        let Some(DirectoryTableEvent::Patch { removes, upserts }) =
            directory_patch(&previous, &next)
        else {
            panic!("expected directory patch");
        };

        assert_eq!(
            removes,
            vec![DirectoryEntryKey {
                name: "a.txt".to_string()
            }]
        );
        assert_eq!(upserts, next);
    }

    #[test]
    fn subscription_patch_returns_none_when_rows_are_unchanged() {
        let rows = vec![fs_entry(
            "a.txt",
            "C:\\dir\\a.txt",
            FsEntryKind::File,
            Some(10),
        )];

        assert_eq!(roots_patch(&rows, &rows), None);
        assert_eq!(directory_patch(&rows, &rows), None);
    }

    #[tokio::test]
    async fn session_authenticate_marks_session_authenticated() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let issued = issue_client_secret("test-browser", now_unix());
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![issued.record],
                ..ClientCredentials::default()
            },
        )
        .unwrap();
        let config_state = Arc::new(Mutex::new(load_or_default(&config_path).unwrap()));
        let client_credentials = Arc::new(Mutex::new(
            load_client_credentials_or_default(&credentials_path).unwrap(),
        ));
        let session_state = Arc::new(Mutex::new(RpcSessionState::default()));

        let responses = handle_reqres_messages(
            vec![ReqResMessage::SessionAuthenticate {
                mechanism: PAIRED_SECRET_AUTH_MECHANISM.to_string(),
                payload: PairedSecretCredential {
                    credential_id: issued.client_id.clone(),
                    credential_secret: issued.client_secret,
                }
                .encode(),
            }],
            &config_path,
            &credentials_path,
            config_state,
            client_credentials,
            test_pairing_challenge(),
            session_state.clone(),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 1);
        let response = &responses[0];

        assert!(matches!(response, ReqResMessage::SessionAuthenticated));
        assert_eq!(
            session_state.lock().await.authenticated_client_id,
            Some(issued.client_id)
        );
    }

    #[tokio::test]
    async fn session_authenticate_rejects_expired_client_credential() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let mut issued = issue_client_secret("test-browser", now_unix());
        issued.record.expires_at_unix = now_unix() - 1;
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![issued.record],
                ..ClientCredentials::default()
            },
        )
        .unwrap();
        let session_state = Arc::new(Mutex::new(RpcSessionState::default()));

        let responses = handle_reqres_messages(
            vec![ReqResMessage::SessionAuthenticate {
                mechanism: PAIRED_SECRET_AUTH_MECHANISM.to_string(),
                payload: PairedSecretCredential {
                    credential_id: issued.client_id,
                    credential_secret: issued.client_secret,
                }
                .encode(),
            }],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(load_or_default(&config_path).unwrap())),
            Arc::new(Mutex::new(
                load_client_credentials_or_default(&credentials_path).unwrap(),
            )),
            test_pairing_challenge(),
            session_state.clone(),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            ReqResMessage::SessionAuthError {
                code: SessionAuthErrorCode::InvalidCredentials,
                ..
            }
        ));
        assert_eq!(session_state.lock().await.authenticated_client_id, None);
    }

    #[tokio::test]
    async fn renew_client_credential_extends_authenticated_client_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let mut issued = issue_client_secret("test-browser", now_unix() - 10);
        let client_id = issued.client_id.clone();
        issued.record.expires_at_unix = now_unix() + 10;
        let previous_expires_at_unix = issued.record.expires_at_unix;
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![issued.record],
                ..ClientCredentials::default()
            },
        )
        .unwrap();
        let session_state = Arc::new(Mutex::new(RpcSessionState {
            session_id: 0,
            authenticated_client_id: Some(client_id.clone()),
        }));

        let responses = handle_rpc_messages(
            vec![request_message(ProcId::RenewClientCredential, None)],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            session_state,
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            ReqResMessage::ResponseUnaryOk { .. }
        ));
        let renewal = RenewClientCredentialResponse::decode(payload(&responses[0])).unwrap();
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        let record = stored
            .clients
            .iter()
            .find(|record| record.client_id == client_id)
            .unwrap();
        assert!(renewal.client_credential_expires_at_unix > previous_expires_at_unix);
        assert_eq!(
            renewal.client_credential_expires_at_unix,
            record.expires_at_unix
        );
        assert!(
            renewal.client_credential_expires_at_unix
                >= now_unix() + CLIENT_CREDENTIAL_TTL_SECONDS - 1
        );
    }

    #[tokio::test]
    async fn remove_client_deletes_persisted_credential() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("rieul.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let current = issue_client_secret("current", now_unix());
        let removed = issue_client_secret("removed", now_unix());
        let current_client_id = current.client_id.clone();
        let removed_client_id = removed.client_id.clone();
        save(&config_path, &SystemConfig::default()).unwrap();
        save_client_credentials(
            &credentials_path,
            &ClientCredentials {
                clients: vec![current.record, removed.record],
                ..ClientCredentials::default()
            },
        )
        .unwrap();

        let responses = handle_rpc_messages(
            vec![request_message(
                ProcId::RemoveClient,
                Some(
                    RemoveClientReq {
                        client_id: removed_client_id.clone(),
                    }
                    .encode(),
                ),
            )],
            &config_path,
            &credentials_path,
            Arc::new(Mutex::new(SystemConfig::default())),
            Arc::new(Mutex::new(ClientCredentials::default())),
            test_pairing_challenge(),
            Arc::new(Mutex::new(RpcSessionState {
                session_id: 0,
                authenticated_client_id: Some(current_client_id.clone()),
            })),
            test_files(),
            test_terminals(),
            None,
        )
        .await
        .unwrap();

        assert!(matches!(
            responses.as_slice(),
            [ReqResMessage::ResponseUnaryOk { .. }]
        ));
        let stored = load_client_credentials_or_default(&credentials_path).unwrap();
        assert_eq!(stored.clients.len(), 1);
        assert_eq!(stored.clients[0].client_id, current_client_id);
        assert!(!stored
            .clients
            .iter()
            .any(|record| record.client_id == removed_client_id));
    }

    #[test]
    fn kill_host_process_rejects_the_daemon_process() {
        assert_eq!(
            kill_host_process(u64::from(std::process::id())),
            Err(KillHostProcessError::PermissionDenied)
        );
    }

    #[test]
    fn kill_host_process_reports_an_invalid_pid_as_missing() {
        assert_eq!(
            kill_host_process(u64::MAX),
            Err(KillHostProcessError::NotFound)
        );
    }

    #[test]
    fn kill_host_process_terminates_a_child_process() {
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();

        assert_eq!(kill_host_process(u64::from(child.id())), Ok(()));
        for _ in 0..40 {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        panic!("child process did not exit after kill request");
    }

    fn request_message(proc_id: ProcId, payload: Option<Vec<u8>>) -> ReqResMessage {
        ReqResMessage::RequestUnary {
            proc_id: proc_id.as_u64(),
            payload,
        }
    }

    fn start_pairing_request_message(confirmation_code: &str) -> ReqResMessage {
        request_message(
            ProcId::StartPairing,
            Some(
                StartPairingRequest {
                    confirmation_code: confirmation_code.to_string(),
                    client_label: "test-browser".to_string(),
                    client_id: Some("existing-client".to_string()),
                }
                .encode(),
            ),
        )
    }

    fn test_pairing_challenge() -> SharedPairingChallenge {
        Arc::new(Mutex::new(PairingState::default()))
    }

    fn test_pairing_challenge_with_label(
        record: PairingRecord,
        client_label: &str,
        client_id: Option<String>,
    ) -> SharedPairingChallenge {
        Arc::new(Mutex::new(PairingState {
            next_attempt_id: 1,
            current_attempts: HashMap::new(),
            active_challenge: Some(ActivePairingChallenge {
                attempt_id: 1,
                attempt_key: pairing_attempt_key(client_id.as_deref()),
                owner_session_id: 0,
                record,
                client_label: client_label.to_string(),
                client_id,
            }),
        }))
    }

    fn test_pairing_challenge_with_session(
        owner_session_id: RpcSessionId,
        record: PairingRecord,
    ) -> SharedPairingChallenge {
        Arc::new(Mutex::new(PairingState {
            next_attempt_id: 1,
            current_attempts: HashMap::new(),
            active_challenge: Some(ActivePairingChallenge {
                attempt_id: 1,
                attempt_key: PairingAttemptKey::Anonymous,
                owner_session_id,
                record,
                client_label: "browser".to_string(),
                client_id: None,
            }),
        }))
    }

    async fn active_pairing_challenge(
        pairing_challenge: &SharedPairingChallenge,
    ) -> Option<ActivePairingChallenge> {
        pairing_challenge.lock().await.active_challenge.clone()
    }

    fn test_session_state(session_id: RpcSessionId) -> SharedRpcSessionState {
        Arc::new(Mutex::new(RpcSessionState {
            session_id,
            authenticated_client_id: None,
        }))
    }

    #[derive(Clone, Default)]
    struct RecordingPairingNotifier {
        confirmations: Arc<std::sync::Mutex<Vec<PairingConfirmationRequest>>>,
        notifications: Arc<std::sync::Mutex<Vec<PairingCodeNotification>>>,
        completed: Arc<std::sync::Mutex<usize>>,
    }

    impl PairingNotifier for RecordingPairingNotifier {
        fn confirm_pairing_request(
            &self,
            request: PairingConfirmationRequest,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let confirmations = self.confirmations.clone();
            Box::pin(async move {
                confirmations.lock().unwrap().push(request);
                Ok(())
            })
        }

        fn notify_pairing_code(
            &self,
            notification: PairingCodeNotification,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let notifications = self.notifications.clone();
            Box::pin(async move {
                notifications.lock().unwrap().push(notification);
                Ok(())
            })
        }

        fn notify_pairing_completed(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let completed = self.completed.clone();
            Box::pin(async move {
                *completed.lock().unwrap() += 1;
                Ok(())
            })
        }
    }

    #[derive(Clone)]
    struct BlockingFirstPairingNotifier {
        confirmations: Arc<std::sync::Mutex<Vec<PairingConfirmationRequest>>>,
        notifications: Arc<std::sync::Mutex<Vec<PairingCodeNotification>>>,
        first_started: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
        first_release: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
    }

    impl BlockingFirstPairingNotifier {
        fn new() -> (
            Self,
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ) {
            let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
            let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
            (
                Self {
                    confirmations: Arc::new(std::sync::Mutex::new(Vec::new())),
                    notifications: Arc::new(std::sync::Mutex::new(Vec::new())),
                    first_started: Arc::new(std::sync::Mutex::new(Some(started_sender))),
                    first_release: Arc::new(Mutex::new(Some(release_receiver))),
                },
                started_receiver,
                release_sender,
            )
        }
    }

    impl PairingNotifier for BlockingFirstPairingNotifier {
        fn confirm_pairing_request(
            &self,
            request: PairingConfirmationRequest,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let confirmations = self.confirmations.clone();
            let first_started = self.first_started.clone();
            let first_release = self.first_release.clone();
            Box::pin(async move {
                confirmations.lock().unwrap().push(request);
                let release = first_release.lock().await.take();
                if let Some(release) = release {
                    if let Some(started) = first_started.lock().unwrap().take() {
                        let _ = started.send(());
                    }
                    let _ = release.await;
                }
                Ok(())
            })
        }

        fn notify_pairing_code(
            &self,
            notification: PairingCodeNotification,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let notifications = self.notifications.clone();
            Box::pin(async move {
                notifications.lock().unwrap().push(notification);
                Ok(())
            })
        }
    }

    fn payload(message: &ReqResMessage) -> &[u8] {
        message.payload().unwrap()
    }

    fn error(message: &ReqResMessage) -> &[u8] {
        message.error().unwrap()
    }

    fn fs_entry(name: &str, path: &str, kind: FsEntryKind, size: Option<u64>) -> FsEntry {
        FsEntry {
            name: name.to_string(),
            path: path.to_string(),
            kind,
            size,
            modified_at_ms: None,
            readonly: false,
        }
    }

    fn test_files() -> SharedFileService {
        Arc::new(TestFileService)
    }

    fn test_terminals() -> SharedTerminalManager {
        Arc::new(TerminalManager::new(
            std::env::temp_dir().join("Rieul-test-shell-integration"),
        ))
    }

    #[derive(Debug)]
    struct TestFileService;

    impl FileService for TestFileService {
        fn roots(&self) -> rieul_daemon_core::traits::BoxFutureResult<'_, Vec<FsEntry>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn list_directory(
            &self,
            _path: String,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, Vec<FsEntry>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn read_file(
            &self,
            _request: ReadFileReq,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, Vec<u8>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn write_file<'a>(
            &'a self,
            _start: WriteFileStart,
            _chunks: Box<dyn WriteFileChunkSource + 'a>,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'a, WriteFileResult> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn create_node(
            &self,
            _op: CreateNodeOp,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn rename_path(
            &self,
            _from: String,
            _to: String,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn delete_path(
            &self,
            _path: String,
            _mode: DeleteMode,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn trash_items(
            &self,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, Vec<rieul_daemon_core::rpc::TrashItem>>
        {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn restore_trash_item(
            &self,
            _item_id: String,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn purge_trash_item(
            &self,
            _item_id: String,
        ) -> rieul_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }
    }
}
