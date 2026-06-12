use anyhow::Result;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use wgo_daemon_core::config::{
    daemon_status_path, load_or_default, load_pairing_state_or_default, pairing_state_path,
    save_pairing_state, SystemConfig, TlsConfig,
};
use wgo_daemon_core::pairing::create_pairing_code;

#[cfg(windows)]
use windows::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub struct PairingWindowModel {
    pub daemon_url: String,
    pub pairing_code: String,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct MachineInfoWindowModel {
    pub daemon_url: String,
}

#[derive(Debug, Clone)]
struct ActivePairingCode {
    code: String,
    expires_at_unix: i64,
}

pub fn show_machine_info_window(config_path: &Path) -> Result<()> {
    let config = load_or_default(config_path)?;
    show_machine_info(&MachineInfoWindowModel {
        daemon_url: default_daemon_url(&config),
    })
}

pub fn is_pairing_ui_available(config_path: &Path) -> bool {
    let Ok(config) = load_or_default(config_path) else {
        return false;
    };
    has_usable_daemon_endpoint(config_path, &config) && daemon_status_is_ready(config_path)
}

#[cfg(windows)]
pub fn show_machine_info_window_owned(config_path: &Path, owner: HWND) -> Result<()> {
    let config = load_or_default(config_path)?;
    show_machine_info_owned(
        &MachineInfoWindowModel {
            daemon_url: default_daemon_url(&config),
        },
        owner,
    )
}

pub fn create_and_show_pairing_window(config_path: &Path, _daemon_url: Option<&str>) -> Result<()> {
    let pairing = create_and_save_pairing_code(config_path)?;
    show_live_pairing_window(config_path, pairing)
}

#[cfg(windows)]
pub fn create_and_show_pairing_window_owned(
    config_path: &Path,
    _daemon_url: Option<&str>,
    owner: HWND,
) -> Result<()> {
    let pairing = create_and_save_pairing_code(config_path)?;
    task_dialog::show_pairing_code(config_path, pairing, owner)
}

pub fn show_pairing_window(model: &PairingWindowModel) -> Result<()> {
    let message = pairing_dialog_content(&model.pairing_code, model.expires_in_seconds);
    show_copyable_text_window("Pairing code", &message, &model.pairing_code)
}

pub fn show_machine_info(model: &MachineInfoWindowModel) -> Result<()> {
    let message = format!(
        "Use this URL to add this machine from a client.\n\n{}",
        model.daemon_url
    );
    show_copyable_text_window("Machine info", &message, &model.daemon_url)
}

#[cfg(windows)]
pub fn show_machine_info_owned(model: &MachineInfoWindowModel, owner: HWND) -> Result<()> {
    let message = format!(
        "Use this URL to add this machine from a client.\n\n{}",
        model.daemon_url
    );
    task_dialog::show_owned("Machine info", &message, &model.daemon_url, owner)
}

pub fn show_error_window(message: &str) -> Result<()> {
    show_message_box("wgo error", message)
}

fn default_daemon_url(config: &SystemConfig) -> String {
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

fn has_usable_daemon_endpoint(config_path: &Path, config: &SystemConfig) -> bool {
    if let Some(tls) = &config.tls {
        return configured_tls_files_exist(config_path, tls);
    }
    config
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .is_some_and(|domain| {
            domain
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .ends_with(".ts.net")
        })
}

fn daemon_status_is_ready(config_path: &Path) -> bool {
    std::fs::read_to_string(daemon_status_path(config_path))
        .is_ok_and(|status| status.lines().next() == Some("ready"))
}

fn configured_tls_files_exist(config_path: &Path, tls: &TlsConfig) -> bool {
    resolve_config_relative(config_path, &tls.cert_file).is_file()
        && resolve_config_relative(config_path, &tls.key_file).is_file()
}

fn resolve_config_relative(config_path: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn create_and_save_pairing_code(config_path: &Path) -> Result<ActivePairingCode> {
    let pairing_path = pairing_state_path(config_path);
    let mut state = load_pairing_state_or_default(&pairing_path)?;
    let now = now_unix();
    let pairing = create_pairing_code(now);
    let expires_at_unix = pairing.record.expires_at_unix;
    state.pairing = Some(pairing.record);
    save_pairing_state(&pairing_path, &state)?;
    Ok(ActivePairingCode {
        code: pairing.code,
        expires_at_unix,
    })
}

fn pairing_dialog_content(code: &str, remaining_seconds: i64) -> String {
    format!(
        "Code: {}\nExpires in: {}",
        code,
        format_remaining_time(remaining_seconds)
    )
}

fn format_remaining_time(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(windows)]
fn show_live_pairing_window(config_path: &Path, pairing: ActivePairingCode) -> Result<()> {
    task_dialog::show_pairing_code(config_path, pairing, HWND::default())
}

#[cfg(not(windows))]
fn show_live_pairing_window(_config_path: &Path, pairing: ActivePairingCode) -> Result<()> {
    let now = now_unix();
    show_pairing_window(&PairingWindowModel {
        daemon_url: String::new(),
        pairing_code: pairing.code,
        expires_in_seconds: pairing.expires_at_unix - now,
    })
}

#[cfg(windows)]
fn show_copyable_text_window(title: &str, text: &str, copy_text: &str) -> Result<()> {
    task_dialog::show(title, text, copy_text)
}

#[cfg(not(windows))]
fn show_copyable_text_window(title: &str, text: &str, _copy_text: &str) -> Result<()> {
    println!("{title}\n{text}");
    Ok(())
}

#[cfg(windows)]
fn show_message_box(title: &str, message: &str) -> Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn show_message_box(title: &str, message: &str) -> Result<()> {
    println!("{title}\n{message}");
    Ok(())
}

#[cfg(windows)]
mod task_dialog {
    use std::mem::size_of;
    use std::path::{Path, PathBuf};
    use std::ptr::copy_nonoverlapping;

    use anyhow::{anyhow, Context, Result};
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
        TDE_CONTENT, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDF_SIZE_TO_CONTENT,
        TDM_SET_ELEMENT_TEXT, TDN_BUTTON_CLICKED, TDN_CREATED, TDN_TIMER,
    };
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

    use super::{
        create_and_save_pairing_code, now_unix, pairing_dialog_content, ActivePairingCode,
    };

    const CMD_COPY: usize = 2001;
    const CMD_CLOSE: usize = 2002;
    const CF_UNICODETEXT_FORMAT: u32 = 13;
    const S_OK: HRESULT = HRESULT(0);
    const S_FALSE: HRESULT = HRESULT(1);

    struct PairingDialogState {
        config_path: PathBuf,
        code: String,
        expires_at_unix: i64,
        last_rendered_remaining: Option<i64>,
        last_rotation_attempt_unix: Option<i64>,
        last_error: Option<String>,
        content_buffer: Vec<u16>,
    }

    impl PairingDialogState {
        fn new(config_path: &Path, pairing: ActivePairingCode) -> Self {
            Self {
                config_path: config_path.to_path_buf(),
                code: pairing.code,
                expires_at_unix: pairing.expires_at_unix,
                last_rendered_remaining: None,
                last_rotation_attempt_unix: None,
                last_error: None,
                content_buffer: Vec::new(),
            }
        }

        fn initial_content(&self) -> String {
            pairing_dialog_content(&self.code, self.remaining_seconds())
        }

        fn refresh(&mut self, hwnd: HWND) -> Result<()> {
            let now = now_unix();
            if now >= self.expires_at_unix && self.last_rotation_attempt_unix != Some(now) {
                self.last_rotation_attempt_unix = Some(now);
                let pairing = create_and_save_pairing_code(&self.config_path)
                    .context("refresh pairing code")?;
                self.code = pairing.code;
                self.expires_at_unix = pairing.expires_at_unix;
                self.last_rendered_remaining = None;
            }

            let remaining = self.remaining_seconds();
            if self.last_rendered_remaining == Some(remaining) && self.last_error.is_none() {
                return Ok(());
            }

            self.last_error = None;
            self.last_rendered_remaining = Some(remaining);
            self.set_content(hwnd, &pairing_dialog_content(&self.code, remaining));
            Ok(())
        }

        fn render_error(&mut self, hwnd: HWND, err: &anyhow::Error) {
            let message = format!(
                "Code refresh failed.\n\n{}\n\nRetrying...",
                err.root_cause()
            );
            if self.last_error.as_deref() == Some(message.as_str()) {
                return;
            }
            self.last_error = Some(message.clone());
            self.set_content(hwnd, &message);
        }

        fn remaining_seconds(&self) -> i64 {
            self.expires_at_unix - now_unix()
        }

        fn set_content(&mut self, hwnd: HWND, text: &str) {
            self.content_buffer = wide_null(text);
            unsafe {
                SendMessageW(
                    hwnd,
                    TDM_SET_ELEMENT_TEXT.0 as u32,
                    Some(WPARAM(TDE_CONTENT.0 as usize)),
                    Some(LPARAM(self.content_buffer.as_ptr() as isize)),
                );
            }
        }
    }

    pub fn show(title: &str, text: &str, copy_text: &str) -> Result<()> {
        show_owned(title, text, copy_text, HWND::default())
    }

    pub fn show_owned(title: &str, text: &str, copy_text: &str, owner: HWND) -> Result<()> {
        let window_title = wide_null("Whats Going On");
        let main_instruction = wide_null(title);
        let content = wide_null(text);
        let copy_button_text = wide_null("Copy");
        let close_button_text = wide_null("Close");
        let buttons = [
            TASKDIALOG_BUTTON {
                nButtonID: CMD_COPY as i32,
                pszButtonText: PCWSTR(copy_button_text.as_ptr()),
            },
            TASKDIALOG_BUTTON {
                nButtonID: CMD_CLOSE as i32,
                pszButtonText: PCWSTR(close_button_text.as_ptr()),
            },
        ];

        let config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
            pszWindowTitle: PCWSTR(window_title.as_ptr()),
            pszMainInstruction: PCWSTR(main_instruction.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: CMD_COPY as i32,
            ..Default::default()
        };

        let mut selected_button = 0;
        unsafe {
            TaskDialogIndirect(&config, Some(&mut selected_button), None, None)
                .context("show task dialog")?;
        }

        if selected_button == CMD_COPY as i32 {
            copy_to_clipboard(copy_text).context("copy dialog value")?;
        }
        Ok(())
    }

    pub fn show_pairing_code(
        config_path: &Path,
        pairing: ActivePairingCode,
        owner: HWND,
    ) -> Result<()> {
        let window_title = wide_null("Whats Going On");
        let main_instruction = wide_null("Pairing code");
        let copy_button_text = wide_null("Copy code");
        let close_button_text = wide_null("Close");
        let mut state = PairingDialogState::new(config_path, pairing);
        let content = wide_null(&state.initial_content());
        let buttons = [
            TASKDIALOG_BUTTON {
                nButtonID: CMD_COPY as i32,
                pszButtonText: PCWSTR(copy_button_text.as_ptr()),
            },
            TASKDIALOG_BUTTON {
                nButtonID: CMD_CLOSE as i32,
                pszButtonText: PCWSTR(close_button_text.as_ptr()),
            },
        ];

        let config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER | TDF_SIZE_TO_CONTENT,
            pszWindowTitle: PCWSTR(window_title.as_ptr()),
            pszMainInstruction: PCWSTR(main_instruction.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: CMD_COPY as i32,
            pfCallback: Some(pairing_dialog_callback),
            lpCallbackData: (&mut state as *mut PairingDialogState) as isize,
            ..Default::default()
        };

        let mut selected_button = 0;
        unsafe {
            TaskDialogIndirect(&config, Some(&mut selected_button), None, None)
                .context("show pairing task dialog")?;
        }
        Ok(())
    }

    unsafe extern "system" fn pairing_dialog_callback(
        hwnd: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        wparam: WPARAM,
        _lparam: LPARAM,
        lprefdata: isize,
    ) -> HRESULT {
        let Some(state) = (lprefdata as *mut PairingDialogState).as_mut() else {
            return S_OK;
        };

        if msg == TDN_CREATED || msg == TDN_TIMER {
            if let Err(err) = state.refresh(hwnd) {
                state.render_error(hwnd, &err);
            }
            return S_OK;
        }

        if msg == TDN_BUTTON_CLICKED && wparam.0 == CMD_COPY {
            if let Err(err) = copy_to_clipboard(&state.code) {
                let _ = super::show_message_box("wgo error", &format!("Failed to copy:\n\n{err}"));
            }
            return S_FALSE;
        }

        S_OK
    }

    fn copy_to_clipboard(text: &str) -> Result<()> {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * size_of::<u16>();
        unsafe {
            let memory =
                GlobalAlloc(GMEM_MOVEABLE, byte_len).context("allocate clipboard memory")?;
            let locked = GlobalLock(memory);
            if locked.is_null() {
                return Err(anyhow!("lock clipboard memory"));
            }
            copy_nonoverlapping(wide.as_ptr(), locked.cast::<u16>(), wide.len());
            let _ = GlobalUnlock(memory);

            OpenClipboard(None).context("open clipboard")?;
            let result = (|| -> Result<()> {
                EmptyClipboard().context("empty clipboard")?;
                SetClipboardData(CF_UNICODETEXT_FORMAT, Some(HANDLE(memory.0)))
                    .context("set clipboard data")?;
                Ok(())
            })();
            let close_result = CloseClipboard().context("close clipboard");
            result.and(close_result)
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
