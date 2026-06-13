use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use notify::{Event, RecursiveMode, Watcher};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tracing::{info, warn};
use web_transport_quinn::proto::ConnectResponse;
use wgo_daemon_core::cbor::Value;
use wgo_daemon_core::config::{
    client_credentials_path, daemon_status_path, load_client_credentials_or_default,
    load_or_default, load_or_generated_default, save, save_client_credentials, ClientCredentials,
    SystemConfig,
};
use wgo_daemon_core::pairing::{
    create_pairing_code, issue_client_secret, reissue_client_secret, renew_client_credential,
    verify_client_credential, verify_pairing_code, PairingRecord,
};
use wgo_daemon_core::rpc::{
    BulkMutationItemResult, BulkMutationRes, CompletePairingRequest, CompletePairingResponse,
    CreateNodesReq, DaemonInfo, DeletePathsReq, DirectoryEntryKey,
    DirectorySubscriptionCloseReason, DirectoryTableEvent, FsEntry, ProcId, ReadFileChunk,
    ReadFileReq, RenamePathsReq, RenewClientCredentialResponse, RootEntryKey,
    RootsSubscriptionCloseReason, RootsTableEvent, RpcErrorCode, RpcErrorPayload,
    StartPairingRequest, StartPairingResponse, WriteFileReq,
};
use wgo_daemon_core::traits::{FileService, ServiceError};
use wgo_daemon_core::wire::{
    DatagramMessage, PairedSecretCredential, ReqResMessage, RpcErrorKind, SessionAuthErrorCode,
    MAX_MESSAGE_SEQUENCE_SIZE, PAIRED_SECRET_AUTH_MECHANISM,
};

use crate::cert::{
    configured_certificate_paths, prepare_server_certificate, uses_scheduled_certificate_refresh,
};

const CERT_RELOAD_DEBOUNCE: Duration = Duration::from_millis(250);
const CONFIG_STARTUP_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const SCHEDULED_CERT_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SUBSCRIPTION_DEBOUNCE: Duration = Duration::from_millis(150);
const ROOTS_SUBSCRIPTION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const READ_FILE_CHUNK_SIZE: usize = 64 * 1024;

type SharedSystemConfig = Arc<Mutex<SystemConfig>>;
type SharedClientCredentials = Arc<Mutex<ClientCredentials>>;
type RpcSessionId = u64;
type PairingAttemptId = u64;
type SharedPairingChallenge = Arc<Mutex<PairingState>>;
type SharedRpcSessionState = Arc<Mutex<RpcSessionState>>;
type SharedFileService = Arc<dyn FileService>;
type SharedPairingNotifier = Arc<dyn PairingNotifier>;

static NEXT_RPC_SESSION_ID: AtomicU64 = AtomicU64::new(1);

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
}

#[derive(Default)]
struct RpcSessionState {
    session_id: RpcSessionId,
    authenticated_client_id: Option<String>,
}

