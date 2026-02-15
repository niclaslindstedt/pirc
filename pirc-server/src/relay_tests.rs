use std::sync::Arc;

use pirc_protocol::{Command, Message, PircSubcommand};
use tokio::sync::mpsc;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::handler::{handle_message, PreRegistrationState};
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

fn register_user(
    nick: &str,
    username: &str,
    connection_id: u64,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    config: &ServerConfig,
    prekey_store: &Arc<PreKeyBundleStore>,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
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
        prekey_store,
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
        prekey_store,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- PIRC ENCRYPTED relay ----

#[tokio::test]
async fn encrypted_relay_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "base64-encrypted-payload".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store,
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::Encrypted)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "base64-encrypted-payload");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed ENCRYPTED message");
    }
}

#[tokio::test]
async fn encrypted_relay_target_not_found() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Ghost".to_owned(), "payload".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

#[tokio::test]
async fn encrypted_relay_no_params() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(Command::Pirc(PircSubcommand::Encrypted), vec![]);

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS));
}

// ---- PIRC KEYEXCHANGE-ACK relay ----

#[tokio::test]
async fn keyexchange_ack_relay_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeAck),
        vec!["Bob".to_owned(), "ack-data".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store,
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::KeyExchangeAck)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "ack-data");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed KEYEXCHANGE-ACK message");
    }
}

#[tokio::test]
async fn keyexchange_ack_relay_target_not_found() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeAck),
        vec!["Ghost".to_owned(), "data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

// ---- PIRC KEYEXCHANGE-COMPLETE relay ----

#[tokio::test]
async fn keyexchange_complete_relay_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
        vec!["Bob".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store,
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::KeyExchangeComplete)));
    assert_eq!(relay.params[0], "Bob");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed KEYEXCHANGE-COMPLETE message");
    }
}

#[tokio::test]
async fn keyexchange_complete_relay_target_not_found() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
        vec!["Ghost".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

// ---- PIRC FINGERPRINT relay ----

#[tokio::test]
async fn fingerprint_relay_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::Fingerprint),
        vec!["Bob".to_owned(), "ABCD1234FINGERPRINT".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store,
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::Fingerprint)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "ABCD1234FINGERPRINT");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed FINGERPRINT message");
    }
}

#[tokio::test]
async fn fingerprint_relay_target_not_found() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::Fingerprint),
        vec!["Ghost".to_owned(), "fingerprint-data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

#[tokio::test]
async fn fingerprint_relay_no_params() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    let msg = Message::new(Command::Pirc(PircSubcommand::Fingerprint), vec![]);

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS));
}

// ---- Server does not inspect payload ----

#[tokio::test]
async fn encrypted_payload_forwarded_verbatim() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );
    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store,
    );

    // Send arbitrary binary-safe payload — server must not decode or modify it.
    let payload = "dGhpcyBpcyBhbiBlbmNyeXB0ZWQgbWVzc2FnZQ==";
    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), payload.to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store,
    );

    let relay = rx_bob.recv().await.unwrap();
    assert_eq!(relay.params[1], payload, "payload must be forwarded verbatim");
}
