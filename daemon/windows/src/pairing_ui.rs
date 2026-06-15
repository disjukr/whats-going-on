use anyhow::Result;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wgo_daemon_core::config::{daemon_status_path, load_or_default, SystemConfig, TlsConfig};

#[cfg(windows)]
use windows::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub struct PairingWindowModel {
    pub daemon_url: String,
    pub pairing_code: String,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct PairingConfirmationModel {
    pub confirmation_code: String,
    pub client_label: String,
}

#[derive(Debug, Clone)]
pub struct DaemonInfoWindowModel {
    pub daemon_url: String,
    pub daemon_version: String,
}

pub fn show_daemon_info_window(config_path: &Path) -> Result<()> {
    let config = load_or_default(config_path)?;
    show_daemon_info(&DaemonInfoWindowModel {
        daemon_url: default_daemon_url(&config),
        daemon_version: daemon_version(),
    })
}

pub fn is_pairing_ui_available(config_path: &Path) -> bool {
    let Ok(config) = load_or_default(config_path) else {
        return false;
    };
    has_usable_daemon_endpoint(config_path, &config) && daemon_status_is_ready(config_path)
}

#[cfg(windows)]
pub fn show_daemon_info_window_owned(config_path: &Path, owner: HWND) -> Result<()> {
    let config = load_or_default(config_path)?;
    show_daemon_info_owned(
        &DaemonInfoWindowModel {
            daemon_url: default_daemon_url(&config),
            daemon_version: daemon_version(),
        },
        owner,
    )
}

pub fn show_pairing_window(model: &PairingWindowModel) -> Result<()> {
    let message = pairing_dialog_content(&model.pairing_code, model.expires_in_seconds);
    show_copyable_text_window("Pairing code", &message, &model.pairing_code, "Copy Code")
}

#[cfg(windows)]
pub fn confirm_pairing_request_owned(
    model: &PairingConfirmationModel,
    owner: HWND,
) -> Result<bool> {
    let candidates = confirmation_code_candidates(&model.confirmation_code);
    let message = format!(
        "Client:\n{}\n\nSelect the two-digit code shown on that client.",
        pairing_client_label(&model.client_label)
    );
    let Some(selected) =
        task_dialog::select_owned("Confirm pairing", &message, &candidates, owner)?
    else {
        return Ok(false);
    };
    Ok(selected == model.confirmation_code)
}

#[cfg(windows)]
pub fn close_active_pairing_confirmation_window() {
    task_dialog::close_active_select_dialog();
}

pub fn show_daemon_info(model: &DaemonInfoWindowModel) -> Result<()> {
    let message = format!(
        "Daemon URL:\n{}\n\nDaemon version:\n{}",
        model.daemon_url, model.daemon_version
    );
    show_copyable_text_window("Daemon info", &message, &model.daemon_url, "Copy URL")
}

#[cfg(windows)]
pub fn show_daemon_info_owned(model: &DaemonInfoWindowModel, owner: HWND) -> Result<()> {
    let message = format!(
        "Daemon URL:\n{}\n\nDaemon version:\n{}",
        model.daemon_url, model.daemon_version
    );
    task_dialog::show_owned(
        "Daemon info",
        &message,
        &model.daemon_url,
        "Copy URL",
        owner,
    )
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

fn daemon_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
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

fn confirmation_code_candidates(code: &str) -> Vec<String> {
    let normalized = normalize_confirmation_code(code);
    let mut seed = confirmation_candidate_seed(&normalized);
    let mut candidates = vec![normalized.clone()];
    while candidates.len() < 4 {
        seed = lcg_next(seed);
        let candidate = format!("{:02}", seed % 100);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }

    for index in (1..candidates.len()).rev() {
        seed = lcg_next(seed);
        let swap_index = (seed as usize) % (index + 1);
        candidates.swap(index, swap_index);
    }
    candidates
}

fn normalize_confirmation_code(code: &str) -> String {
    let digits: String = code
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(2)
        .collect();
    if digits.len() == 2 {
        digits
    } else {
        "00".to_string()
    }
}

fn pairing_client_label(label: &str) -> &str {
    let label = label.trim();
    if label.is_empty() {
        "Unknown client"
    } else {
        label
    }
}

fn confirmation_candidate_seed(code: &str) -> u64 {
    let time_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    code.bytes()
        .fold(time_seed ^ 0x9e37_79b9_7f4a_7c15, |seed, byte| {
            seed.rotate_left(5) ^ u64::from(byte)
        })
}

fn lcg_next(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1)
}

#[cfg(windows)]
fn show_copyable_text_window(
    title: &str,
    text: &str,
    copy_text: &str,
    copy_button_label: &str,
) -> Result<()> {
    task_dialog::show(title, text, copy_text, copy_button_label)
}

#[cfg(not(windows))]
fn show_copyable_text_window(
    title: &str,
    text: &str,
    _copy_text: &str,
    _copy_button_label: &str,
) -> Result<()> {
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
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::copy_nonoverlapping;
    use std::sync::{Mutex, OnceLock};

    use anyhow::{anyhow, Context, Result};
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
        TDF_ALLOW_DIALOG_CANCELLATION, TDF_SIZE_TO_CONTENT, TDM_CLICK_BUTTON, TDN_CREATED,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
        HWND_NOTOPMOST, HWND_TOPMOST, IDCANCEL, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOW,
    };

    const CMD_COPY: usize = 2001;
    const CMD_CLOSE: usize = 2002;
    const CMD_OPTION_BASE: usize = 2100;
    const CF_UNICODETEXT_FORMAT: u32 = 13;
    static ACTIVE_SELECT_DIALOG: OnceLock<Mutex<Option<isize>>> = OnceLock::new();

    pub fn show(title: &str, text: &str, copy_text: &str, copy_button_label: &str) -> Result<()> {
        show_owned(title, text, copy_text, copy_button_label, HWND::default())
    }

    pub fn show_owned(
        title: &str,
        text: &str,
        copy_text: &str,
        copy_button_label: &str,
        owner: HWND,
    ) -> Result<()> {
        let window_title = wide_null("Whats Going On");
        let main_instruction = wide_null(title);
        let content = wide_null(text);
        let copy_button_text = wide_null(copy_button_label);
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
            pfCallback: Some(dialog_callback),
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

    pub fn select_owned(
        title: &str,
        text: &str,
        options: &[String],
        owner: HWND,
    ) -> Result<Option<String>> {
        let window_title = wide_null("Whats Going On");
        let main_instruction = wide_null(title);
        let content = wide_null(text);
        let option_texts: Vec<Vec<u16>> = options.iter().map(|option| wide_null(option)).collect();
        let buttons: Vec<TASKDIALOG_BUTTON> = option_texts
            .iter()
            .enumerate()
            .map(|(index, text)| TASKDIALOG_BUTTON {
                nButtonID: (CMD_OPTION_BASE + index) as i32,
                pszButtonText: PCWSTR(text.as_ptr()),
            })
            .collect();

        let config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
            pszWindowTitle: PCWSTR(window_title.as_ptr()),
            pszMainInstruction: PCWSTR(main_instruction.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: CMD_OPTION_BASE as i32,
            pfCallback: Some(select_dialog_callback),
            ..Default::default()
        };

        let mut selected_button = 0;
        unsafe {
            TaskDialogIndirect(&config, Some(&mut selected_button), None, None)
                .context("show task dialog")?;
        }
        clear_active_select_dialog();

        let index = selected_button - CMD_OPTION_BASE as i32;
        if index < 0 {
            return Ok(None);
        }
        Ok(options.get(index as usize).cloned())
    }

    pub fn close_active_select_dialog() {
        let Some(hwnd_value) = active_select_dialog().lock().ok().and_then(|hwnd| *hwnd) else {
            return;
        };
        let hwnd = HWND(hwnd_value as *mut c_void);
        let _ = unsafe {
            PostMessageW(
                Some(hwnd),
                TDM_CLICK_BUTTON.0 as u32,
                WPARAM(IDCANCEL.0 as usize),
                LPARAM(0),
            )
        };
    }

    fn active_select_dialog() -> &'static Mutex<Option<isize>> {
        ACTIVE_SELECT_DIALOG.get_or_init(|| Mutex::new(None))
    }

    fn clear_active_select_dialog() {
        if let Ok(mut active) = active_select_dialog().lock() {
            *active = None;
        }
    }

    unsafe extern "system" fn select_dialog_callback(
        hwnd: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        _wparam: WPARAM,
        _lparam: LPARAM,
        _lprefdata: isize,
    ) -> HRESULT {
        if msg == TDN_CREATED {
            present_dialog(hwnd);
            if let Ok(mut active) = active_select_dialog().lock() {
                *active = Some(hwnd.0 as isize);
            }
        }
        HRESULT(0)
    }

    unsafe extern "system" fn dialog_callback(
        hwnd: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        _wparam: WPARAM,
        _lparam: LPARAM,
        _lprefdata: isize,
    ) -> HRESULT {
        if msg == TDN_CREATED {
            present_dialog(hwnd);
        }
        HRESULT(0)
    }

    fn present_dialog(hwnd: HWND) {
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
        }
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
