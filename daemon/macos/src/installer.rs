use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::pairing_ui::{show_confirmation_window, show_error_window, show_message_window};

const DEFAULT_APP_INSTALL_PATH: &str = "/Applications/Whats Going On.app";
const DEFAULT_SYSTEM_LABEL: &str = "com.disjukr.whats-going-on.system";
const DEFAULT_USER_LABEL: &str = "com.disjukr.whats-going-on.user";
const SYSTEM_DAEMON_PATH: &str = "/Library/Application Support/wgo/bin/wgo-macos-system";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupAction {
    Continue,
    Exit,
}

#[derive(Debug, Clone)]
struct InstallSettings {
    source_app: PathBuf,
    install_path: PathBuf,
    system_label: String,
    user_label: String,
}

impl InstallSettings {
    fn from_environment() -> Result<Option<Self>> {
        let Some(source_app) = source_app_bundle_path()? else {
            return Ok(None);
        };
        let install_path = env::var_os("WGO_APP_INSTALL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_install_path(&source_app));
        Ok(Some(Self {
            source_app,
            install_path,
            system_label: env::var("WGO_SYSTEM_LABEL")
                .unwrap_or_else(|_| DEFAULT_SYSTEM_LABEL.to_string()),
            user_label: env::var("WGO_USER_LABEL")
                .unwrap_or_else(|_| DEFAULT_USER_LABEL.to_string()),
        }))
    }

    fn system_plist_path(&self) -> PathBuf {
        PathBuf::from("/Library/LaunchDaemons").join(format!("{}.plist", self.system_label))
    }

    fn user_plist_path(&self) -> PathBuf {
        PathBuf::from("/Library/LaunchAgents").join(format!("{}.plist", self.user_label))
    }

    fn installed_system_plist_source(&self) -> PathBuf {
        self.install_path
            .join("Contents")
            .join("Resources")
            .join(format!("{}.plist", self.system_label))
    }

    fn installed_user_plist_source(&self) -> PathBuf {
        self.install_path
            .join("Contents")
            .join("Resources")
            .join(format!("{}.plist", self.user_label))
    }

    fn source_install_command(&self) -> PathBuf {
        self.source_app
            .join("Contents")
            .join("MacOS")
            .join("install")
    }

    fn source_uninstall_command(&self) -> PathBuf {
        self.source_app
            .join("Contents")
            .join("MacOS")
            .join("uninstall")
    }

    fn installed_uninstall_command(&self) -> PathBuf {
        self.install_path
            .join("Contents")
            .join("MacOS")
            .join("uninstall")
    }
}

pub fn ensure_installed_or_prompt() -> Result<StartupAction> {
    let Some(settings) = InstallSettings::from_environment()? else {
        return Ok(StartupAction::Continue);
    };

    if is_same_path(&settings.source_app, &settings.install_path)
        && launchd_install_is_ready(&settings)
    {
        return Ok(StartupAction::Continue);
    }

    if !is_same_path(&settings.source_app, &settings.install_path)
        && launchd_install_is_ready(&settings)
    {
        open_installed_app(&settings.install_path)?;
        return Ok(StartupAction::Exit);
    }

    let message = format!(
        "Whats Going On needs to install a system daemon to stay available after reboot.\n\n\
This will copy the app to:\n{}\n\n\
It will also add launchd jobs under /Library/LaunchDaemons and /Library/LaunchAgents. \
macOS will ask for an administrator password.",
        settings.install_path.display()
    );
    if !show_confirmation_window("Install Whats Going On?", &message)? {
        return Ok(StartupAction::Exit);
    }

    match install_with_administrator_privileges(&settings) {
        Ok(()) => {
            show_message_window(
                "Whats Going On Installed",
                "The daemon has been installed and registered with launchd.",
            )?;
            if !current_user_agent_is_loaded(&settings) {
                open_installed_app(&settings.install_path)?;
            }
            Ok(StartupAction::Exit)
        }
        Err(err) => {
            show_error_window(&format!("Failed to install Whats Going On:\n\n{err}"))?;
            Ok(StartupAction::Exit)
        }
    }
}

pub fn uninstall_or_prompt() -> Result<bool> {
    let Some(settings) = InstallSettings::from_environment()? else {
        show_error_window("Whats Going On is not running from an app bundle.")?;
        return Ok(false);
    };

    let message = "\
This will stop Whats Going On, unregister its launchd jobs, and remove the installed system daemon.\n\n\
The app bundle and configuration files will be left in place.";
    if !show_confirmation_window("Uninstall Whats Going On?", message)? {
        return Ok(false);
    }

    match uninstall_with_administrator_privileges(&settings) {
        Ok(()) => {
            show_message_window(
                "Whats Going On Uninstalled",
                "The launchd jobs and system daemon have been removed. You can move the app to the Trash.",
            )?;
            Ok(true)
        }
        Err(err) => {
            show_error_window(&format!("Failed to uninstall Whats Going On:\n\n{err}"))?;
            Ok(false)
        }
    }
}

fn source_app_bundle_path() -> Result<Option<PathBuf>> {
    if let Some(path) = env::var_os("WGO_APP_BUNDLE_PATH").map(PathBuf::from) {
        return Ok(Some(path));
    }

    let current_exe = env::current_exe().context("resolve current executable path")?;
    Ok(current_exe
        .ancestors()
        .find(|path| path.extension() == Some(OsStr::new("app")))
        .map(Path::to_path_buf))
}

fn default_install_path(source_app: &Path) -> PathBuf {
    source_app
        .file_name()
        .map(|file_name| PathBuf::from("/Applications").join(file_name))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_APP_INSTALL_PATH))
}

