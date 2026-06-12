use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use time::OffsetDateTime;
use tracing::info;
use wgo_daemon_core::config::{
    load_or_generated_default, load_pairing_state_or_default, macos_system_config_path,
    pairing_state_path, save, save_pairing_state, SystemConfig,
};
use wgo_daemon_core::pairing::create_pairing_code;
use wgo_daemon_core::DEFAULT_LISTEN_ADDR;
use wgo_daemon_host::server::run_system_server;
use wgo_macos_daemon::fs::MacFileService;

#[derive(Debug, Parser)]
#[command(name = "wgo-macos-system")]
#[command(about = "macOS system daemon for whats-going-on")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long, default_value = DEFAULT_LISTEN_ADDR)]
        listen: SocketAddr,

        #[arg(long)]
        config: Option<PathBuf>,
    },
    Pair {
        #[arg(long, default_value = DEFAULT_LISTEN_ADDR)]
        listen: SocketAddr,

        #[arg(long)]
        config: Option<PathBuf>,

        #[arg(long)]
        url: Option<String>,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    Install,
    Uninstall,
    Start,
    Stop,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Args::parse().command {
        Command::Run { listen, config } => {
            run_system_server(
                listen,
                config.unwrap_or_else(macos_system_config_path),
                Arc::new(MacFileService::default()),
                None,
                "macOS system daemon",
            )
            .await
        }
        Command::Pair {
            listen,
            config,
            url,
        } => {
            let config_path = config.unwrap_or_else(macos_system_config_path);
            let mut config = load_or_generated_default(&config_path)?;
            config.listen_addr = listen.to_string();
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let pairing = create_pairing_code(now);
            save(&config_path, &config)?;
            let pairing_path = pairing_state_path(&config_path);
            let mut pairing_state = load_pairing_state_or_default(&pairing_path)?;
            pairing_state.pairing = Some(pairing.record.clone());
            save_pairing_state(&pairing_path, &pairing_state)?;
            let daemon_url = url.unwrap_or_else(|| default_pairing_url(&config, listen));
            println!("URL: {daemon_url}");
            println!("Pairing code: {}", pairing.code);
            println!("Expires at unix: {}", pairing.record.expires_at_unix);
            println!("Config: {}", config_path.display());
            println!("Pairing state: {}", pairing_path.display());
            Ok(())
        }
        Command::Service { command } => {
            info!(
                ?command,
                "service management is scaffolded for the macOS backend"
            );
            println!("service command {command:?} is scaffolded; LaunchDaemon integration is next");
            Ok(())
        }
    }
}

fn default_pairing_url(config: &SystemConfig, listen: SocketAddr) -> String {
    let port = listen.port();
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
    if listen.ip().is_unspecified() {
        return format!("https://localhost:{port}");
    }
    format!("https://{listen}")
}
