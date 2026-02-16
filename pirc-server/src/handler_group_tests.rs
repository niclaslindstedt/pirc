//! Tests for group chat server-side handlers.

use std::sync::Arc;

use pirc_common::types::GroupId;
use pirc_protocol::{Command, Message, PircSubcommand};
use tokio::sync::mpsc;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::group_registry::GroupRegistry;
use crate::handler::{handle_message, PreRegistrationState};
use crate::offline_store::OfflineMessageStore;
use crate::prekey_store::PreKeyBundleStore;
use crate::registry::UserRegistry;

fn make_config() -> ServerConfig {
    ServerConfig::default()
}

fn make_sender() -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
) {
    mpsc::unbounded_channel()
}

fn make_channels() -> Arc<ChannelRegistry> {
    Arc::new(ChannelRegistry::new())
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

/// Register a user and drain the welcome burst.
fn register_user(
    nick: &str,
    username: &str,
    connection_id: u64,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    config: &ServerConfig,
    group_registry: &Arc<GroupRegistry>,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new(hostname.to_owned());
    handle_message(
        &nick_msg(nick),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
        None,
        &prekey_store,
        &offline_store,
        group_registry,
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
        &prekey_store,
        &offline_store,
        group_registry,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

fn group_create_msg(group_name: &str) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::GroupCreate),
        vec![group_name.to_owned()],
    )
}

fn group_invite_msg(group_id: u64, target: &str) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::GroupInvite),
        vec![group_id.to_string(), target.to_owned()],
    )
}

fn group_join_msg(group_id: u64) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::GroupJoin),
        vec![group_id.to_string()],
    )
}

fn group_leave_msg(group_id: u64) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::GroupLeave),
        vec![group_id.to_string()],
    )
}

fn group_message_msg(group_id: u64, target: &str, payload: &str) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::GroupMessage),
        vec![group_id.to_string(), target.to_owned(), payload.to_owned()],
    )
}

// ── GROUP CREATE ────────────────────────────────────────────

#[tokio::test]
async fn group_create_returns_group_id() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );

    handle_message(
        &group_create_msg("my-group"),
        1, &registry, &channels, &tx, &mut state, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Pirc(PircSubcommand::GroupCreate));
    // params[0] = group_id, params[1] = group_name
    assert_eq!(reply.params.len(), 2);
    let gid: u64 = reply.params[0].parse().unwrap();
    assert!(gid > 0);
    assert_eq!(reply.params[1], "my-group");

    // Verify the group was created in the registry
    assert!(group_registry.exists(GroupId::new(gid)));
    assert!(group_registry.is_member(GroupId::new(gid), "Alice"));
    assert!(group_registry.is_admin(GroupId::new(gid), "Alice"));
}

#[tokio::test]
async fn group_create_no_params_returns_error() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );

    let msg = Message::new(Command::Pirc(PircSubcommand::GroupCreate), vec![]);
    handle_message(
        &msg, 1, &registry, &channels, &tx, &mut state, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS)
    );
}

// ── GROUP JOIN ──────────────────────────────────────────────

