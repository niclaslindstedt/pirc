use std::sync::Arc;
use std::time::Duration;

use pirc_common::UserMode;
use pirc_protocol::numeric::ERR_NOPRIVILEGES;
use pirc_protocol::{Command, Message, PircSubcommand};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::channel_registry::ChannelRegistry;
use crate::cluster::InviteKeyStore;
use crate::config::ServerConfig;
use crate::degraded_mode::DegradedModeState;
use crate::handler::{handle_message, PreRegistrationState};
use crate::handler_cluster::ClusterContext;
use crate::raft::test_utils::MemStorage;
use crate::raft::transport::{PeerMap, SharedPeerMap};
use crate::raft::{ClusterCommand, NodeId, NullStateMachine, RaftBuilder, RaftConfig, RaftHandle};
use crate::registry::UserRegistry;

fn make_sender() -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
) {
    mpsc::unbounded_channel()
}

fn make_config() -> ServerConfig {
    ServerConfig::default()
}

fn nick_msg(nick: &str) -> Message {
    Message::new(Command::Nick, vec![nick.to_owned()])
}

fn user_msg(username: &str, realname: &str) -> Message {
    Message::new(
        Command::User,
        vec![
            username.to_owned(),
            "0".to_owned(),
            "*".to_owned(),
            realname.to_owned(),
        ],
    )
}

fn register_user(
    nick: &str,
    username: &str,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    config: &ServerConfig,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());
    handle_message(
        &nick_msg(nick),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
        None,
    );
    handle_message(
        &user_msg(username, &format!("{nick} Test")),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
        None,
    );
    assert!(state.registered);
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

fn make_oper(registry: &Arc<UserRegistry>, connection_id: u64) {
    let session_arc = registry.get_by_connection(connection_id).unwrap();
    let mut session = session_arc.write().expect("session lock poisoned");
    session.modes.insert(UserMode::Operator);
}

fn test_raft_config() -> RaftConfig {
    RaftConfig {
        election_timeout_min: Duration::from_millis(100),
        election_timeout_max: Duration::from_millis(200),
        heartbeat_interval: Duration::from_millis(30),
        node_id: NodeId::new(1),
        peers: vec![],
        ..RaftConfig::default()
    }
}

async fn setup_cluster_context() -> (Arc<ClusterContext>, Arc<RaftHandle<ClusterCommand>>) {
    let config = test_raft_config();
    let node_id = config.node_id;

    let (_driver, handle, _shutdown_tx, _inbound_tx, _outbound_rx) =
        RaftBuilder::<ClusterCommand, _, _>::new()
            .config(config)
            .storage(MemStorage::new())
            .state_machine(NullStateMachine)
            .build()
            .await
            .unwrap();

    let handle = Arc::new(handle);
    let invite_keys = Arc::new(Mutex::new(InviteKeyStore::new()));
    let peer_map = PeerMap::new(vec![
        (NodeId::new(2), "10.0.0.2:7000".parse().unwrap()),
        (NodeId::new(3), "10.0.0.3:7000".parse().unwrap()),
    ]);
    let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

    let ctx = Arc::new(ClusterContext {
        invite_keys,
        raft_handle: Arc::clone(&handle),
        shared_peer_map,
        self_id: node_id,
        degraded_state: Arc::new(DegradedModeState::new()),
    });

    (ctx, handle)
}

/// Collect all NOTICE messages from the receiver.
fn collect_notices(rx: &mut mpsc::UnboundedReceiver<Message>) -> Vec<String> {
    let mut notices = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if msg.command == Command::Notice {
            if let Some(text) = msg.params.last() {
                notices.push(text.clone());
            }
        }
    }
    notices
}

// ---- Invite-key generate ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_generate_requires_oper() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyGenerate), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_generate_success() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyGenerate), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert!(notices.len() >= 2);
    assert!(notices[0].contains("Invite key generated:"));
    assert!(notices[1].contains("86400")); // default 24h expiry
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_generate_with_custom_ttl() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::InviteKeyGenerate),
        vec!["3600".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert!(notices.len() >= 2);
    assert!(notices[0].contains("Invite key generated:"));
    assert!(notices[1].contains("3600"));
}

// ---- Invite-key list ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_list_requires_oper() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyList), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_list_empty() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyList), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("No active invite keys"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_list_shows_keys() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    // Generate a key first.
    {
        let mut store = ctx.invite_keys.lock().await;
        store.create(NodeId::new(1), None, true);
    }

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyList), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert!(notices.len() >= 2);
    assert!(notices[0].contains("Active invite keys (1)"));
    assert!(notices[1].contains("single-use"));
}

