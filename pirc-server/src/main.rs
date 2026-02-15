use std::net::SocketAddr;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Connector, Listener, ShutdownController, ShutdownSignal};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::cluster::{ClusterService, InviteKeyStore, PersistedClusterState, PersistedPeer};
use pirc_server::config::{ClusterStartupMode, ServerConfig};
use pirc_server::handler::{self, HandleResult, PreRegistrationState};
use pirc_server::handler_cluster::ClusterContext;
use pirc_server::raft::rpc::RaftMessage;
use pirc_server::raft::transport::{PeerConnections, PeerMap, PeerUpdater, SharedPeerMap};
use pirc_server::raft::{ClusterCommand, FileStorage, NodeId, NullStateMachine, RaftBuilder, RaftHandle};
use pirc_server::registry::UserRegistry;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

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

/// Aggregated cluster state returned by [`init_raft_cluster`].
///
/// All fields are held alive for the lifetime of the server. The
/// `cluster_context` is shared with connection handlers for operator commands.
#[allow(dead_code)]
struct ClusterState {
    raft_handle: Arc<RaftHandle<ClusterCommand>>,
    cluster_service: Arc<ClusterService>,
    cluster_context: Arc<ClusterContext>,
    _raft_shutdown: pirc_server::raft::ShutdownSender,
    _task_handles: Vec<tokio::task::JoinHandle<()>>,
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
    let cluster_state: Option<ClusterState> = if config.cluster.enabled {
        match init_raft_cluster(&config, &shutdown_signal).await {
            Ok(state) => Some(state),
            Err(e) => {
                error!("failed to initialize raft cluster: {e}");
                process::exit(1);
            }
        }
    } else {
        None
    };