pub async fn run_system_server(
    addr: SocketAddr,
    config_path: PathBuf,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
    log_label: &'static str,
) -> Result<()> {
    loop {
        write_daemon_status(&config_path, DaemonStatus::NotReady("starting"));
        match run_system_server_once(
            addr,
            config_path.clone(),
            files.clone(),
            pairing_notifier.clone(),
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
    addr: SocketAddr,
    config_path: PathBuf,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
    log_label: &'static str,
) -> Result<()> {
    let provider = web_transport_quinn::crypto::default_provider();
    let mut config = load_startup_config(&config_path, addr)?;
    let certificate = prepare_server_certificate(&mut config, addr, &config_path, &provider)?;
    save(&config_path, &config)?;
    let config_state = Arc::new(Mutex::new(config));
    let credentials_path = client_credentials_path(&config_path);
    let client_credentials = Arc::new(Mutex::new(load_client_credentials_or_default(
        &credentials_path,
    )?));
    let pairing_challenge = Arc::new(Mutex::new(PairingState::default()));

    let resolver = Arc::new(ReloadingCertResolver::new(certificate.certified_key));
    let mut server = build_reloadable_server(addr, provider.clone(), resolver.clone())?;
    write_daemon_status(&config_path, DaemonStatus::Ready);

    tokio::spawn(reload_certificates(
        config_path.clone(),
        addr,
        provider,
        resolver,
    ));

    info!(%addr, daemon = log_label, "wgo system daemon listening");

    while let Some(request) = server.accept().await {
        let config_path = config_path.clone();
        let credentials_path = credentials_path.clone();
        let config_state = config_state.clone();
        let client_credentials = client_credentials.clone();
        let pairing_challenge = pairing_challenge.clone();
        let files = files.clone();
        let pairing_notifier = pairing_notifier.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_request(
                request,
                config_path,
                credentials_path,
                config_state,
                client_credentials,
                pairing_challenge,
                files,
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

fn load_startup_config(config_path: &Path, addr: SocketAddr) -> Result<SystemConfig> {
    let should_create = !config_path.exists();
    let mut config = load_or_generated_default(config_path)?;
    config.listen_addr = addr.to_string();
    if should_create {
        save(config_path, &config)?;
    }
    Ok(config)
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
    let server_config =
        web_transport_quinn::quinn::ServerConfig::with_crypto(Arc::new(quic_config));
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
            config.listen_addr = addr.to_string();
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
            config.listen_addr = addr.to_string();
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
    pairing_challenge: SharedPairingChallenge,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    let path = request.url.path().to_string();
    match path.as_str() {
        "/rpc" => {
            let session = request.respond(ConnectResponse::OK).await?;
            run_rpc_session(
                session,
                config_path,
                credentials_path,
                config_state,
                client_credentials,
                pairing_challenge,
                files,
                pairing_notifier,
            )
            .await
        }
        "/moqt" => {
            let session = request.respond(ConnectResponse::OK).await?;
            info!("accepted reserved /moqt session");
            session.close(0, b"moqt route reserved");
            Ok(())
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
    pairing_challenge: SharedPairingChallenge,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    let session_state = Arc::new(Mutex::new(RpcSessionState {
        session_id: next_rpc_session_id(),
        authenticated_client_id: None,
    }));
    loop {
        tokio::select! {
            stream = session.accept_bi() => {
                let (mut send, mut recv) = stream?;
                let config_path = config_path.clone();
                let credentials_path = credentials_path.clone();
                let config_state = config_state.clone();
                let client_credentials = client_credentials.clone();
                let pairing_challenge = pairing_challenge.clone();
                let session_state = session_state.clone();
                let files = files.clone();
                let pairing_notifier = pairing_notifier.clone();
                tokio::spawn(async move {
                    let response = async {
                        let messages = read_reqres_message_sequence_from_stream(&mut recv)
                            .await
                            .context("invalid reqres message sequence")?;
                        handle_reqres_stream(
                            messages,
                            &mut send,
                            &config_path,
                            &credentials_path,
                            config_state,
                            client_credentials,
                            pairing_challenge,
                            session_state,
                            files,
                            pairing_notifier,
                        )
                        .await?;
                        send.finish()?;
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

fn is_subscription_proc(proc_id: u64) -> bool {
    proc_id == ProcId::SubscribeRoots.as_u64() || proc_id == ProcId::SubscribeDirectory.as_u64()
}

fn is_server_stream_proc(proc_id: u64) -> bool {
    is_subscription_proc(proc_id) || proc_id == ProcId::ReadFile.as_u64()
}

fn is_client_stream_proc(proc_id: u64) -> bool {
    proc_id == ProcId::WriteFile.as_u64()
}

async fn read_reqres_message_sequence_from_stream(
    recv: &mut web_transport_quinn::RecvStream,
) -> Result<Vec<ReqResMessage>> {
    let bytes = recv.read_to_end(MAX_MESSAGE_SEQUENCE_SIZE).await?;
    ReqResMessage::decode_sequence(&bytes).map_err(Into::into)
}

async fn handle_reqres_messages(
    messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    let Some(first) = messages.first() else {
        return Ok(vec![generic_error_message(
            0,
            RpcErrorCode::BadMessage,
            "reqres message sequence is empty",
        )]);
    };
    if first.is_session_control() {
        handle_session_control_messages(messages, client_credentials, session_state).await
    } else {
        handle_rpc_messages(
            messages,
            config_path,
            credentials_path,
            config_state,
            client_credentials,
            pairing_challenge,
            session_state,
            files,
            pairing_notifier,
        )
        .await
    }
}

async fn handle_reqres_stream(
    messages: Vec<ReqResMessage>,
    send: &mut web_transport_quinn::SendStream,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<()> {
    if let Some((proc_id, payload)) = request_unary_parts(&messages) {
        if is_server_stream_proc(proc_id) {
            return handle_server_stream(
                proc_id,
                payload,
                send,
                config_state,
                session_state,
                files,
            )
            .await;
        }
    }

    if matches!(
        messages.first(),
        Some(ReqResMessage::RequestStreamStart { .. })
    ) {
        let responses = handle_client_stream_messages(messages, session_state, files).await?;
        return write_reqres_messages(send, &responses).await;
    }

    let responses = handle_reqres_messages(
        messages,
        config_path,
        credentials_path,
        config_state,
        client_credentials,
        pairing_challenge,
        session_state,
        files,
        pairing_notifier,
    )
    .await?;
    write_reqres_messages(send, &responses).await
}

fn request_unary_parts(messages: &[ReqResMessage]) -> Option<(u64, Option<Vec<u8>>)> {
    if messages.len() != 1 {
        return None;
    }
    match &messages[0] {
        ReqResMessage::RequestUnary { proc_id, payload } => Some((*proc_id, payload.clone())),
        _ => None,
    }
}

async fn handle_server_stream(
    proc_id: u64,
    payload: Option<Vec<u8>>,
    send: &mut web_transport_quinn::SendStream,
    _config_state: SharedSystemConfig,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
) -> Result<()> {
    if requires_authentication(proc_id) && !is_authenticated(&session_state).await {
        write_reqres_message(
            send,
            stream_generic_error_message(
                proc_id,
                RpcErrorCode::Unauthorized,
                "valid paired client credentials are required",
            ),
        )
        .await?;
        return Ok(());
    }

    if proc_id == ProcId::ReadFile.as_u64() {
        return stream_read_file(send, files, proc_id, payload).await;
    }

    if proc_id == ProcId::SubscribeRoots.as_u64() {
        return stream_roots_subscription(send, files.clone(), proc_id).await;
    }

    let Some(payload) = payload else {
        write_reqres_message(
            send,
            stream_generic_error_message(
                proc_id,
                RpcErrorCode::MissingPayload,
                "SubscribeDirectory requires a payload",
            ),
        )
        .await?;
        return Ok(());
    };
    let request = match wgo_daemon_core::rpc::SubscribeDirectoryReq::decode(&payload) {
        Ok(request) => request,
        Err(_) => {
            write_reqres_message(
                send,
                stream_generic_error_message(
                    proc_id,
                    RpcErrorCode::MalformedPayload,
                    "SubscribeDirectory payload is malformed",
                ),
            )
            .await?;
            return Ok(());
        }
    };
    stream_directory_subscription(send, files, proc_id, request.path).await
}

async fn handle_client_stream_messages(
    mut messages: Vec<ReqResMessage>,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
) -> Result<Vec<ReqResMessage>> {
    let (proc_id, payload) = match messages.remove(0) {
        ReqResMessage::RequestStreamStart { proc_id, payload } => (proc_id, payload),
        _ => unreachable!("caller checks the first message"),
    };

    if !is_client_stream_proc(proc_id) {
        return Ok(vec![generic_error_message(
            proc_id,
            RpcErrorCode::BadMessage,
            "this RPC does not accept a request stream",
        )]);
    }
    if requires_authentication(proc_id) && !is_authenticated(&session_state).await {
        return Ok(vec![unauthorized_message(proc_id)]);
    }
    if proc_id == ProcId::WriteFile.as_u64() {
        return handle_write_file_stream(proc_id, payload, messages, files).await;
    }
    Ok(vec![generic_error_message(
        proc_id,
        RpcErrorCode::NotImplemented,
        "client-streaming RPC is not implemented",
    )])
}

async fn handle_write_file_stream(
    proc_id: u64,
    payload: Option<Vec<u8>>,
    messages: Vec<ReqResMessage>,
    files: SharedFileService,
) -> Result<Vec<ReqResMessage>> {
    let Some(payload) = payload else {
        return Ok(vec![generic_error_message(
            proc_id,
            RpcErrorCode::MissingPayload,
            "WriteFile requires a WriteFileStart payload",
        )]);
    };
    let start = match WriteFileReq::decode(&payload) {
        Ok(WriteFileReq::Start(start)) => start,
        Ok(WriteFileReq::Chunk(_)) | Err(_) => {
            return Ok(vec![generic_error_message(
                proc_id,
                RpcErrorCode::MalformedPayload,
                "WriteFile first payload must be WriteFileStart",
            )]);
        }
    };

    let mut chunks = Vec::new();
    for message in messages {
        let ReqResMessage::RequestStreamChunk { payload } = message else {
            return Ok(vec![generic_error_message(
                proc_id,
                RpcErrorCode::BadMessage,
                "WriteFile request stream may contain only RequestStreamChunk after start",
            )]);
        };
        match WriteFileReq::decode(&payload) {
            Ok(WriteFileReq::Chunk(chunk)) => chunks.push(chunk),
            Ok(WriteFileReq::Start(_)) | Err(_) => {
                return Ok(vec![generic_error_message(
                    proc_id,
                    RpcErrorCode::MalformedPayload,
                    "WriteFile chunk payload must be WriteFileChunk",
                )]);
            }
        }
    }

    let result = match files.write_file(start, chunks).await {
        Ok(result) => result,
        Err(err) => return Ok(vec![service_error_message(proc_id, err)]),
    };
    Ok(vec![ok_payload_message(proc_id, result.encode())])
}

async fn stream_read_file(
    send: &mut web_transport_quinn::SendStream,
    files: SharedFileService,
    proc_id: u64,
    payload: Option<Vec<u8>>,
) -> Result<()> {
    let Some(payload) = payload else {
        write_reqres_message(
            send,
            stream_generic_error_message(
                proc_id,
                RpcErrorCode::MissingPayload,
                "ReadFile requires a payload",
            ),
        )
        .await?;
        return Ok(());
    };
    let request = match ReadFileReq::decode(&payload) {
        Ok(request) => request,
        Err(_) => {
            write_reqres_message(
                send,
                stream_generic_error_message(
                    proc_id,
                    RpcErrorCode::MalformedPayload,
                    "ReadFile payload is malformed",
                ),
            )
            .await?;
            return Ok(());
        }
    };
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

async fn handle_rpc_messages(
    mut messages: Vec<ReqResMessage>,
    config_path: &Path,
    credentials_path: &Path,
    config_state: SharedSystemConfig,
    client_credentials: SharedClientCredentials,
    pairing_challenge: SharedPairingChallenge,
    session_state: SharedRpcSessionState,
    files: SharedFileService,
    pairing_notifier: Option<SharedPairingNotifier>,
) -> Result<Vec<ReqResMessage>> {
    if messages.len() != 1 {
        return Ok(vec![generic_error_message(
            0,
            RpcErrorCode::BadMessage,
            "RPC handler expects exactly one request message",
        )]);
    }
    let (proc_id, payload) = match messages.remove(0) {
        ReqResMessage::RequestUnary { proc_id, payload } => (proc_id, payload),
        _ => {
            return Ok(vec![generic_error_message(
                0,
                RpcErrorCode::BadMessage,
                "RPC handler expects RequestUnary",
            )]);
        }
    };
    let payload = payload.as_deref();
    if is_server_stream_proc(proc_id) {
        return Ok(vec![stream_generic_error_message(
            proc_id,
            RpcErrorCode::BadMessage,
            "server-streaming RPCs must be handled by the reqres stream handler",
        )]);
    }
    if requires_authentication(proc_id) && !is_authenticated(&session_state).await {
        return Ok(vec![unauthorized_message(proc_id)]);
    }

    let response = match proc_id {
        id if id == ProcId::GetDaemonInfo.as_u64() => {
            ok_payload_message(proc_id, DaemonInfo::current().encode())
        }
        id if id == ProcId::StartPairing.as_u64() => {
            let Some(payload) = payload else {
                return Ok(vec![error_message(
                    proc_id,
                    "failed",
                    "StartPairing requires a payload",
                )]);
            };
            let request = match StartPairingRequest::decode(payload) {
                Ok(request) => request,
                Err(_) => {
                    return Ok(vec![generic_error_message(
                        proc_id,
                        RpcErrorCode::MalformedPayload,
                        "StartPairing payload is malformed",
                    )]);
                }
            };
            let Some(confirmation_code) = normalize_confirmation_code(&request.confirmation_code)
            else {
                return Ok(vec![generic_error_message(
                    proc_id,
                    RpcErrorCode::MalformedPayload,
                    "StartPairing confirmationCode must be two ASCII digits",
                )]);
            };
            let client_label = pairing_client_label(&request.client_label);
            let requested_client_id = normalize_pairing_client_id(request.client_id.as_deref());
            let Some(notifier) = pairing_notifier.as_ref() else {
                return Ok(vec![error_message(
                    proc_id,
                    "failed",
                    "daemon failed to start pairing",
                )]);
            };
            let current_session_id = rpc_session_id(&session_state).await;
            let attempt_key = pairing_attempt_key(requested_client_id.as_deref());
            let attempt_id = begin_pairing_attempt(&pairing_challenge, attempt_key.clone()).await;
            let config = load_runtime_config(config_path, &config_state).await?;
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
                let mut state = pairing_challenge.lock().await;
                if state
                    .current_attempts
                    .get(&attempt_key)
                    .is_some_and(|current| *current == attempt_id)
                {
                    state.current_attempts.remove(&attempt_key);
                }
                return Ok(vec![error_message(
                    proc_id,
                    "failed",
                    "daemon failed to confirm pairing",
                )]);
            }
            if !is_current_pairing_attempt(&pairing_challenge, &attempt_key, attempt_id).await {
                return Ok(vec![error_message(
                    proc_id,
                    "failed",
                    "pairing request was superseded",
                )]);
            }

            let now = now_unix();
            let pairing = create_pairing_code(now);
            let pairing_code_expires_at_unix = pairing.record.expires_at_unix;
            {
                let mut state = pairing_challenge.lock().await;
                if !state
                    .current_attempts
                    .get(&attempt_key)
                    .is_some_and(|current| *current == attempt_id)
                {
                    return Ok(vec![error_message(
                        proc_id,
                        "failed",
                        "pairing request was superseded",
                    )]);
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
                let mut state = pairing_challenge.lock().await;
                if state.active_challenge.as_ref().is_some_and(|challenge| {
                    challenge.attempt_key == attempt_key && challenge.attempt_id == attempt_id
                }) {
                    state.active_challenge = None;
                    state.current_attempts.remove(&attempt_key);
                }
                return Ok(vec![error_message(
                    proc_id,
                    "failed",
                    "daemon failed to start pairing",
                )]);
            }
            ok_payload_message(
                proc_id,
                StartPairingResponse {
                    pairing_code_expires_at_unix,
                }
                .encode(),
            )
        }
        id if id == ProcId::CompletePairing.as_u64() => {
            let Some(payload) = payload else {
                return Ok(vec![error_message(
                    proc_id,
                    "missing_payload",
                    "CompletePairing requires a payload",
                )]);
            };
            let request = match CompletePairingRequest::decode(payload) {
                Ok(request) => request,
                Err(_) => {
                    return Ok(vec![generic_error_message(
                        proc_id,
                        RpcErrorCode::MalformedPayload,
                        "CompletePairing payload is malformed",
                    )]);
                }
            };
            let now = now_unix();
            let current_session_id = rpc_session_id(&session_state).await;
            let pairing = {
                let mut state = pairing_challenge.lock().await;
                let Some(pairing) = state.active_challenge.as_ref() else {
                    return Ok(vec![error_message(
                        proc_id,
                        "pairing_not_started",
                        "create a local pairing code before completing pairing",
                    )]);
                };
                if pairing.owner_session_id != current_session_id {
                    return Ok(vec![error_message(
                        proc_id,
                        "pairing_not_started",
                        "start pairing on this session before completing pairing",
                    )]);
                }
                if now >= pairing.record.expires_at_unix {
                    let attempt_key = pairing.attempt_key.clone();
                    state.active_challenge = None;
                    state.current_attempts.remove(&attempt_key);
                    return Ok(vec![error_message(
                        proc_id,
                        "pairing_expired",
                        "pairing code expired",
                    )]);
                }
                if !verify_pairing_code(&pairing.record, request.code.trim(), now) {
                    return Ok(vec![error_message(
                        proc_id,
                        "invalid_pairing_code",
                        "pairing code is invalid",
                    )]);
                }
                let pairing = state
                    .active_challenge
                    .take()
                    .expect("active pairing challenge exists after validation");
                state.current_attempts.remove(&pairing.attempt_key);
                pairing
            };

            let mut state =
                load_runtime_client_credentials(credentials_path, &client_credentials).await?;
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
            store_runtime_client_credentials(credentials_path, &client_credentials, state).await?;

            ok_payload_message(
                proc_id,
                CompletePairingResponse {
                    client_id,
                    client_secret: issued.client_secret,
                    client_credential_expires_at_unix,
                }
                .encode(),
            )
        }
        id if id == ProcId::RenewClientCredential.as_u64() => {
            let Some(client_id) = authenticated_client_id(&session_state).await else {
                return Ok(vec![unauthorized_message(proc_id)]);
            };
            let now = now_unix();
            let mut state =
                load_runtime_client_credentials(credentials_path, &client_credentials).await?;
            let Some(record) = state
                .clients
                .iter_mut()
                .find(|record| record.client_id == client_id)
            else {
                return Ok(vec![unauthorized_message(proc_id)]);
            };
            renew_client_credential(record, now);
            let client_credential_expires_at_unix = record.expires_at_unix;
            store_runtime_client_credentials(credentials_path, &client_credentials, state).await?;
            ok_payload_message(
                proc_id,
                RenewClientCredentialResponse {
                    client_credential_expires_at_unix,
                }
                .encode(),
            )
        }
        id if id == ProcId::CreateNodes.as_u64() => {
            let Some(payload) = payload else {
                return Ok(vec![error_message(
                    proc_id,
                    "missing_payload",
                    "CreateNodes requires a payload",
                )]);
            };
            let request = match CreateNodesReq::decode(payload) {
                Ok(request) => request,
                Err(_) => {
                    return Ok(vec![generic_error_message(
                        proc_id,
                        RpcErrorCode::MalformedPayload,
                        "CreateNodes payload is malformed",
                    )]);
                }
            };
            ok_payload_message(
                proc_id,
                create_nodes(files.as_ref(), request).await.encode(),
            )
        }
        id if id == ProcId::RenamePaths.as_u64() => {
            let Some(payload) = payload else {
                return Ok(vec![error_message(
                    proc_id,
                    "missing_payload",
                    "RenamePaths requires a payload",
                )]);
            };
            let request = match RenamePathsReq::decode(payload) {
                Ok(request) => request,
                Err(_) => {
                    return Ok(vec![generic_error_message(
                        proc_id,
                        RpcErrorCode::MalformedPayload,
                        "RenamePaths payload is malformed",
                    )]);
                }
            };
            ok_payload_message(
                proc_id,
                rename_paths(files.as_ref(), request).await.encode(),
            )
        }
        id if id == ProcId::DeletePaths.as_u64() => {
            let Some(payload) = payload else {
                return Ok(vec![error_message(
                    proc_id,
                    "missing_payload",
                    "DeletePaths requires a payload",
                )]);
            };
            let request = match DeletePathsReq::decode(payload) {
                Ok(request) => request,
                Err(_) => {
                    return Ok(vec![generic_error_message(
                        proc_id,
                        RpcErrorCode::MalformedPayload,
                        "DeletePaths payload is malformed",
                    )]);
                }
            };
            ok_payload_message(
                proc_id,
                delete_paths(files.as_ref(), request).await.encode(),
            )
        }
        _ => error_message(
            proc_id,
            "not_implemented",
            "this RPC is reserved but not implemented in the first cut",
        ),
    };
    Ok(vec![response])
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

async fn store_runtime_client_credentials(
    credentials_path: &Path,
    client_credentials: &SharedClientCredentials,
    state: ClientCredentials,
) -> Result<()> {
    save_client_credentials(credentials_path, &state)?;
    *client_credentials.lock().await = state;
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
    let variant_id = method_error_variant(proc_id, code)?;
    Some(
        Value::Array(vec![
            Value::U64(variant_id),
            Value::Map(std::collections::BTreeMap::from([(
                1,
                Value::Text(message.to_string()),
            )])),
        ])
        .encode(),
    )
}

fn method_error_variant(proc_id: u64, code: &str) -> Option<u64> {
    match proc_id {
        id if id == ProcId::GetDaemonInfo.as_u64() => match code {
            "failed" => Some(0),
            _ => None,
        },
        id if id == ProcId::StartPairing.as_u64() => match code {
            "failed" => Some(0),
            _ => None,
        },
        id if id == ProcId::CompletePairing.as_u64() => match code {
            "pairing_not_started" => Some(1),
            "pairing_expired" => Some(2),
            "invalid_pairing_code" => Some(3),
            _ => None,
        },
        id if id == ProcId::RenewClientCredential.as_u64() => match code {
            "failed" => Some(0),
            _ => None,
        },
        id if id == ProcId::SubscribeRoots.as_u64() => match code {
            "failed" => Some(0),
            _ => None,
        },
        id if id == ProcId::SubscribeDirectory.as_u64() => match code {
            "failed" => Some(0),
            "permission_denied" => Some(1),
            "not_found" => Some(2),
            "not_directory" => Some(3),
            _ => None,
        },
        id if id == ProcId::ReadFile.as_u64() => match code {
            "failed" => Some(0),
            "permission_denied" => Some(1),
            "not_found" => Some(2),
            "not_file" => Some(3),
            "invalid_path" => Some(4),
            _ => None,
        },
        id if id == ProcId::WriteFile.as_u64() => match code {
            "failed" => Some(0),
            "permission_denied" => Some(1),
            "not_found" => Some(2),
            "already_exists" => Some(3),
            "not_directory" => Some(4),
            "not_file" => Some(5),
            "invalid_path" => Some(6),
            _ => None,
        },
        id if id == ProcId::CreateNodes.as_u64()
            || id == ProcId::RenamePaths.as_u64()
            || id == ProcId::DeletePaths.as_u64() =>
        {
            match code {
                "failed" => Some(0),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgo_daemon_core::pairing::{
        create_pairing_code, issue_client_secret, verify_client_secret,
        CLIENT_CREDENTIAL_TTL_SECONDS, PAIRING_TTL_SECONDS,
    };
    use wgo_daemon_core::rpc::{
        CreateNodeOp, DeleteMode, FsEntryKind, ReadFileReq, WriteFileChunk, WriteFileResult,
        WriteFileStart,
    };

    #[test]
    fn creates_startup_config_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nested").join("wgo.yaml");
        let addr: SocketAddr = "127.0.0.1:9012".parse().unwrap();

        let config = load_startup_config(&config_path, addr).unwrap();

        assert_eq!(config.listen_addr, "127.0.0.1:9012");
        assert_eq!(load_or_default(&config_path).unwrap(), config);
    }

    #[tokio::test]
    async fn complete_pairing_reads_config_written_after_server_start() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wgo.yaml");
        let credentials_path = client_credentials_path(&config_path);
        let pairing = create_pairing_code(now_unix());
        save(&config_path, &SystemConfig::default()).unwrap();

        let config_state = Arc::new(Mutex::new(SystemConfig::default()));
        let client_credentials = Arc::new(Mutex::new(ClientCredentials::default()));
        let pairing_challenge =
            test_pairing_challenge_with_label(pairing.record.clone(), "test-browser", None);
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
    }

    #[tokio::test]
    async fn complete_pairing_reuses_requested_existing_client_id() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
    async fn start_pairing_notifies_local_pairing_ui() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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
        let config_path = dir.path().join("wgo.yaml");
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

    #[derive(Debug)]
    struct TestFileService;

    impl FileService for TestFileService {
        fn roots(&self) -> wgo_daemon_core::traits::BoxFutureResult<'_, Vec<FsEntry>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn list_directory(
            &self,
            _path: String,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, Vec<FsEntry>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn read_file(
            &self,
            _request: ReadFileReq,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, Vec<u8>> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn write_file(
            &self,
            _start: WriteFileStart,
            _chunks: Vec<WriteFileChunk>,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, WriteFileResult> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn create_node(
            &self,
            _op: CreateNodeOp,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn rename_path(
            &self,
            _from: String,
            _to: String,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }

        fn delete_path(
            &self,
            _path: String,
            _mode: DeleteMode,
        ) -> wgo_daemon_core::traits::BoxFutureResult<'_, ()> {
            Box::pin(async { Err(ServiceError::Unsupported) })
        }
    }
}
