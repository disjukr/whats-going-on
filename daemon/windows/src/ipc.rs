use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
#[cfg(windows)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
pub use wgo_daemon_host::server::PairingConfirmationRequest;
use wgo_daemon_host::server::{PairingCodeNotification, PairingNotifier};

pub const USER_PIPE_NAME: &str = r"\\.\pipe\wgo-user-active";
const PAIRING_IPC_VERSION: &str = "pairing.v2";
#[cfg(windows)]
const PAIRING_IPC_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct UserDaemonRegistration {
    pub pipe_name: String,
    pub user_name: String,
    pub session_id: u32,
}

impl UserDaemonRegistration {
    pub fn active_user(user_name: impl Into<String>) -> Self {
        Self {
            pipe_name: USER_PIPE_NAME.to_string(),
            user_name: user_name.into(),
            session_id: 0,
        }
    }
}

pub type PairingNotification = PairingCodeNotification;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingIpcRequest {
    Confirm(PairingConfirmationRequest),
    ShowCode(PairingNotification),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UserTrayPairingNotifier;

impl PairingNotifier for UserTrayPairingNotifier {
    fn confirm_pairing_request(
        &self,
        request: PairingConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move { send_pairing_ipc_request(PairingIpcRequest::Confirm(request)).await })
    }

    fn notify_pairing_code(
        &self,
        notification: PairingCodeNotification,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            send_pairing_ipc_request(PairingIpcRequest::ShowCode(notification)).await
        })
    }
}

#[cfg(windows)]
async fn send_pairing_ipc_request(request: PairingIpcRequest) -> Result<()> {
    let mut client = ClientOptions::new()
        .open(USER_PIPE_NAME)
        .with_context(|| format!("open user daemon pipe {USER_PIPE_NAME}"))?;
    client
        .write_all(encode_pairing_ipc_request(&request).as_bytes())
        .await
        .context("write pairing request to user daemon pipe")?;
    client
        .flush()
        .await
        .context("flush pairing request to user daemon pipe")?;

    read_pairing_ipc_response(&mut client).await
}

#[cfg(not(windows))]
async fn send_pairing_ipc_request(_request: PairingIpcRequest) -> Result<()> {
    anyhow::bail!("Windows user tray pairing notification is only available on Windows");
}

#[cfg(windows)]
pub fn spawn_pairing_notification_server(
    handler: impl Fn(PairingIpcRequest) -> Result<()> + Send + Sync + 'static,
) {
    let handler = Arc::new(handler);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::warn!(?err, "failed to create pairing notification runtime");
                return;
            }
        };
        if let Err(err) = runtime.block_on(run_pairing_notification_server(handler)) {
            tracing::warn!(?err, "pairing notification pipe server stopped");
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_pairing_notification_server(
    _handler: impl Fn(PairingIpcRequest) -> Result<()> + Send + Sync + 'static,
) {
}

#[cfg(windows)]
async fn run_pairing_notification_server(
    handler: Arc<dyn Fn(PairingIpcRequest) -> Result<()> + Send + Sync>,
) -> std::io::Result<()> {
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(USER_PIPE_NAME)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().create(USER_PIPE_NAME)?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let mut pipe = connected;
            let request = match read_pairing_ipc_request(&mut pipe).await {
                Ok(request) => request,
                Err(err) => {
                    tracing::warn!(?err, "failed to read pairing notification pipe request");
                    return;
                }
            };
            let response = match tokio::task::spawn_blocking(move || handler(request)).await {
                Ok(Ok(())) => encode_pairing_ipc_ok_response(),
                Ok(Err(err)) => encode_pairing_ipc_error_response(&err.to_string()),
                Err(err) => encode_pairing_ipc_error_response(&err.to_string()),
            };
            if let Err(err) = pipe.write_all(response.as_bytes()).await {
                tracing::warn!(?err, "failed to write pairing notification pipe response");
                return;
            }
            if let Err(err) = pipe.shutdown().await {
                tracing::warn!(?err, "failed to finish pairing notification pipe response");
            }
        });
    }
}