    let cluster_ctx: Option<Arc<ClusterContext>> =
        cluster_state.as_ref().map(|s| Arc::clone(&s.cluster_context));
    // Keep cluster_state alive for the lifetime of the server.
    let _cluster_state = cluster_state;

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
                let conn_cluster_ctx = cluster_ctx.clone();
                tokio::spawn(async move {
                    handle_connection(
                        connection,
                        peer_addr,
                        conn_shutdown,
                        conn_registry,
                        conn_channels,
                        conn_config,
                        conn_shutdown_controller,
                        conn_cluster_ctx,
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
    cluster_ctx: Option<Arc<ClusterContext>>,
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
                            cluster_ctx.as_ref().map(|c| c.as_ref()),
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
/// Supports three startup modes:
/// - **Bootstrap**: single-node cluster as master, ready to accept joins.
/// - **Join**: connect to an existing cluster node, send `CLUSTER JOIN`, configure from `WELCOME`.
/// - **Rejoin**: restart with existing Raft state and static peer config.
///
/// Returns a [`ClusterState`] with the Raft handle, cluster service, and
/// background task handles.
async fn init_raft_cluster(
    config: &ServerConfig,
    shutdown_signal: &ShutdownSignal,
) -> Result<ClusterState, Box<dyn std::error::Error>> {
    let mode = config.cluster.startup_mode();
    info!(?mode, "cluster startup mode");

    match mode {
        ClusterStartupMode::Bootstrap => init_bootstrap(config, shutdown_signal).await,
        ClusterStartupMode::Join => init_join(config, shutdown_signal).await,
        ClusterStartupMode::Rejoin => init_rejoin(config, shutdown_signal).await,
    }
}

/// Bootstrap a new single-node cluster as master.
async fn init_bootstrap(
    config: &ServerConfig,
    shutdown_signal: &ShutdownSignal,
) -> Result<ClusterState, Box<dyn std::error::Error>> {
    let raft_config = config
        .cluster
        .to_bootstrap_raft_config()
        .ok_or("bootstrap mode requires cluster.node_id")?;

    let node_id = raft_config.node_id;
    let data_dir = config.cluster.raft_data_dir();
    info!(?data_dir, %node_id, "bootstrapping new cluster");

    std::fs::create_dir_all(&data_dir)?;
    let storage = FileStorage::new(&data_dir).await?;

    let (mut driver, handle, shutdown_sender, inbound_tx, outbound_rx) =
        RaftBuilder::<ClusterCommand, FileStorage, NullStateMachine>::new()
            .config(raft_config)
            .storage(storage)
            .state_machine(NullStateMachine)
            .build()
            .await?;

    let handle = Arc::new(handle);
    let mut handles = Vec::new();

    // Empty peer map — single-node bootstrap has no peers initially.
    let peer_map = PeerMap::new(vec![]);
    let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
    let peer_conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));

    let outbound_handle = pirc_server::raft::transport::spawn_outbound_transport(
        outbound_rx,
        Arc::clone(&peer_conns),
    );
    handles.push(outbound_handle);

    let peer_updater = PeerUpdater::new(Arc::clone(&shared_peer_map), Arc::clone(&peer_conns));

    // Bind the cluster port listener for Raft messages and CLUSTER JOIN requests.
    if let Some(raft_port) = config.cluster.raft_port {
        let listen_addr = cluster_listen_addr(config, raft_port);
        let listener = Listener::bind(listen_addr).await?;
        info!(%listen_addr, "cluster port listener bound (bootstrap)");

        let listener_handle = spawn_cluster_listener(
            listener,
            inbound_tx,
            Arc::clone(&shared_peer_map),
            shutdown_signal.clone(),
        );
        handles.push(listener_handle);
    }

    // Spawn the Raft driver.
    let driver_handle = tokio::spawn(async move {
        driver.run().await;
    });
    handles.push(driver_handle);

    // Create the invite key store and cluster service.
    let self_addr = cluster_self_addr(config);
    let invite_keys = Arc::new(Mutex::new(InviteKeyStore::new()));
    let next_node_id_start = node_id.as_u64() + 1000;
    let cluster_service = Arc::new(ClusterService::new(
        Arc::clone(&invite_keys),
        Arc::clone(&handle),
        peer_updater,
        Arc::clone(&shared_peer_map),
        node_id,
        self_addr,
        next_node_id_start,
    ));

    // Persist initial cluster state so we can rejoin later.
    let persisted = PersistedClusterState::new_bootstrap(node_id, self_addr);
    persisted.save(&data_dir)?;
    info!("persisted initial cluster state");

    let cluster_context = Arc::new(ClusterContext {
        invite_keys: Arc::clone(&invite_keys),
        raft_handle: Arc::clone(&handle),
        shared_peer_map: Arc::clone(&shared_peer_map),
        self_id: node_id,
    });

    info!("cluster bootstrapped as master");
    Ok(ClusterState {
        raft_handle: handle,
        cluster_service,
        cluster_context,
        _raft_shutdown: shutdown_sender,
        _task_handles: handles,
    })
}

/// Join an existing cluster using an invite key.
async fn init_join(
    config: &ServerConfig,
    shutdown_signal: &ShutdownSignal,
) -> Result<ClusterState, Box<dyn std::error::Error>> {
    let invite_key = config
        .cluster
        .invite_key
        .as_ref()
        .ok_or("join mode requires cluster.invite_key")?;
    let join_address: SocketAddr = config
        .cluster
        .join_address
        .as_ref()
        .ok_or("join mode requires cluster.join_address")?
        .parse()
        .map_err(|e| format!("invalid cluster.join_address: {e}"))?;

    info!(%join_address, "joining existing cluster");

    // Connect to the existing cluster node and send CLUSTER JOIN.
    let connector = Connector::new();
    let mut connection = connector.connect(join_address).await?;

    let join_msg = ClusterService::build_join_message(invite_key);
    connection.send(join_msg).await?;

    // Wait for CLUSTER WELCOME response.
    let response = connection
        .recv()
        .await?
        .ok_or("connection closed before receiving welcome")?;

    let (assigned_id, topology) = ClusterService::parse_welcome_message(&response)
        .map_err(|e| format!("failed to parse welcome: {e}"))?;

    info!(%assigned_id, peers = topology.peers.len(), "received cluster welcome");

    // Build Raft config with the assigned node ID and peers from the topology.
    let peer_map_entries: Vec<_> = topology
        .peers
        .iter()
        .filter(|p| NodeId::new(p.id) != assigned_id)
        .map(|p| (NodeId::new(p.id), p.addr))
        .collect();
    let peer_ids: Vec<NodeId> = peer_map_entries.iter().map(|(id, _)| *id).collect();

    let raft_config = pirc_server::raft::RaftConfig {
        election_timeout_min: Duration::from_millis(
            config.cluster.election_timeout_min_ms.unwrap_or(150),
        ),
        election_timeout_max: Duration::from_millis(
            config.cluster.election_timeout_max_ms.unwrap_or(300),
        ),
        heartbeat_interval: Duration::from_millis(
            config.cluster.heartbeat_interval_ms.unwrap_or(50),
        ),
        node_id: assigned_id,
        peers: peer_ids,
        ..pirc_server::raft::RaftConfig::default()
    };

    let data_dir = config.cluster.raft_data_dir();
    std::fs::create_dir_all(&data_dir)?;
    let storage = FileStorage::new(&data_dir).await?;

    let (mut driver, handle, shutdown_sender, inbound_tx, outbound_rx) =
        RaftBuilder::<ClusterCommand, FileStorage, NullStateMachine>::new()
            .config(raft_config)
            .storage(storage)
            .state_machine(NullStateMachine)
            .build()
            .await?;

    let handle = Arc::new(handle);
    let mut handles = Vec::new();

    // Build persisted peers before peer_map_entries is consumed.
    let persisted_peers: Vec<PersistedPeer> = peer_map_entries
        .iter()
        .map(|(id, addr)| PersistedPeer {
            id: *id,
            addr: *addr,
        })
        .collect();

    let peer_map = PeerMap::new(peer_map_entries);
    let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
    let peer_conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));