#[tokio::test]
async fn group_join_adds_member_and_broadcasts() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (_tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates a group
    handle_message(
        &group_create_msg("test-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Bob joins the group
    handle_message(
        &group_join_msg(gid),
        2, &registry, &channels, &_tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Alice should receive a GROUP JOIN notification
    let join_notify = rx_alice.recv().await.unwrap();
    assert_eq!(join_notify.command, Command::Pirc(PircSubcommand::GroupJoin));
    assert_eq!(join_notify.params[0], gid.to_string());

    // Bob should receive GROUP JOIN + GROUP MEMBERS
    let bob_join = rx_bob.recv().await.unwrap();
    assert_eq!(bob_join.command, Command::Pirc(PircSubcommand::GroupJoin));

    let bob_members = rx_bob.recv().await.unwrap();
    assert_eq!(bob_members.command, Command::Pirc(PircSubcommand::GroupMembers));
    // Should list both Alice and Bob
    assert!(bob_members.params.len() >= 2); // group_id + at least 2 members

    // Verify registry state
    assert!(group_registry.is_member(GroupId::new(gid), "Bob"));
    assert!(!group_registry.is_admin(GroupId::new(gid), "Bob"));
}

#[tokio::test]
async fn group_join_nonexistent_group_returns_error() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );

    handle_message(
        &group_join_msg(999),
        1, &registry, &channels, &tx, &mut state, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("does not exist"));
}

// ── GROUP LEAVE ─────────────────────────────────────────────

#[tokio::test]
async fn group_leave_removes_member_and_broadcasts() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates a group
    handle_message(
        &group_create_msg("leave-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Bob joins
    handle_message(
        &group_join_msg(gid),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    // Drain join notifications
    while rx_alice.try_recv().is_ok() {}
    while rx_bob.try_recv().is_ok() {}

    // Bob leaves
    handle_message(
        &group_leave_msg(gid),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Alice should receive GROUP LEAVE notification
    let leave_notify = rx_alice.recv().await.unwrap();
    assert_eq!(leave_notify.command, Command::Pirc(PircSubcommand::GroupLeave));

    // Verify Bob is no longer a member
    assert!(!group_registry.is_member(GroupId::new(gid), "Bob"));
    assert!(group_registry.is_member(GroupId::new(gid), "Alice"));
}

#[tokio::test]
async fn admin_leave_transfers_to_longest_tenured() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group (is admin)
    handle_message(
        &group_create_msg("admin-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Bob joins
    handle_message(
        &group_join_msg(gid),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    while rx_alice.try_recv().is_ok() {}
    while rx_bob.try_recv().is_ok() {}

    // Alice (admin) leaves
    handle_message(
        &group_leave_msg(gid),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Bob should now be admin
    assert!(group_registry.is_admin(GroupId::new(gid), "Bob"));
    assert!(!group_registry.is_member(GroupId::new(gid), "Alice"));
}

#[tokio::test]
async fn last_member_leave_destroys_group() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group
    handle_message(
        &group_create_msg("solo-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Alice leaves (last member)
    handle_message(
        &group_leave_msg(gid),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Group should be destroyed
    assert!(!group_registry.exists(GroupId::new(gid)));
}

// ── GROUP INVITE ────────────────────────────────────────────

#[tokio::test]
async fn group_invite_relays_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group
    handle_message(
        &group_create_msg("invite-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Alice invites Bob
    handle_message(
        &group_invite_msg(gid, "Bob"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Bob should receive the invite
    let invite = rx_bob.recv().await.unwrap();
    assert_eq!(invite.command, Command::Pirc(PircSubcommand::GroupInvite));
    assert_eq!(invite.params[0], gid.to_string());
}

#[tokio::test]
async fn group_invite_by_non_member_fails() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group (Bob is NOT a member)
    handle_message(
        &group_create_msg("invite-grp2"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Bob (non-member) tries to invite Charlie
    let (_tx_charlie, _rx_charlie, _state_charlie) = register_user(
        "Charlie", "charlie", 3, "127.0.0.3", &registry, &channels, &config, &group_registry,
    );

    handle_message(
        &group_invite_msg(gid, "Charlie"),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Bob should receive a notice about not being a member
    let reply = rx_bob.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("not a member"));
}

// ── GROUP MSG ───────────────────────────────────────────────

#[tokio::test]
async fn group_message_relays_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group, Bob joins
    handle_message(
        &group_create_msg("msg-grp"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    handle_message(
        &group_join_msg(gid),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    while rx_alice.try_recv().is_ok() {}
    while rx_bob.try_recv().is_ok() {}

    // Alice sends a group message to Bob
    handle_message(
        &group_message_msg(gid, "Bob", "encrypted_payload_data"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Bob should receive the message
    let msg = rx_bob.recv().await.unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::GroupMessage));
    assert_eq!(msg.params[2], "encrypted_payload_data");
}

#[tokio::test]
async fn group_message_non_member_fails() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &group_registry,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config, &group_registry,
    );

    // Alice creates group (Bob is NOT a member)
    handle_message(
        &group_create_msg("msg-grp2"),
        1, &registry, &channels, &tx_alice, &mut state_alice, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );
    let create_reply = rx_alice.recv().await.unwrap();
    let gid: u64 = create_reply.params[0].parse().unwrap();

    // Bob tries to send a message
    handle_message(
        &group_message_msg(gid, "Alice", "payload"),
        2, &registry, &channels, &tx_bob, &mut state_bob, &config, None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
        &group_registry,
    );

    // Bob should receive an error notice
    let reply = rx_bob.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("not a member"));
}
