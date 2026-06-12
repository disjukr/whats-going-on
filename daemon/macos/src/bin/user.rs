use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use time::OffsetDateTime;
use wgo_daemon_core::config::{
    load_or_default, load_pairing_state_or_default, macos_system_config_path, pairing_state_path,
    save_pairing_state, SystemConfig,
};
use wgo_daemon_core::pairing::create_pairing_code;
use wgo_macos_daemon::pairing_ui::{show_pairing_window, PairingWindowModel};

#[derive(Debug, Parser)]
#[command(name = "wgo-macos-user")]
#[command(about = "macOS user daemon for whats-going-on")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
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
        Command::Run => {
            println!("wgo macOS user daemon scaffold running. Press Ctrl+C in the parent dev script to stop it.");
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        Command::PairingWindow { daemon_url, config } => {
            let config_path = config.unwrap_or_else(macos_system_config_path);
            let config = load_or_default(&config_path)?;
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let pairing = create_pairing_code(now);
            let pairing_path = pairing_state_path(&config_path);
            let mut pairing_state = load_pairing_state_or_default(&pairing_path)?;
            pairing_state.pairing = Some(pairing.record.clone());
            save_pairing_state(&pairing_path, &pairing_state)?;
            show_pairing_window(&PairingWindowModel {
                daemon_url: daemon_url.unwrap_or_else(|| default_daemon_url(&config)),
                pairing_code: pairing.code,
                expires_in_seconds: pairing.record.expires_at_unix - now,
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