    let outbound_handle = pirc_server::raft::transport::spawn_outbound_transport(
        outbound_rx,
        Arc::clone(&peer_conns),
    );
    handles.push(outbound_handle);

    let peer_updater = PeerUpdater::new(Arc::clone(&shared_peer_map), Arc::clone(&peer_conns));

    // Bind cluster port listener.
    if let Some(raft_port) = config.cluster.raft_port {
        let listen_addr = cluster_listen_addr(config, raft_port);
        let listener = Listener::bind(listen_addr).await?;
        info!(%listen_addr, "cluster port listener bound (join)");

        let listener_handle = spawn_cluster_listener(
            listener,
            inbound_tx,
            Arc::clone(&shared_peer_map),
            shutdown_signal.clone(),
        );
        handles.push(listener_handle);
    }

    let driver_handle = tokio::spawn(async move {
        driver.run().await;
    });
    handles.push(driver_handle);

    let self_addr = cluster_self_addr(config);
    let invite_keys = Arc::new(Mutex::new(InviteKeyStore::new()));
    let next_node_id_start = assigned_id.as_u64() + 1000;
    let cluster_service = Arc::new(ClusterService::new(
        Arc::clone(&invite_keys),
        Arc::clone(&handle),
        peer_updater,
        Arc::clone(&shared_peer_map),
        assigned_id,
        self_addr,
        next_node_id_start,
    ));

    // Persist cluster state so we can rejoin later without an invite key.
    let persisted = PersistedClusterState::new_from_join(assigned_id, self_addr, persisted_peers);
    persisted.save(&data_dir)?;
    info!("persisted cluster state after join");

    let cluster_context = Arc::new(ClusterContext {
        invite_keys: Arc::clone(&invite_keys),
        raft_handle: Arc::clone(&handle),
        shared_peer_map: Arc::clone(&shared_peer_map),
        self_id: assigned_id,
    });

    info!("joined cluster successfully");
    Ok(ClusterState {
        raft_handle: handle,
        cluster_service,
        cluster_context,
        _raft_shutdown: shutdown_sender,
        _task_handles: handles,
    })
}

