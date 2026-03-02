use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::{Command, Message, PircSubcommand};
use tokio::sync::mpsc;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::handler::{handle_message, PreRegistrationState};
use crate::offline_store::OfflineMessageStore;
use crate::prekey_store::PreKeyBundleStore;
use crate::group_registry::GroupRegistry;
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
    offline_store: &Arc<OfflineMessageStore>,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new(hostname.to_owned());
    let group_registry = Arc::new(GroupRegistry::new());
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
        offline_store,
        &group_registry,
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
        offline_store,
        &group_registry,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- Bundle upload (PIRC KEYEXCHANGE * <data>) ----

#[tokio::test]
async fn store_bundle_via_keyexchange_self() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Build a bundle message: tag byte 1 (Bundle) followed by some data.
    let bundle_data = vec![1, 10, 20, 30];
    let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["*".to_owned(), encoded],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    // Should receive acknowledgment notice.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("Pre-key bundle registered"));

    // Bundle should be stored.
    let nick = Nickname::new("Alice").unwrap();
    let stored = prekey_store.get_bundle(&nick).unwrap();
    assert_eq!(stored, bundle_data);
}

#[tokio::test]
async fn store_bundle_invalid_encoding_sends_error() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["*".to_owned(), "not-valid-base64!!!".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("Invalid"));
}

#[tokio::test]
async fn store_bundle_wrong_tag_sends_error() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Tag byte 0 = RequestBundle, not a Bundle message.
    let data = vec![0];
    let encoded = pirc_crypto::protocol::encode_for_wire(&data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["*".to_owned(), encoded],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("Expected a Bundle"));
}

// ---- Request bundle (PIRC KEYEXCHANGE <target>) ----

#[tokio::test]
async fn request_bundle_returns_stored_bundle() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Pre-store Bob's bundle.
    let bob_nick = Nickname::new("Bob").unwrap();
    let bundle_data = vec![1, 42, 43, 44];
    prekey_store.store_bundle(&bob_nick, bundle_data.clone());

    // Also register Bob so we can verify the target nick resolution works.
    let (_tx2, _rx2, _state2) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice requests Bob's bundle (no data param = RequestBundle).
    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    // Alice should receive Bob's bundle back.
    let reply = rx.recv().await.unwrap();
    assert!(matches!(reply.command, Command::Pirc(PircSubcommand::KeyExchange)));
    assert_eq!(reply.params[0], "Alice"); // Sent to Alice
    // Decode the bundle data from the reply.
    let decoded = pirc_crypto::protocol::decode_from_wire(&reply.params[1]).unwrap();
    assert_eq!(decoded, bundle_data);
}

#[tokio::test]
async fn request_bundle_with_explicit_request_tag() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let bob_nick = Nickname::new("Bob").unwrap();
    let bundle_data = vec![1, 50, 51, 52];
    prekey_store.store_bundle(&bob_nick, bundle_data.clone());

    let (_tx2, _rx2, _state2) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // RequestBundle with explicit tag byte 0.
    let request_data = vec![0]; // TAG_REQUEST_BUNDLE
    let encoded = pirc_crypto::protocol::encode_for_wire(&request_data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert!(matches!(reply.command, Command::Pirc(PircSubcommand::KeyExchange)));
    let decoded = pirc_crypto::protocol::decode_from_wire(&reply.params[1]).unwrap();
    assert_eq!(decoded, bundle_data);
}

#[tokio::test]
async fn request_bundle_missing_sends_notice() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Request bundle for Bob who has no bundle stored.
    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("No pre-key bundle available"));
}

// ---- Relay messages (PIRC KEYEXCHANGE <target> <non-request-data>) ----

#[tokio::test]
async fn relay_init_message_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice sends an InitMessage (tag byte 2) to Bob.
    let init_data = vec![2, 100, 101, 102];
    let encoded = pirc_crypto::protocol::encode_for_wire(&init_data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded.clone()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    // Bob should receive the relayed message.
    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::KeyExchange)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], encoded);
    // Should have Alice's prefix.
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix");
    }
}

#[tokio::test]
async fn relay_complete_message_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice sends a Complete message (tag byte 3) to Bob.
    let complete_data = vec![3];
    let encoded = pirc_crypto::protocol::encode_for_wire(&complete_data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded.clone()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::KeyExchange)));
    assert_eq!(relay.params[1], encoded);
}

#[tokio::test]
async fn relay_bundle_message_to_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let (_tx_bob, mut rx_bob, _state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice sends a Bundle message (tag byte 1) to Bob (e.g. direct bundle delivery).
    let bundle_data = vec![1, 200, 201, 202];
    let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded.clone()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::KeyExchange)));
    assert_eq!(relay.params[1], encoded);
}

// ---- Error cases ----

#[tokio::test]
async fn relay_to_nonexistent_target_sends_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice sends a Complete message to a nonexistent user.
    let data = vec![3];
    let encoded = pirc_crypto::protocol::encode_for_wire(&data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Ghost".to_owned(), encoded],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("offline"));
}

#[tokio::test]
async fn keyexchange_no_params_sends_need_more_params() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec![],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS));
}

// ---- Chunked bundle upload (PIRC KEYEXCHANGE * <n>/<total> <chunk>) ----

#[tokio::test]
async fn chunked_bundle_upload_assembles_and_stores() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());
    let group_registry = Arc::new(GroupRegistry::new());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Build a small bundle and encode it, then split into 2 chunks.
    let bundle_data = vec![1u8, 2, 3, 4, 5, 6];
    let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_data);
    let mid = encoded.len() / 2;
    let chunk1 = &encoded[..mid];
    let chunk2 = &encoded[mid..];

    // Send first chunk — no acknowledgment yet.
    let msg1 = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["*".to_owned(), "1/2".to_owned(), chunk1.to_owned()],
    );
    handle_message(&msg1, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &group_registry);
    assert!(rx.try_recv().is_err(), "no reply after first chunk");

    // Send second chunk — should trigger storage and acknowledgment.
    let msg2 = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["*".to_owned(), "2/2".to_owned(), chunk2.to_owned()],
    );
    handle_message(&msg2, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &group_registry);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert!(reply.trailing().unwrap().contains("Pre-key bundle registered"));

    let nick = Nickname::new("Alice").unwrap();
    let stored = prekey_store.get_bundle(&nick).unwrap();
    assert_eq!(stored, bundle_data);
}
