//! pirc — a modern IRC client with end-to-end encryption.
//!
//! This binary provides a terminal-based IRC client with:
//!
//! - TUI interface built with ratatui
//! - End-to-end encrypted messaging via the triple ratchet protocol
//! - P2P direct connections with NAT traversal
//! - Encrypted group chat support
//! - mIRC-compatible scripting engine
//! - Native plugin system

mod app;
pub mod client_command;
pub mod command_parser;
mod config;
mod connection_state;
pub mod encryption;
pub mod group_chat;
mod message_handler;
pub mod p2p;
pub mod p2p_crypto;
mod registration;
mod tui;

use app::App;
use config::ClientConfig;
use std::path::PathBuf;
use std::process;
use tracing_subscriber::prelude::*;

fn parse_config_path() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            if i + 1 < args.len() {
                return Some(PathBuf::from(&args[i + 1]));
            }
            eprintln!("error: --config requires a path argument");
            process::exit(1);
        }
        i += 1;
    }
    None
}

#[tokio::main]
async fn main() {
    let config_path = parse_config_path();

    let config = match ClientConfig::load(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = config.validate() {
        eprintln!("error: {e}");
        process::exit(1);
    }

    // Initialize file-only logging (no stdout — TUI would corrupt terminal).
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    let log_dir = home.join(".pirc").join("logs").join("client");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_filename = format!("{}.log", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let file_appender = tracing_appender::rolling::never(&log_dir, &log_filename);
    let (non_blocking_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking_writer)
        .with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        );
    tracing_subscriber::registry().with(file_layer).init();

    let app = App::new(config);

    if let Err(e) = app.run().await {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