#[cfg(windows)]
async fn read_pairing_ipc_request(pipe: &mut NamedPipeServer) -> Result<PairingIpcRequest> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 256];
    loop {
        let count = pipe
            .read(&mut buffer)
            .await
            .context("read pairing request from user daemon pipe")?;
        if count == 0 {
            bail!("pairing request pipe closed before a complete request was received");
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > PAIRING_IPC_MAX_BYTES {
            bail!("pairing request exceeds {PAIRING_IPC_MAX_BYTES} bytes");
        }
        let text = std::str::from_utf8(&bytes).context("pairing request is not UTF-8")?;
        if let Some(request) = decode_pairing_ipc_request(text) {
            return Ok(request);
        }
    }
}

#[cfg(windows)]
async fn read_pairing_ipc_response(client: &mut NamedPipeClient) -> Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 256];
    loop {
        let count = client
            .read(&mut buffer)
            .await
            .context("read pairing response from user daemon pipe")?;
        if count == 0 {
            bail!("pairing response pipe closed before a complete response was received");
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > PAIRING_IPC_MAX_BYTES {
            bail!("pairing response exceeds {PAIRING_IPC_MAX_BYTES} bytes");
        }
        let text = std::str::from_utf8(&bytes).context("pairing response is not UTF-8")?;
        if let Some(response) = decode_pairing_ipc_response(text) {
            return response;
        }
    }
}

fn encode_pairing_ipc_request(request: &PairingIpcRequest) -> String {
    match request {
        PairingIpcRequest::Confirm(request) => {
            format!(
                "{PAIRING_IPC_VERSION}\nconfirm\n{}\n{}\n{}\n",
                ipc_line(&request.daemon_url),
                ipc_line(&request.confirmation_code),
                ipc_line(&request.client_label)
            )
        }
        PairingIpcRequest::ShowCode(notification) => {
            format!(
                "{PAIRING_IPC_VERSION}\ncode\n{}\n{}\n{}\n",
                ipc_line(&notification.daemon_url),
                ipc_line(&notification.pairing_code),
                notification.expires_in_seconds
            )
        }
    }
}

fn ipc_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn decode_pairing_ipc_request(text: &str) -> Option<PairingIpcRequest> {
    if !text.ends_with('\n') {
        return None;
    }
    let mut lines = text.lines();
    if lines.next()? != PAIRING_IPC_VERSION {
        return None;
    }
    match lines.next()? {
        "confirm" => Some(PairingIpcRequest::Confirm(PairingConfirmationRequest {
            daemon_url: lines.next()?.to_string(),
            confirmation_code: lines.next()?.to_string(),
            client_label: lines.next()?.to_string(),
        })),
        "code" => Some(PairingIpcRequest::ShowCode(PairingNotification {
            daemon_url: lines.next()?.to_string(),
            pairing_code: lines.next()?.to_string(),
            expires_in_seconds: lines.next()?.parse().ok()?,
        })),
        _ => None,
    }
}

fn encode_pairing_ipc_ok_response() -> String {
    format!("{PAIRING_IPC_VERSION}\nok\n")
}

fn encode_pairing_ipc_error_response(message: &str) -> String {
    format!(
        "{PAIRING_IPC_VERSION}\nerror\n{}\n",
        message.replace('\n', " ")
    )
}

fn decode_pairing_ipc_response(text: &str) -> Option<Result<()>> {
    if !text.ends_with('\n') {
        return None;
    }
    let mut lines = text.lines();
    if lines.next() != Some(PAIRING_IPC_VERSION) {
        return Some(Err(anyhow!("pairing response has an unknown version")));
    }
    match lines.next()? {
        "ok" => Some(Ok(())),
        "error" => Some(Err(anyhow!(lines
            .next()
            .unwrap_or("pairing request was rejected")
            .to_string()))),
        _ => Some(Err(anyhow!("pairing response is malformed"))),
    }
}
