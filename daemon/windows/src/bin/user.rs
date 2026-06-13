#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use wgo_daemon_core::config::windows_program_data_config_path;
use wgo_windows_daemon::tray::run_pairing_tray;

#[derive(Debug, Parser)]
#[command(name = "wgo-windows-user")]
#[command(about = "Windows user tray daemon for whats-going-on")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Args::parse()
        .command
        .unwrap_or(Command::Run { config: None })
    {
        Command::Run { config } => {
            run_pairing_tray(config.unwrap_or_else(windows_program_data_config_path))
        }
    }
}