/// Rejoin with existing persisted Raft state and static peer config.
///
/// If a persisted `cluster_state.json` exists in the data directory, it is
/// used for node ID and peer configuration instead of the static config file.
async fn init_rejoin(
    config: &ServerConfig,
    shutdown_signal: &ShutdownSignal,
) -> Result<ClusterState, Box<dyn std::error::Error>> {
    let data_dir = config.cluster.raft_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    // Try to load persisted cluster state first.
    let persisted = PersistedClusterState::load(&data_dir)?;

    let (node_id, self_addr, peer_map_entries, next_node_id_start) = if let Some(ref state) =
        persisted
    {
        info!(
            node_id = %state.node_id,
            generation = state.generation,
            peers = state.peers.len(),
            "loaded persisted cluster state"
        );
        let entries: Vec<_> = state.peers.iter().map(|p| (p.id, p.addr)).collect();
        (
            state.node_id,
            state.self_addr,
            entries,
            state.next_node_id,
        )
    } else {
        info!("no persisted cluster state found, using config file");
        let raft_cfg = config
            .cluster
            .to_raft_config()
            .ok_or("rejoin mode requires cluster.node_id")?;
        let nid = raft_cfg.node_id;
        let peer_entries = config.cluster.parse_peers();
        let entries: Vec<_> = peer_entries
            .iter()
            .filter_map(|entry| {
                entry
                    .address
                    .parse::<SocketAddr>()
                    .ok()
                    .map(|addr| (entry.node_id, addr))
            })
            .collect();
        let sa = cluster_self_addr(config);
        (nid, sa, entries, nid.as_u64() + 1000)
    };

    info!(?data_dir, %node_id, "rejoining cluster");

    // Build raft config with peer IDs.
    let peer_ids: Vec<NodeId> = peer_map_entries.iter().map(|(id, _)| *id).collect();
    let raft_config = pirc_server::raft::RaftConfig {
        election_timeout_min: Duration::from_millis(
            config.cluster.election_timeout_min_ms.unwrap_or(150),
        ),
        election_timeout_max: Duration::from_millis(
            config.cluster.election_timeout_max_ms.unwrap_or(300),
        ),
        heartbeat_interval: Duration::from_millis(
            config.cluster.heartbeat_interval_ms.unwrap_or(50),
        ),
        node_id,
        peers: peer_ids,
        ..pirc_server::raft::RaftConfig::default()
    };

    let storage = FileStorage::new(&data_dir).await?;

    let (mut driver, handle, shutdown_sender, inbound_tx, outbound_rx) =
        RaftBuilder::<ClusterCommand, FileStorage, NullStateMachine>::new()
            .config(raft_config)
            .storage(storage)
            .state_machine(NullStateMachine)
            .build()
            .await?;

    let handle = Arc::new(handle);
    let mut handles = Vec::new();

    let peer_map = PeerMap::new(peer_map_entries);
    let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
    let peer_conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));

    let outbound_handle = pirc_server::raft::transport::spawn_outbound_transport(
        outbound_rx,
        Arc::clone(&peer_conns),
    );
    handles.push(outbound_handle);

    let peer_updater = PeerUpdater::new(Arc::clone(&shared_peer_map), Arc::clone(&peer_conns));

    if let Some(raft_port) = config.cluster.raft_port {
        let listen_addr = cluster_listen_addr(config, raft_port);
        let listener = Listener::bind(listen_addr).await?;
        info!(%listen_addr, "cluster port listener bound (rejoin)");

        let listener_handle = spawn_cluster_listener(
            listener,
            inbound_tx,
            Arc::clone(&shared_peer_map),
            shutdown_signal.clone(),
        );
        handles.push(listener_handle);
    }

    let driver_handle = tokio::spawn(async move {
        driver.run().await;
    });
    handles.push(driver_handle);

    // Load persisted invite keys if available.
    let invite_store = InviteKeyStore::load(&data_dir).unwrap_or_default();
    let invite_keys = Arc::new(Mutex::new(invite_store));
    let cluster_service = Arc::new(ClusterService::new(
        Arc::clone(&invite_keys),
        Arc::clone(&handle),
        peer_updater,
        Arc::clone(&shared_peer_map),
        node_id,
        self_addr,
        next_node_id_start,
    ));

    let cluster_context = Arc::new(ClusterContext {
        invite_keys: Arc::clone(&invite_keys),
        raft_handle: Arc::clone(&handle),
        shared_peer_map: Arc::clone(&shared_peer_map),
        self_id: node_id,
    });

    info!("cluster rejoin initialized");
    Ok(ClusterState {
        raft_handle: handle,
        cluster_service,
        cluster_context,
        _raft_shutdown: shutdown_sender,
        _task_handles: handles,
    })
}

/// Compute the listen address for the cluster port.
fn cluster_listen_addr(config: &ServerConfig, raft_port: u16) -> SocketAddr {
    format!("{}:{raft_port}", config.network.bind_address)
        .parse()
        .unwrap_or_else(|_| format!("0.0.0.0:{raft_port}").parse().unwrap())
}

