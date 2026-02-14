use std::net::SocketAddr;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownController, ShutdownSignal};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::handler::{self, HandleResult, PreRegistrationState};
use pirc_server::raft::transport::{PeerConnections, PeerMap};
use pirc_server::raft::{FileStorage, RaftBuilder, RaftHandle};
use pirc_server::registry::UserRegistry;
use tokio::sync::{mpsc, Mutex};
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

    let config = match ServerConfig::load(config_path.as_deref()) {
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
    let shutdown_controller = Arc::new(shutdown_controller);

    // Spawn Ctrl+C handler
    {
        let ctrl_c_shutdown = Arc::clone(&shutdown_controller);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("received Ctrl+C, initiating shutdown");
            ctrl_c_shutdown.shutdown();
        });
    }

    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());

    // Raft cluster initialization
    let mut _raft_shutdown: Option<pirc_server::raft::ShutdownSender> = None;
    let mut _raft_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let raft_handle: Option<Arc<RaftHandle<String>>> = if config.cluster.enabled {
        match init_raft_cluster(&config, &shutdown_signal).await {
            Ok((handle, raft_shutdown_sender, handles)) => {
                _raft_shutdown = Some(raft_shutdown_sender);
                _raft_handles = handles;
                Some(Arc::new(handle))
            }
            Err(e) => {
                error!("failed to initialize raft cluster: {e}");
                process::exit(1);
            }
        }
    } else {
        None
    };
    let _ = &raft_handle; // suppress unused warning when cluster is disabled

    let config = Arc::new(config);

    // Accept loop
    loop {
        match listener.accept_with_shutdown(&mut shutdown_signal).await {
            Ok(Some((connection, peer_addr))) => {
                let conn_shutdown = shutdown_signal.clone();
                let conn_registry = Arc::clone(&registry);
                let conn_channels = Arc::clone(&channels);
                let conn_config = Arc::clone(&config);
                let conn_shutdown_controller = Arc::clone(&shutdown_controller);
                tokio::spawn(async move {
                    handle_connection(
                        connection,
                        peer_addr,
                        conn_shutdown,
                        conn_registry,
                        conn_channels,
                        conn_config,
                        conn_shutdown_controller,
                    )
                    .await;
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

/// Keepalive intervals for PING/PONG.
const PING_INTERVAL: Duration = Duration::from_secs(120);
const PING_TIMEOUT: Duration = Duration::from_secs(60);

async fn handle_connection(
    mut connection: Connection,
    peer_addr: SocketAddr,
    mut shutdown: ShutdownSignal,
    registry: Arc<UserRegistry>,
    channels: Arc<ChannelRegistry>,
    config: Arc<ServerConfig>,
    shutdown_controller: Arc<ShutdownController>,
) {
    let conn_id = connection.info().id;
    info!(conn_id, %peer_addr, "handling connection");

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let mut state = PreRegistrationState::new(peer_addr.ip().to_string());

    let mut ping_interval = tokio::time::interval(PING_INTERVAL);
    ping_interval.tick().await; // Consume the immediate first tick
    let mut ping_pending = false;
    let ping_timeout = tokio::time::sleep(PING_TIMEOUT);
    tokio::pin!(ping_timeout);

    loop {
        tokio::select! {
            result = connection.recv_with_shutdown(&mut shutdown) => {
                match result {
                    Ok(Some(msg)) => {
                        info!(conn_id, %peer_addr, %msg, "received message");

                        // Clear pending ping on any PONG response.
                        if msg.command == Command::Pong {
                            ping_pending = false;
                        }

                        let handle_result = handler::handle_message(
                            &msg, conn_id, &registry, &channels, &tx, &mut state, &config,
                        );

                        // Drain all queued outbound messages after handling
                        while let Ok(out_msg) = rx.try_recv() {
                            if let Err(e) = connection.send(out_msg).await {
                                warn!(conn_id, %peer_addr, "failed to send response: {e}");
                                // Clean up on send failure
                                if state.registered {
                                    registry.remove_by_connection(conn_id);
                                }
                                return;
                            }
                        }

                        match handle_result {
                            HandleResult::Quit => {
                                info!(conn_id, %peer_addr, "client quit");
                                return;
                            }
                            HandleResult::Shutdown => {
                                info!(conn_id, %peer_addr, "operator initiated server shutdown");
                                shutdown_controller.shutdown();
                                return;
                            }
                            HandleResult::Continue => {}
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

            _ = ping_interval.tick() => {
                // Send a server PING for keepalive.
                let ping = Message::builder(Command::Ping)
                    .trailing("pircd")
                    .build();
                if let Err(e) = connection.send(ping).await {
                    warn!(conn_id, %peer_addr, "failed to send PING: {e}");
                    break;
                }
                ping_pending = true;
                ping_timeout.as_mut().reset(tokio::time::Instant::now() + PING_TIMEOUT);
            }

            _ = &mut ping_timeout, if ping_pending => {
                warn!(conn_id, %peer_addr, "ping timeout, closing connection");
                // Send ERROR before closing.
                let error_msg = Message::builder(Command::Error)
                    .trailing(&format!("Closing Link: {} (Ping timeout)", peer_addr.ip()))
                    .build();
                let _ = connection.send(error_msg).await;
                break;
            }
        }
    }

    // Clean up: remove user from registry on disconnect.
    if state.registered {
        registry.remove_by_connection(conn_id);
    }
}

/// Initialize the Raft consensus engine when clustering is enabled.
///
/// Sets up file storage, builds the Raft driver and handle, spawns the driver
/// task, creates the transport bridge (outbound sender + peer listener), and
/// returns the handle and shutdown sender.
async fn init_raft_cluster(
    config: &ServerConfig,
    shutdown_signal: &ShutdownSignal,
) -> Result<
    (
        RaftHandle<String>,
        pirc_server::raft::ShutdownSender,
        Vec<tokio::task::JoinHandle<()>>,
    ),
    Box<dyn std::error::Error>,
> {
    let raft_config = config
        .cluster
        .to_raft_config()
        .ok_or("cluster enabled but raft config could not be built")?;

    let data_dir = config.cluster.raft_data_dir();
    info!(?data_dir, node_id = %raft_config.node_id, "initializing raft cluster");

    // Ensure data directory exists.
    std::fs::create_dir_all(&data_dir)?;

    let storage = FileStorage::new(&data_dir).await?;

    let (mut driver, handle, shutdown_sender, inbound_tx, outbound_rx) =
        RaftBuilder::<String, FileStorage>::new()
            .config(raft_config)
            .storage(storage)
            .build()
            .await?;

    let mut handles = Vec::new();

    // Parse peer addresses for the transport layer.
    let peer_entries = config.cluster.parse_peers();
    let peer_map_entries: Vec<_> = peer_entries
        .iter()
        .filter_map(|entry| {
            entry
                .address
                .parse::<SocketAddr>()
                .ok()
                .map(|addr| (entry.node_id, addr))
        })
        .collect();
    let peer_map = PeerMap::new(peer_map_entries);

    // Spawn the outbound transport task.
    let peer_conns = Arc::new(Mutex::new(PeerConnections::new(peer_map.clone())));
    let outbound_handle =
        pirc_server::raft::transport::spawn_outbound_transport(outbound_rx, peer_conns);
    handles.push(outbound_handle);

    // Spawn the peer listener if a raft_port is configured.
    if let Some(raft_port) = config.cluster.raft_port {
        let listen_addr: SocketAddr =
            format!("{}:{raft_port}", config.network.bind_address)
                .parse()
                .unwrap_or_else(|_| {
                    format!("0.0.0.0:{raft_port}").parse().unwrap()
                });

        match Listener::bind(listen_addr).await {
            Ok(listener) => {
                info!(%listen_addr, "raft peer listener bound");
                let listener_handle =
                    pirc_server::raft::transport::spawn_peer_listener::<String>(
                        listener,
                        inbound_tx,
                        &peer_map,
                        shutdown_signal.clone(),
                    );
                handles.push(listener_handle);
            }
            Err(e) => {
                error!(%listen_addr, error = %e, "failed to bind raft peer listener");
                return Err(Box::new(e));
            }
        }
    }

    // Spawn the Raft driver task.
    let driver_handle = tokio::spawn(async move {
        driver.run().await;
    });
    handles.push(driver_handle);

    info!("raft cluster initialized");
    Ok((handle, shutdown_sender, handles))
}
