use std::net::SocketAddr;
use std::path::Path;

use anyhow::Result;
use wgo_daemon_core::config::{load_or_default, SystemConfig};

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

pub fn show_pairing_window(model: &PairingWindowModel) -> Result<()> {
    show_info_window(
        "wgo pairing code",
        &format!(
            "URL: {}\n\nPairing code: {}\nExpires in: {} seconds",
            model.daemon_url, model.pairing_code, model.expires_in_seconds
        ),
    )
}

pub fn confirm_pairing_request(model: &PairingConfirmationModel) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        return Ok(macos_alert::show_confirm(
            "wgo pairing request",
            &format!(
                "Client: {}\nConfirmation code: {}\n\nAllow this client to pair with this machine?",
                model.client_label, model.confirmation_code
            ),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!(
            "wgo pairing request\nClient: {}\nConfirmation code: {}",
            model.client_label, model.confirmation_code
        );
        Ok(false)
    }
}

pub fn show_confirmation_window(title: &str, message: &str) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        return Ok(macos_alert::show_confirm(title, message));
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!("{title}\n{message}");
        Ok(false)
    }
}

pub fn show_message_window(title: &str, message: &str) -> Result<()> {
    show_info_window(title, message)
}

pub fn show_machine_info_window(config_path: &Path) -> Result<()> {
    let config = load_or_default(config_path)?;
    show_info_window(
        "wgo machine info",
        &machine_info_message(&config, config_path),
    )
}

pub fn show_error_window(message: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos_alert::show_message("wgo", message, macos_alert::AlertKind::Error);
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("wgo\n{message}");
        Ok(())
    }
}

fn show_info_window(title: &str, message: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos_alert::show_message(title, message, macos_alert::AlertKind::Info);
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!("{title}\n{message}");
        Ok(())
    }
}

fn machine_info_message(config: &SystemConfig, config_path: &Path) -> String {
    format!(
        "URL: {}\nConfig: {}",
        default_daemon_url(config),
        config_path.display()
    )
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

#[cfg(target_os = "macos")]
mod macos_alert {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAlertStyle, NSApplication,
        NSModalPanelWindowLevel,
    };
    use objc2_foundation::NSString;

    pub enum AlertKind {
        Info,
        Error,
    }

    pub fn show_message(title: &str, message: &str, kind: AlertKind) {
        let response = run_alert(title, message, kind, &["OK"]);
        let _ = response;
    }

    pub fn show_confirm(title: &str, message: &str) -> bool {
        run_alert(title, message, AlertKind::Info, &["Yes", "No"]) == NSAlertFirstButtonReturn
    }

    fn run_alert(
        title: &str,
        message: &str,
        kind: AlertKind,
        buttons: &[&str],
    ) -> objc2_app_kit::NSModalResponse {
        let mtm =
            MainThreadMarker::new().expect("macOS pairing UI must be shown on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        let alert = NSAlert::new(mtm);
        let style = match kind {
            AlertKind::Info => NSAlertStyle::Informational,
            AlertKind::Error => NSAlertStyle::Critical,
        };
        alert.setAlertStyle(style);

        for button in buttons {
            let title = NSString::from_str(button);
            alert.addButtonWithTitle(&title);
        }

        let title = NSString::from_str(title);
        let message = NSString::from_str(message);
        alert.setMessageText(&title);
        alert.setInformativeText(&message);

        let window = alert.window();
        window.center();
        window.setLevel(NSModalPanelWindowLevel);
        window.makeKeyAndOrderFront(None);
        window.orderFrontRegardless();

        let response = alert.runModal();
        if response != NSAlertFirstButtonReturn && buttons.len() == 2 {
            return NSAlertSecondButtonReturn;
        }
        response
    }
}