/// Compute this server's cluster-facing address for inclusion in topology.
fn cluster_self_addr(config: &ServerConfig) -> SocketAddr {
    let port = config.cluster.raft_port.unwrap_or(config.network.port);
    format!("{}:{port}", config.network.bind_address)
        .parse()
        .unwrap_or_else(|_| format!("0.0.0.0:{port}").parse().unwrap())
}

/// Spawn a cluster port listener that dispatches between Raft messages and
/// `CLUSTER JOIN` requests.
///
/// Incoming connections are read for their first message. If it's a
/// `CLUSTER JOIN`, the join protocol is handled inline. Otherwise, the
/// connection is treated as a Raft peer and forwarded to the inbound channel.
fn spawn_cluster_listener(
    listener: Listener,
    inbound_tx: mpsc::UnboundedSender<(NodeId, RaftMessage<ClusterCommand>)>,
    shared_peer_map: SharedPeerMap,
    mut shutdown: ShutdownSignal,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("cluster port listener started");
        loop {
            match listener.accept_with_shutdown(&mut shutdown).await {
                Ok(Some((connection, peer_addr))) => {
                    let inbound_tx = inbound_tx.clone();
                    let shared_peer_map = Arc::clone(&shared_peer_map);
                    tokio::spawn(async move {
                        handle_cluster_connection(
                            connection,
                            peer_addr,
                            inbound_tx,
                            shared_peer_map,
                        )
                        .await;
                    });
                }
                Ok(None) => {
                    info!("cluster port listener shutting down");
                    break;
                }
                Err(e) => {
                    warn!(error = %e, "failed to accept cluster connection");
                }
            }
        }
    })
}

/// Handle a single connection on the cluster port.
///
/// Reads the first message to determine whether this is a Raft peer or a
/// `CLUSTER JOIN` request, then dispatches accordingly.
async fn handle_cluster_connection(
    mut connection: Connection,
    peer_addr: SocketAddr,
    inbound_tx: mpsc::UnboundedSender<(NodeId, RaftMessage<ClusterCommand>)>,
    shared_peer_map: SharedPeerMap,
) {
    // Read the first message to determine connection type.
    let first_msg = match connection.recv().await {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            debug!(%peer_addr, "cluster connection closed before first message");
            return;
        }
        Err(e) => {
            warn!(%peer_addr, error = %e, "error reading first cluster message");
            return;
        }
    };

    // Try to parse as a Raft message first (most common case).
    if let Ok(raft_msg) = RaftMessage::<ClusterCommand>::from_protocol_message(&first_msg) {
        // Identify the peer by looking up IP in the shared peer map.
        let node_id = {
            let map = shared_peer_map.read().await;
            let found = map
                .node_ids()
                .find(|id| map.get(*id).is_some_and(|addr| addr.ip() == peer_addr.ip()));
            found
        };
        let node_id = node_id.unwrap_or_else(|| {
            warn!(%peer_addr, "raft message from unknown peer IP");
            NodeId::new(u64::from(match peer_addr.ip() {
                std::net::IpAddr::V4(ip) => u32::from(ip),
                std::net::IpAddr::V6(_) => 0,
            }))
        });

        // Forward the first message, then continue as a regular inbound handler.
        if inbound_tx.send((node_id, raft_msg)).is_err() {
            return;
        }
        pirc_server::raft::transport::spawn_inbound_handler::<ClusterCommand>(
            node_id,
            connection,
            inbound_tx,
        );
        return;
    }

    // Not a Raft message — check if it's a CLUSTER JOIN.
    if ClusterService::parse_join_message(&first_msg).is_ok() {
        debug!(%peer_addr, "received CLUSTER JOIN on cluster port");
        // The actual join handling requires the ClusterService, which is not
        // available here directly. For now, log and close — the full join
        // handling will be wired in T139 (command handlers).
        let error_msg = ClusterService::build_error_message(
            &pirc_server::cluster::JoinError::Protocol {
                reason: "join protocol not yet handled on this listener".into(),
            },
        );
        let _ = connection.send(error_msg).await;
        return;
    }

    warn!(%peer_addr, command = ?first_msg.command, "unknown message on cluster port");
}
