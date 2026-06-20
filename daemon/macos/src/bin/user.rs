use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use wgo_daemon_core::config::{load_or_default, macos_system_config_path, SystemConfig};
use wgo_macos_daemon::installer::{ensure_installed_or_prompt, StartupAction};
use wgo_macos_daemon::pairing_ui::{show_pairing_window, PairingWindowModel};
use wgo_macos_daemon::tray::run_pairing_tray;

#[derive(Debug, Parser)]
#[command(name = "wgo-macos-user")]
#[command(about = "macOS user daemon for whats-going-on")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    PairingWindow {
        #[arg(long)]
        daemon_url: Option<String>,

        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Args::parse().command {
        Command::Run { config } => {
            if ensure_installed_or_prompt()? == StartupAction::Exit {
                return Ok(());
            }
            run_pairing_tray(config.unwrap_or_else(macos_system_config_path))
        }
        Command::PairingWindow { daemon_url, config } => {
            let config_path = config.unwrap_or_else(macos_system_config_path);
            let config = load_or_default(&config_path)?;
            show_pairing_window(&PairingWindowModel {
                daemon_url: daemon_url.unwrap_or_else(|| default_daemon_url(&config)),
                pairing_code: "Start pairing from a client".to_string(),
                expires_in_seconds: 0,
            })
        }
    }
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
