use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
pub use wgo_daemon_host::server::PairingConfirmationRequest;
use wgo_daemon_host::server::{PairingCodeNotification, PairingNotifier};

const PAIRING_IPC_VERSION: &str = "pairing.v2";
const PAIRING_IPC_MAX_BYTES: usize = 4096;

pub type PairingNotification = PairingCodeNotification;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingIpcRequest {
    Confirm(PairingConfirmationRequest),
    ShowCode(PairingNotification),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacUserPairingNotifier;

impl PairingNotifier for MacUserPairingNotifier {
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
            tracing::warn!(?err, "pairing notification socket server stopped");
        }
    });
}

async fn send_pairing_ipc_request(request: PairingIpcRequest) -> Result<()> {
    let socket_path = active_user_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .await
        .with_context(|| format!("connect user daemon socket {}", socket_path.display()))?;
    stream
        .write_all(encode_pairing_ipc_request(&request).as_bytes())
        .await
        .context("write pairing request to user daemon socket")?;
    stream
        .shutdown()
        .await
        .context("finish pairing request to user daemon socket")?;

    read_pairing_ipc_response(&mut stream).await
}

async fn run_pairing_notification_server(
    handler: Arc<dyn Fn(PairingIpcRequest) -> Result<()> + Send + Sync>,
) -> Result<()> {
    let socket_path = current_user_socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("bind user daemon socket {}", socket_path.display()))?;

    loop {
        let (mut stream, _) = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let request = match read_pairing_ipc_request(&mut stream).await {
                Ok(request) => request,
                Err(err) => {
                    tracing::warn!(?err, "failed to read pairing notification socket request");
                    return;
                }
            };
            let response = match tokio::task::spawn_blocking(move || handler(request)).await {
                Ok(Ok(())) => encode_pairing_ipc_ok_response(),
                Ok(Err(err)) => encode_pairing_ipc_error_response(&err.to_string()),
                Err(err) => encode_pairing_ipc_error_response(&err.to_string()),
            };
            if let Err(err) = stream.write_all(response.as_bytes()).await {
                tracing::warn!(?err, "failed to write pairing notification socket response");
            }
        });
    }
}

async fn read_pairing_ipc_request(stream: &mut UnixStream) -> Result<PairingIpcRequest> {
    let text = read_ipc_text(stream, "pairing request").await?;
    decode_pairing_ipc_request(&text).context("pairing request is incomplete or invalid")
}

async fn read_pairing_ipc_response(stream: &mut UnixStream) -> Result<()> {
    let text = read_ipc_text(stream, "pairing response").await?;
    decode_pairing_ipc_response(&text).context("pairing response is incomplete or invalid")?
}

async fn read_ipc_text(stream: &mut UnixStream, label: &str) -> Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 256];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .with_context(|| format!("read {label} from user daemon socket"))?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > PAIRING_IPC_MAX_BYTES {
            bail!("{label} exceeds {PAIRING_IPC_MAX_BYTES} bytes");
        }
    }
    String::from_utf8(bytes).with_context(|| format!("{label} is not UTF-8"))
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
    if lines.next()? != PAIRING_IPC_VERSION {
        return None;
    }
    match lines.next()? {
        "ok" => Some(Ok(())),
        "error" => Some(Err(anyhow::anyhow!(
            "{}",
            lines.next().unwrap_or("pairing request failed")
        ))),
        _ => None,
    }
}

fn active_user_socket_path() -> PathBuf {
    if let Some(uid) = std::env::var_os("SUDO_UID").filter(|uid| !uid.is_empty()) {
        return socket_path_for_uid(uid.to_string_lossy());
    }
    #[cfg(target_os = "macos")]
    if let Some(uid) = console_user_uid() {
        return socket_path_for_uid(uid);
    }
    current_user_socket_path()
}

#[cfg(target_os = "macos")]
fn console_user_uid() -> Option<String> {
    let output = std::process::Command::new("stat")
        .args(["-f", "%u", "/dev/console"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty() && uid != "0")
}

fn current_user_socket_path() -> PathBuf {
    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    socket_path_for_uid(uid)
}

fn socket_path_for_uid(uid: impl AsRef<str>) -> PathBuf {
    PathBuf::from("/tmp").join(format!("wgo-user-active-{}.sock", uid.as_ref()))
}
