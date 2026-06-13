use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use wgo_daemon_core::config::{
    client_credentials_path, load_or_generated_default, save, windows_program_data_config_path,
    SystemConfig,
};
use wgo_daemon_core::DEFAULT_LISTEN_ADDR;
use wgo_daemon_host::server::run_system_server;
use wgo_windows_daemon::fs::WindowsFileService;
use wgo_windows_daemon::ipc::UserTrayPairingNotifier;

#[derive(Debug, Parser)]
#[command(name = "wgo-windows-system")]
#[command(about = "Windows system daemon for whats-going-on")]
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
    Run {
        #[arg(long, default_value = DEFAULT_LISTEN_ADDR)]
        listen: SocketAddr,

        #[arg(long)]
        config: Option<PathBuf>,
    },
    Install {
        #[arg(long, default_value = DEFAULT_LISTEN_ADDR)]
        listen: SocketAddr,

        #[arg(long)]
        config: Option<PathBuf>,

        #[arg(long)]
        exe: Option<PathBuf>,
    },
    Uninstall,
    Start,
    Stop,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Args::parse().command {
        Command::Run { listen, config } => run_system_server_blocking(
            listen,
            config.unwrap_or_else(windows_program_data_config_path),
        ),
        Command::Pair {
            listen,
            config,
            url,
        } => {
            let config_path = config.unwrap_or_else(windows_program_data_config_path);
            let mut config = load_or_generated_default(&config_path)?;
            config.listen_addr = listen.to_string();
            save(&config_path, &config)?;
            let credentials_path = client_credentials_path(&config_path);
            let daemon_url = url.unwrap_or_else(|| default_pairing_url(&config, listen));
            println!("URL: {daemon_url}");
            println!("Config: {}", config_path.display());
            println!("Client credentials: {}", credentials_path.display());
            println!(
                "Start pairing from a client. Pairing codes are kept only in the running daemon process."
            );
            Ok(())
        }
        Command::Service { command } => handle_service_command(command),
    }
}

fn run_system_server_blocking(listen: SocketAddr, config_path: PathBuf) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_system_server(
        listen,
        config_path,
        Arc::new(WindowsFileService),
        Some(Arc::new(UserTrayPairingNotifier)),
        "Windows system daemon",
    ))
}

#[cfg(windows)]
fn handle_service_command(command: ServiceCommand) -> Result<()> {
    use wgo_windows_daemon::service::{
        install_service, run_dispatcher, start_service, stop_service, uninstall_service,
        ServiceRunOptions,
    };

    match command {
        ServiceCommand::Run { listen, config } => run_dispatcher(ServiceRunOptions::new(
            listen,
            config.unwrap_or_else(windows_program_data_config_path),
        )),
        ServiceCommand::Install {
            listen,
            config,
            exe,
        } => {
            let service_binary_path = match exe {
                Some(exe) => exe,
                None => std::env::current_exe()?,
            };
            install_service(
                service_binary_path,
                listen,
                config.unwrap_or_else(windows_program_data_config_path),
            )
        }
        ServiceCommand::Uninstall => uninstall_service(),
        ServiceCommand::Start => start_service(),
        ServiceCommand::Stop => stop_service(),
    }
}

#[cfg(not(windows))]
fn handle_service_command(_command: ServiceCommand) -> Result<()> {
    anyhow::bail!("Windows service management is only available on Windows")
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
