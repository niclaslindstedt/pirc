mod config;
pub mod registry;
pub mod user;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use tracing::{error, info, warn};

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

    let config = match config::ServerConfig::load(config_path.as_deref()) {
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

    // Initialize tracing subscriber
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let addr: SocketAddr = format!("{}:{}", config.network.bind_address, config.network.port)
        .parse()
        .unwrap_or_else(|e| {
            error!("invalid bind address: {e}");
            process::exit(1);
        });

    let listener = match Listener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind listener: {e}");
            process::exit(1);
        }
    };

    let local_addr = listener.local_addr().unwrap_or(addr);
    info!(%local_addr, "pircd starting");

    let (shutdown_controller, mut shutdown_signal) = ShutdownSignal::new();

    // Spawn Ctrl+C handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, initiating shutdown");
        shutdown_controller.shutdown();
    });

    // Accept loop
    loop {
        match listener.accept_with_shutdown(&mut shutdown_signal).await {
            Ok(Some((connection, peer_addr))) => {
                let conn_shutdown = shutdown_signal.clone();
                tokio::spawn(async move {
                    handle_connection(connection, peer_addr, conn_shutdown).await;
                });
            }
            Ok(None) => {
                info!("shutdown signal received, stopping accept loop");
                break;
            }
            Err(e) => {
                warn!("failed to accept connection: {e}");
            }
        }
    }

    info!("pircd shut down");
}

async fn handle_connection(
    mut connection: Connection,
    peer_addr: SocketAddr,
    mut shutdown: ShutdownSignal,
) {
    let conn_id = connection.info().id;
    info!(conn_id, %peer_addr, "handling connection");

    loop {
        match connection.recv_with_shutdown(&mut shutdown).await {
            Ok(Some(msg)) => {
                info!(conn_id, %peer_addr, %msg, "received message");
                // Echo the message back (temporary behavior)
                if let Err(e) = connection.send(msg).await {
                    warn!(conn_id, %peer_addr, "failed to send response: {e}");
                    break;
                }
            }
            Ok(None) => {
                info!(conn_id, %peer_addr, "connection closed");
                break;
            }
            Err(e) => {
                warn!(conn_id, %peer_addr, "error reading from connection: {e}");
                break;
            }
        }
    }
}
