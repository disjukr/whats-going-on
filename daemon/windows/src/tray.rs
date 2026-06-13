use anyhow::Result;
use std::path::PathBuf;

#[cfg(windows)]
pub fn run_pairing_tray(config_path: PathBuf) -> Result<()> {
    windows_tray::run(config_path)
}

#[cfg(not(windows))]
pub fn run_pairing_tray(_config_path: PathBuf) -> Result<()> {
    anyhow::bail!("Windows tray UI is only available on Windows");
}

#[cfg(windows)]
mod windows_tray {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::Sender;
    use std::sync::{Mutex, OnceLock};

    use anyhow::{anyhow, Result};
    use wgo_daemon_core::config::{generated_default_system_config, save};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_REALTIME, NIF_TIP,
        NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK,
        NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
        PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu,
        TranslateMessage, HICON, LR_DEFAULTCOLOR, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
        SW_SHOWNORMAL, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP,
        WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WNDCLASSW, WS_OVERLAPPED,
    };

    use crate::ipc::{
        spawn_pairing_notification_server, PairingConfirmationRequest, PairingIpcRequest,
        PairingNotification,
    };
    use crate::pairing_ui::{
        close_active_pairing_confirmation_window, confirm_pairing_request_owned,
        is_pairing_ui_available, show_error_window, show_machine_info_window_owned,
        show_pairing_window, PairingConfirmationModel, PairingWindowModel,
    };

    const CLASS_NAME: PCWSTR = w!("WgoWindowsUserTrayWindow");
    const WINDOW_TITLE: PCWSTR = w!("Whats Going On");
    const TRAY_MESSAGE: u32 = WM_APP + 1;
    const PAIRING_NOTIFICATION_MESSAGE: u32 = WM_APP + 2;
    const PAIRING_CONFIRMATION_MESSAGE: u32 = WM_APP + 3;
    const TRAY_ICON_ID: u32 = 1;
    const CMD_SHOW_MACHINE_INFO: usize = 1001;
    const CMD_OPEN_SETTINGS: usize = 1002;
    const CMD_QUIT: usize = 1003;
    const NIN_KEYSELECT: u32 = 1025;
    const TRAY_ICON_BYTES: &[u8] = include_bytes!("../assets/tray.ico");
    const TRAY_ICON_SIZE: i32 = 32;
    const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;
    const CONFIG_NOT_READY_MESSAGE: &str =
        "Machine config is not ready. Set a .ts.net domain or TLS certificate files first.";

    struct TrayRuntime {
        config_path: PathBuf,
    }

    #[derive(Default)]
    struct PendingPairingNotification {
        notification: Option<PairingNotification>,
        message_posted: bool,
    }

    #[derive(Default)]
    struct PendingPairingConfirmation {
        request: Option<PairingConfirmationRequest>,
        responder: Option<Sender<Result<(), String>>>,
        message_posted: bool,
    }

    static TRAY_RUNTIME: OnceLock<TrayRuntime> = OnceLock::new();
    static PENDING_PAIRING_NOTIFICATION: OnceLock<Mutex<PendingPairingNotification>> =
        OnceLock::new();
    static PENDING_PAIRING_CONFIRMATION: OnceLock<Mutex<PendingPairingConfirmation>> =
        OnceLock::new();

    pub fn run(config_path: PathBuf) -> Result<()> {
        TRAY_RUNTIME
            .set(TrayRuntime { config_path })
            .map_err(|_| anyhow!("tray runtime was already initialized"))?;

        unsafe {
            let instance = current_instance()?;
            register_window_class(instance)?;
            let hwnd = create_message_window(instance)?;
            let icon = load_tray_icon()?;
            add_tray_icon(hwnd, icon)?;
            let hwnd_value = hwnd.0 as usize;
            spawn_pairing_notification_server(move |request| match request {
                PairingIpcRequest::Confirm(request) => {
                    confirm_pairing_request_via_tray(hwnd_value, request)
                }
                PairingIpcRequest::ShowCode(notification) => {
                    post_pairing_notification(hwnd_value, notification)
                }
            });

            let message_result = run_message_loop();
            remove_tray_icon(hwnd);
            let _ = DestroyIcon(icon);
            let _ = DestroyWindow(hwnd);
            message_result
        }
    }

    unsafe fn current_instance() -> Result<HINSTANCE> {
        let module = unsafe { GetModuleHandleW(None)? };
        Ok(HINSTANCE(module.0))
    }

    unsafe fn register_window_class(instance: HINSTANCE) -> Result<()> {
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&window_class) };
        if atom == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(())
    }

    unsafe fn create_message_window(instance: HINSTANCE) -> Result<HWND> {
        Ok(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                WINDOW_TITLE,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?
        })
    }

    unsafe fn add_tray_icon(hwnd: HWND, icon: HICON) -> Result<()> {
        let mut data = notify_icon_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = icon;
        write_wide(&mut data.szTip, "Whats Going On");

        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            return Err(windows::core::Error::from_thread().into());
        }

        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool() {
            remove_tray_icon(hwnd);
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(())
    }

    fn load_tray_icon() -> Result<HICON> {
        let image = select_ico_image(TRAY_ICON_BYTES, TRAY_ICON_SIZE as u16)?;
        unsafe {
            CreateIconFromResourceEx(
                image,
                true,
                ICON_RESOURCE_VERSION,
                TRAY_ICON_SIZE,
                TRAY_ICON_SIZE,
                LR_DEFAULTCOLOR,
            )
            .map_err(Into::into)
        }
    }

    fn select_ico_image(ico: &[u8], desired_size: u16) -> Result<&[u8]> {
        if read_u16_le(ico, 0)? != 0 || read_u16_le(ico, 2)? != 1 {
            return Err(anyhow!("invalid tray icon file"));
        }

        let count = read_u16_le(ico, 4)? as usize;
        let mut best: Option<(u16, &[u8])> = None;
        for index in 0..count {
            let entry_offset = 6 + index * 16;
            if entry_offset + 16 > ico.len() {
                return Err(anyhow!("invalid tray icon directory"));
            }

            let width = icon_dimension(ico[entry_offset]);
            let height = icon_dimension(ico[entry_offset + 1]);
            if width != height {
                continue;
            }

            let image_size = read_u32_le(ico, entry_offset + 8)? as usize;
            let image_offset = read_u32_le(ico, entry_offset + 12)? as usize;
            let image = ico
                .get(image_offset..image_offset + image_size)
                .ok_or_else(|| anyhow!("invalid tray icon image"))?;

            let should_replace = match best {
                None => true,
                Some((best_size, _)) if width >= desired_size && best_size < desired_size => true,
                Some((best_size, _)) if width >= desired_size && best_size >= desired_size => {
                    width < best_size
                }
                Some((best_size, _)) if width < desired_size && best_size < desired_size => {
                    width > best_size
                }
                _ => false,
            };

            if should_replace {
                best = Some((width, image));
            }
        }

        best.map(|(_, image)| image)
            .ok_or_else(|| anyhow!("tray icon file does not contain any square images"))
    }

    fn icon_dimension(value: u8) -> u16 {
        if value == 0 {
            256
        } else {
            value as u16
        }
    }

    fn read_u16_le(input: &[u8], offset: usize) -> Result<u16> {
        let bytes = input
            .get(offset..offset + 2)
            .ok_or_else(|| anyhow!("invalid tray icon file"))?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32_le(input: &[u8], offset: usize) -> Result<u32> {
        let bytes = input
            .get(offset..offset + 4)
            .ok_or_else(|| anyhow!("invalid tray icon file"))?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn remove_tray_icon(hwnd: HWND) {
        let data = notify_icon_data(hwnd);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    fn notify_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        }
    }

    unsafe fn run_message_loop() -> Result<()> {
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            TRAY_MESSAGE => {
                let event = tray_event(lparam);
                match event {
                    NIN_BALLOONUSERCLICK => show_pending_pairing_confirmation(hwnd),
                    NIN_SELECT | NIN_KEYSELECT => {
                        if has_pending_pairing_confirmation() {
                            show_pending_pairing_confirmation(hwnd);
                        } else {
                            show_machine_info(hwnd);
                        }
                    }
                    WM_CONTEXTMENU => show_context_menu(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            PAIRING_NOTIFICATION_MESSAGE => {
                if wparam.0 != 0 {
                    return LRESULT(0);
                }
                let Some(notification) = take_pending_pairing_notification() else {
                    return LRESULT(0);
                };
                if let Err(err) = show_pairing_notification(hwnd, &notification) {
                    let _ = show_pairing_window(&PairingWindowModel {
                        daemon_url: notification.daemon_url.clone(),
                        pairing_code: notification.pairing_code.clone(),
                        expires_in_seconds: notification.expires_in_seconds,
                    });
                    let _ = show_error_window(&format!(
                        "Failed to show pairing notification:\n\n{err}"
                    ));
                }
                LRESULT(0)
            }
            PAIRING_CONFIRMATION_MESSAGE => {
                if let Err(err) = show_pairing_confirmation_notification(hwnd) {
                    let _ = show_error_window(&format!(
                        "Failed to show pairing confirmation notification:\n\n{err}"
                    ));
                    show_pending_pairing_confirmation(hwnd);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                match low_word(wparam.0) as usize {
                    CMD_SHOW_MACHINE_INFO => show_machine_info(hwnd),
                    CMD_OPEN_SETTINGS => open_settings(hwnd),
                    CMD_QUIT => {
                        let _ = unsafe { DestroyWindow(hwnd) };
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                remove_tray_icon(hwnd);
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn post_pairing_notification(
        hwnd_value: usize,
        notification: PairingNotification,
    ) -> Result<()> {
        let should_post = match queue_pairing_notification(notification) {
            Ok(should_post) => should_post,
            Err(err) => {
                let _ =
                    show_error_window(&format!("Failed to queue pairing notification:\n\n{err}"));
                return Err(err);
            }
        };
        if !should_post {
            return Ok(());
        }

        let hwnd = HWND(hwnd_value as *mut c_void);
        if let Err(err) = unsafe {
            PostMessageW(
                Some(hwnd),
                PAIRING_NOTIFICATION_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            )
        } {
            clear_pending_pairing_notification();
            let _ = show_error_window(&format!("Failed to queue pairing notification:\n\n{err}"));
            return Err(err.into());
        }
        Ok(())
    }

    fn pending_pairing_notification() -> &'static Mutex<PendingPairingNotification> {
        PENDING_PAIRING_NOTIFICATION
            .get_or_init(|| Mutex::new(PendingPairingNotification::default()))
    }

    fn queue_pairing_notification(notification: PairingNotification) -> Result<bool> {
        let mut pending = pending_pairing_notification()
            .lock()
            .map_err(|_| anyhow!("pending pairing notification mutex was poisoned"))?;
        pending.notification = Some(notification);
        if pending.message_posted {
            return Ok(false);
        }
        pending.message_posted = true;
        Ok(true)
    }

    fn take_pending_pairing_notification() -> Option<PairingNotification> {
        let Ok(mut pending) = pending_pairing_notification().lock() else {
            return None;
        };
        pending.message_posted = false;
        pending.notification.take()
    }

    fn clear_pending_pairing_notification() {
        if let Ok(mut pending) = pending_pairing_notification().lock() {
            pending.message_posted = false;
            pending.notification = None;
        }
    }

    fn confirm_pairing_request_via_tray(
        hwnd_value: usize,
        request: PairingConfirmationRequest,
    ) -> Result<()> {
        close_active_pairing_confirmation_window();
        let (sender, receiver) = std::sync::mpsc::channel();
        let should_post = queue_pairing_confirmation(request, sender)?;
        if should_post {
            let hwnd = HWND(hwnd_value as *mut c_void);
            if let Err(err) = unsafe {
                PostMessageW(
                    Some(hwnd),
                    PAIRING_CONFIRMATION_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                clear_pending_pairing_confirmation();
                return Err(err.into());
            }
        }
        receiver
            .recv()
            .map_err(|_| anyhow!("pairing confirmation response channel closed"))?
            .map_err(anyhow::Error::msg)
    }

    fn pending_pairing_confirmation() -> &'static Mutex<PendingPairingConfirmation> {
        PENDING_PAIRING_CONFIRMATION
            .get_or_init(|| Mutex::new(PendingPairingConfirmation::default()))
    }

    fn queue_pairing_confirmation(
        request: PairingConfirmationRequest,
        responder: Sender<Result<(), String>>,
    ) -> Result<bool> {
        let mut pending = pending_pairing_confirmation()
            .lock()
            .map_err(|_| anyhow!("pending pairing confirmation mutex was poisoned"))?;
        let replaced_pending_request = pending.request.is_some() || pending.responder.is_some();
        if let Some(previous) = pending.responder.take() {
            let _ = previous.send(Err("superseded by a newer pairing request".to_string()));
        }
        pending.request = Some(request);
        pending.responder = Some(responder);
        // A toast can expire while the pending confirmation stays active, so
        // replacing it must post another window message to show a fresh toast.
        if pending.message_posted && !replaced_pending_request {
            return Ok(false);
        }
        pending.message_posted = true;
        Ok(true)
    }

    fn take_pending_pairing_confirmation(
    ) -> Option<(PairingConfirmationRequest, Sender<Result<(), String>>)> {
        let Ok(mut pending) = pending_pairing_confirmation().lock() else {
            return None;
        };
        pending.message_posted = false;
        Some((pending.request.take()?, pending.responder.take()?))
    }

    fn clear_pending_pairing_confirmation() {
        if let Ok(mut pending) = pending_pairing_confirmation().lock() {
            pending.message_posted = false;
            pending.request = None;
            if let Some(responder) = pending.responder.take() {
                let _ = responder.send(Err("pairing confirmation was cancelled".to_string()));
            }
        }
    }

    fn has_pending_pairing_confirmation() -> bool {
        pending_pairing_confirmation()
            .lock()
            .is_ok_and(|pending| pending.request.is_some())
    }

    fn show_pending_pairing_confirmation(hwnd: HWND) {
        let Some((request, responder)) = take_pending_pairing_confirmation() else {
            return;
        };
        let _ = clear_pairing_notification(hwnd);
        let result = match confirm_pairing_request_owned(
            &PairingConfirmationModel {
                confirmation_code: request.confirmation_code,
                client_label: request.client_label,
            },
            hwnd,
        ) {
            Ok(true) => Ok(()),
            Ok(false) => Err("pairing confirmation was rejected".to_string()),
            Err(err) => {
                let message = format!("Failed to show pairing confirmation:\n\n{err}");
                let _ = show_error_window(&message);
                Err(err.to_string())
            }
        };
        let _ = responder.send(result);
    }

    fn show_context_menu(hwnd: HWND) {
        if let Err(err) = unsafe { show_context_menu_inner(hwnd) } {
            let _ = show_error_window(&format!("Failed to open tray menu:\n\n{err}"));
        }
    }

    unsafe fn show_context_menu_inner(hwnd: HWND) -> Result<()> {
        let menu = unsafe { CreatePopupMenu()? };
        let config_ready = TRAY_RUNTIME
            .get()
            .is_some_and(|runtime| is_pairing_ui_available(&runtime.config_path));
        let guarded_item_flags = if config_ready {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        unsafe {
            AppendMenuW(
                menu,
                guarded_item_flags,
                CMD_SHOW_MACHINE_INFO,
                w!("Machine info"),
            )?;
            AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())?;
            AppendMenuW(menu, MF_STRING, CMD_OPEN_SETTINGS, w!("Settings"))?;
            AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit"))?;
        }

        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point)? };
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        let command = unsafe {
            TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                None,
                hwnd,
                None,
            )
        };
        unsafe { DestroyMenu(menu)? };

        if command.0 != 0 {
            unsafe {
                PostMessageW(
                    Some(hwnd),
                    WM_COMMAND,
                    WPARAM(command.0 as usize),
                    LPARAM(0),
                )?;
            }
        }
        Ok(())
    }

    fn show_machine_info(hwnd: HWND) {
        let Some(runtime) = TRAY_RUNTIME.get() else {
            let _ = show_error_window("Tray runtime is not initialized.");
            return;
        };
        if !is_pairing_ui_available(&runtime.config_path) {
            let _ = show_error_window(CONFIG_NOT_READY_MESSAGE);
            return;
        }
        if let Err(err) = show_machine_info_window_owned(&runtime.config_path, hwnd) {
            let _ = show_error_window(&format!("Failed to show machine info:\n\n{err}"));
        }
    }

    fn open_settings(hwnd: HWND) {
        let Some(runtime) = TRAY_RUNTIME.get() else {
            let _ = show_error_window("Tray runtime is not initialized.");
            return;
        };
        if let Err(err) = open_config_file(hwnd, &runtime.config_path) {
            let _ = show_error_window(&format!("Failed to open settings:\n\n{err}"));
        }
    }

    fn open_config_file(hwnd: HWND, config_path: &Path) -> Result<()> {
        ensure_config_file_exists(config_path)?;
        let file = wide_path(config_path);
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                w!("open"),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let result_code = result.0 as isize;
        if result_code <= 32 {
            return Err(anyhow!("ShellExecuteW failed with code {result_code}"));
        }
        Ok(())
    }

    fn ensure_config_file_exists(config_path: &Path) -> Result<()> {
        if config_path.exists() {
            return Ok(());
        }
        let config = generated_default_system_config();
        save(config_path, &config)?;
        Ok(())
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn show_pairing_confirmation_notification(hwnd: HWND) -> Result<()> {
        let _ = clear_pairing_notification(hwnd);
        let mut data = notify_icon_data(hwnd);
        data.uFlags = NIF_INFO | NIF_REALTIME;
        data.dwInfoFlags = NIIF_INFO;
        data.Anonymous.uTimeout = 10_000;
        write_wide(&mut data.szInfoTitle, "Pairing requested");
        write_wide(
            &mut data.szInfo,
            "Click to choose the code shown on your client.",
        );

        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(())
    }

    fn show_pairing_notification(hwnd: HWND, notification: &PairingNotification) -> Result<()> {
        let _ = clear_pairing_notification(hwnd);
        let mut data = notify_icon_data(hwnd);
        data.uFlags = NIF_INFO | NIF_REALTIME;
        data.dwInfoFlags = NIIF_INFO;
        data.Anonymous.uTimeout = 10_000;
        write_wide(&mut data.szInfoTitle, "Pairing requested");
        write_wide(
            &mut data.szInfo,
            &format!(
                "Code: {}\nExpires in {} seconds",
                notification.pairing_code, notification.expires_in_seconds
            ),
        );

        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(())
    }

    fn clear_pairing_notification(hwnd: HWND) -> Result<()> {
        let mut data = notify_icon_data(hwnd);
        data.uFlags = NIF_INFO;
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(())
    }

    fn low_word(value: usize) -> u16 {
        (value & 0xffff) as u16
    }

    fn tray_event(lparam: LPARAM) -> u32 {
        low_word(lparam.0 as usize) as u32
    }

    fn write_wide<const N: usize>(buffer: &mut [u16; N], value: &str) {
        for (slot, unit) in buffer
            .iter_mut()
            .zip(value.encode_utf16().chain(std::iter::once(0)))
        {
            *slot = unit;
        }
    }
}