// ---- Invite-key revoke ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_revoke_requires_oper() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::InviteKeyRevoke),
        vec!["some-key".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_revoke_success() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let key_token = {
        let mut store = ctx.invite_keys.lock().await;
        let key = store.create(NodeId::new(1), None, true);
        key.as_str().to_owned()
    };

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::InviteKeyRevoke),
        vec![key_token],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("revoked"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_revoke_not_found() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::InviteKeyRevoke),
        vec!["nonexistent-key".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("not found"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_key_revoke_missing_param() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);
    make_oper(&registry, 1);

    let msg = Message::new(Command::Pirc(PircSubcommand::InviteKeyRevoke), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("Usage"));
}

// ---- Cluster status ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cluster_status_shows_info() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(Command::Pirc(PircSubcommand::ClusterStatus), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert!(notices.len() >= 4);
    assert!(notices[0].contains("Cluster Status"));
    assert!(notices[1].contains("Node ID: 1"));
    assert!(notices[2].contains("Role:"));
    assert!(notices[3].contains("Term:"));
}

// ---- Cluster members ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cluster_members_shows_peers() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(Command::Pirc(PircSubcommand::ClusterMembers), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    // Header + self + 2 peers
    assert!(notices.len() >= 4);
    assert!(notices[0].contains("Cluster Members"));
    assert!(notices[1].contains("Node 1 (self)"));
    // Peers sorted by ID
    assert!(notices[2].contains("Node 2"));
    assert!(notices[3].contains("Node 3"));
}

// ---- Network info ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_info_shows_servers_and_users() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();
    let (ctx, _handle) = setup_cluster_context().await;

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    let msg = Message::new(Command::Pirc(PircSubcommand::NetworkInfo), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config,
        Some(ctx.as_ref()),
    );

    let notices = collect_notices(&mut rx);
    assert!(notices.len() >= 3);
    assert!(notices[0].contains("Network Info"));
    assert!(notices[1].contains("Total servers: 3")); // self + 2 peers
    assert!(notices[2].contains("Local users: 1"));
}

// ---- Protocol parsing ----

#[test]
fn parse_cluster_status() {
    let msg = pirc_protocol::parse("PIRC CLUSTER STATUS\r\n").unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterStatus));
}

#[test]
fn parse_cluster_members() {
    let msg = pirc_protocol::parse("PIRC CLUSTER MEMBERS\r\n").unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterMembers));
}

#[test]
fn parse_invite_key_generate() {
    let msg = pirc_protocol::parse("PIRC INVITE-KEY GENERATE\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::InviteKeyGenerate)
    );
}

#[test]
fn parse_invite_key_generate_with_ttl() {
    let msg = pirc_protocol::parse("PIRC INVITE-KEY GENERATE 3600\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::InviteKeyGenerate)
    );
    assert_eq!(msg.params, vec!["3600"]);
}

#[test]
fn parse_invite_key_list() {
    let msg = pirc_protocol::parse("PIRC INVITE-KEY LIST\r\n").unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::InviteKeyList));
}

#[test]
fn parse_invite_key_revoke() {
    let msg = pirc_protocol::parse("PIRC INVITE-KEY REVOKE abc123\r\n").unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::InviteKeyRevoke));
    assert_eq!(msg.params, vec!["abc123"]);
}

#[test]
fn parse_network_info() {
    let msg = pirc_protocol::parse("PIRC NETWORK INFO\r\n").unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::NetworkInfo));
}

// ---- Commands without cluster context are silently ignored ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_without_cluster_ctx_are_noop() {
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = make_config();

    let (tx, mut rx, mut state) = register_user("Alice", "alice", 1, &registry, &channels, &config);

    // Send cluster commands without a cluster context (None).
    for cmd in [
        Command::Pirc(PircSubcommand::ClusterStatus),
        Command::Pirc(PircSubcommand::ClusterMembers),
        Command::Pirc(PircSubcommand::NetworkInfo),
        Command::Pirc(PircSubcommand::InviteKeyGenerate),
        Command::Pirc(PircSubcommand::InviteKeyList),
        Command::Pirc(PircSubcommand::InviteKeyRevoke),
    ] {
        let msg = Message::new(cmd, vec![]);
        handle_message(
            &msg, 1, &registry, &channels, &tx, &mut state, &config, None,
        );
    }

    // No messages should have been sent.
    assert!(rx.try_recv().is_err());
}
