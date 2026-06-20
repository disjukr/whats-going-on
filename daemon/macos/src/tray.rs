use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use anyhow::{anyhow, Context, Result};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use wgo_daemon_core::config::{generated_default_system_config, load_or_default, save};

use crate::installer::uninstall_or_prompt;
use crate::ipc::{
    spawn_pairing_notification_server, PairingConfirmationRequest, PairingIpcRequest,
    PairingNotification,
};
use crate::pairing_ui::{
    confirm_pairing_request, show_error_window, show_machine_info_window, show_pairing_window,
    PairingConfirmationModel, PairingWindowModel,
};

const CMD_SHOW_MACHINE_INFO: &str = "show-machine-info";
const CMD_OPEN_SETTINGS: &str = "open-settings";
const CMD_UNINSTALL: &str = "uninstall";
const CMD_QUIT: &str = "quit";
const CONFIG_NOT_READY_MESSAGE: &str =
    "Machine config is not ready. Set a .ts.net domain or TLS certificate files first.";

enum UserEvent {
    Menu(MenuEvent),
    ShowCode(PairingNotification),
    Confirm(
        PairingConfirmationRequest,
        Sender<std::result::Result<(), String>>,
    ),
}

pub fn run_pairing_tray(config_path: PathBuf) -> Result<()> {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    event_loop.set_dock_visibility(false);
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }
    }));
    spawn_pairing_server(proxy);

    let menu = create_menu()?;
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Whats Going On")
        .with_icon(create_template_icon()?)
        .with_icon_as_template(true)
        .build()
        .context("create macOS menu bar icon")?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                tracing::info!("wgo macOS user tray daemon started");
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                handle_menu_event(&config_path, event.id(), control_flow);
            }
            Event::UserEvent(UserEvent::ShowCode(notification)) => {
                if let Err(err) = show_pairing_window(&PairingWindowModel {
                    daemon_url: notification.daemon_url,
                    pairing_code: notification.pairing_code,
                    expires_in_seconds: notification.expires_in_seconds,
                }) {
                    let _ = show_error_window(&format!("Failed to show pairing code:\n\n{err}"));
                }
            }
            Event::UserEvent(UserEvent::Confirm(request, responder)) => {
                let result = confirm_pairing_request(&PairingConfirmationModel {
                    confirmation_code: request.confirmation_code,
                    client_label: request.client_label,
                })
                .and_then(|accepted| {
                    if accepted {
                        Ok(())
                    } else {
                        Err(anyhow!("pairing confirmation was rejected"))
                    }
                })
                .map_err(|err| err.to_string());
                let _ = responder.send(result);
            }
            _ => {}
        }
    });
}

fn spawn_pairing_server(proxy: EventLoopProxy<UserEvent>) {
    spawn_pairing_notification_server(move |request| match request {
        PairingIpcRequest::ShowCode(notification) => proxy
            .send_event(UserEvent::ShowCode(notification))
            .map_err(|_| anyhow!("macOS tray event loop is closed")),
        PairingIpcRequest::Confirm(request) => {
            let (sender, receiver) = std::sync::mpsc::channel();
            proxy
                .send_event(UserEvent::Confirm(request, sender))
                .map_err(|_| anyhow!("macOS tray event loop is closed"))?;
            receiver
                .recv()
                .map_err(|_| anyhow!("pairing confirmation response channel closed"))?
                .map_err(anyhow::Error::msg)
        }
    });
}

fn create_menu() -> Result<Menu> {
    let menu = Menu::new();
    let machine_info = MenuItem::with_id(
        MenuId::new(CMD_SHOW_MACHINE_INFO),
        "Machine Info",
        true,
        None,
    );
    let settings = MenuItem::with_id(MenuId::new(CMD_OPEN_SETTINGS), "Settings", true, None);
    let uninstall = MenuItem::with_id(MenuId::new(CMD_UNINSTALL), "Uninstall...", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id(MenuId::new(CMD_QUIT), "Quit", true, None);
    menu.append_items(&[&machine_info, &settings, &uninstall, &separator, &quit])?;
    Ok(menu)
}

fn handle_menu_event(config_path: &Path, id: &MenuId, control_flow: &mut ControlFlow) {
    match id.as_ref() {
        CMD_SHOW_MACHINE_INFO => show_machine_info(config_path),
        CMD_OPEN_SETTINGS => {
            if let Err(err) = open_config_file(config_path) {
                let _ = show_error_window(&format!("Failed to open settings:\n\n{err}"));
            }
        }
        CMD_UNINSTALL => match uninstall_or_prompt() {
            Ok(true) => *control_flow = ControlFlow::Exit,
            Ok(false) => {}
            Err(err) => {
                let _ = show_error_window(&format!("Failed to uninstall:\n\n{err}"));
            }
        },
        CMD_QUIT => *control_flow = ControlFlow::Exit,
        _ => {}
    }
}

fn show_machine_info(config_path: &Path) {
    if !is_pairing_ui_available(config_path) {
        let _ = show_error_window(CONFIG_NOT_READY_MESSAGE);
        return;
    }
    if let Err(err) = show_machine_info_window(config_path) {
        let _ = show_error_window(&format!("Failed to show machine info:\n\n{err}"));
    }
}

fn is_pairing_ui_available(config_path: &Path) -> bool {
    load_or_default(config_path).is_ok_and(|config| {
        config
            .domain
            .as_deref()
            .map(str::trim)
            .is_some_and(|domain| !domain.is_empty())
            || config.tls.is_some()
    })
}

fn open_config_file(config_path: &Path) -> Result<()> {
    ensure_config_file_exists(config_path)?;
    let status = std::process::Command::new("open")
        .arg(config_path)
        .status()
        .context("run open for macOS config file")?;
    if !status.success() {
        return Err(anyhow!("open exited with status {status}"));
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

fn create_template_icon() -> Result<Icon> {
    const SIZE: u32 = 64;
    const WGO_TRAY_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wgo-tray.rgba"));

    let rgba = WGO_TRAY_RGBA.to_vec();
    if rgba.len() != (SIZE * SIZE * 4) as usize {
        return Err(anyhow!("generated macOS tray icon has an invalid size"));
    }
    Icon::from_rgba(rgba, SIZE, SIZE).map_err(Into::into)
}