fn launchd_install_is_ready(settings: &InstallSettings) -> bool {
    settings.install_path.is_dir()
        && Path::new(SYSTEM_DAEMON_PATH).is_file()
        && settings.installed_system_plist_source().is_file()
        && settings.installed_user_plist_source().is_file()
        && settings.system_plist_path().is_file()
        && settings.user_plist_path().is_file()
        && launchctl_print(&format!("system/{}", settings.system_label))
}

fn current_user_agent_is_loaded(settings: &InstallSettings) -> bool {
    current_gui_domain()
        .map(|domain| launchctl_print(&format!("{domain}/{}", settings.user_label)))
        .unwrap_or(false)
}

fn launchctl_print(domain_label: &str) -> bool {
    Command::new("/bin/launchctl")
        .arg("print")
        .arg(domain_label)
        .status()
        .is_ok_and(|status| status.success())
}

fn current_gui_domain() -> Option<String> {
    let output = Command::new("/usr/bin/id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let uid = String::from_utf8(output.stdout).ok()?;
    let uid = uid.trim();
    if uid.is_empty() {
        None
    } else {
        Some(format!("gui/{uid}"))
    }
}

fn open_installed_app(app_path: &Path) -> Result<()> {
    let status = Command::new("/usr/bin/open")
        .arg(app_path)
        .status()
        .context("open installed app")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("open exited with status {status}"))
    }
}

fn install_with_administrator_privileges(settings: &InstallSettings) -> Result<()> {
    let install_command = settings.source_install_command();
    if !install_command.is_file() {
        return Err(anyhow!(
            "missing bundled installer: {}",
            install_command.display()
        ));
    }
    run_script_with_administrator_privileges(&install_command, "installation")
}

fn uninstall_with_administrator_privileges(settings: &InstallSettings) -> Result<()> {
    let uninstall_command = if settings.source_uninstall_command().is_file() {
        settings.source_uninstall_command()
    } else {
        settings.installed_uninstall_command()
    };
    if !uninstall_command.is_file() {
        return Err(anyhow!(
            "missing bundled uninstaller: {}",
            uninstall_command.display()
        ));
    }
    run_script_with_administrator_privileges(&uninstall_command, "uninstallation")
}

fn run_script_with_administrator_privileges(path: &Path, action: &str) -> Result<()> {
    let command = format!("/bin/bash {}", shell_quote(path));
    let apple_script = format!(
        "do shell script {} with administrator privileges",
        apple_script_string(&command)
    );

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(apple_script)
        .output()
        .with_context(|| format!("request administrator privileges for {action}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let details = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if details.is_empty() {
        Err(anyhow!("{action} exited with status {}", output.status))
    } else {
        Err(anyhow!(details))
    }
}

fn is_same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn shell_quote(value: impl AsRef<OsStr>) -> String {
    let value = value.as_ref().to_string_lossy();
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn apple_script_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            ch => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a b'c"), "'a b'\\''c'");
    }

    #[test]
    fn apple_script_string_escapes_quotes_and_backslashes() {
        assert_eq!(
            apple_script_string(r#"/bin/bash "/tmp/a\b""#),
            r#""/bin/bash \"/tmp/a\\b\"""#
        );
    }
}
