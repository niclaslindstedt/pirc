use std::sync::Arc;

use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
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

/// Register a user and drain ALL messages (welcome burst + offline).
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
    let (tx, mut rx) = mpsc::unbounded_channel();
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

/// Register a user but only drain the welcome burst (RPL_WELCOME, RPL_YOURHOST,
/// RPL_CREATED, ERR_NOMOTD), preserving any offline messages that follow.
fn register_user_collect_offline(
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
    Vec<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = mpsc::unbounded_channel();
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

    // Collect all messages from the channel.
    let mut all_msgs = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        all_msgs.push(msg);
    }

    // Separate welcome burst (numeric messages) from offline messages (PIRC commands).
    let offline_msgs: Vec<Message> = all_msgs
        .into_iter()
        .filter(|m| matches!(m.command, Command::Pirc(_)))
        .collect();

    (tx, offline_msgs, state)
}

// ---- Offline delivery on reconnect ----

#[tokio::test]
async fn encrypted_message_queued_and_delivered_on_reconnect() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice.
    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Bob is NOT registered (offline). Alice sends an encrypted message to Bob.
    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "encrypted-payload".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Alice should get a notice about Bob being offline.
    let notice = rx_alice.recv().await.unwrap();
    assert_eq!(notice.command, Command::Notice);
    assert!(notice.trailing().unwrap().contains("offline"));

    // Now Bob connects and registers.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 1, "Bob should receive one offline message");
    assert!(matches!(offline_msgs[0].command, Command::Pirc(PircSubcommand::Encrypted)));
    assert_eq!(offline_msgs[0].params[1], "encrypted-payload");
}

#[tokio::test]
async fn keyexchange_message_queued_and_delivered_on_reconnect() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice.
    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Bob is offline. Alice sends a key exchange relay message (InitMessage, tag byte != 0 and != 1).
    let data = vec![2, 10, 20, 30]; // Tag 2 = InitMessage
    let encoded = pirc_crypto::protocol::encode_for_wire(&data);

    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded.clone()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Alice should get a notice about Bob being offline.
    let notice = rx_alice.recv().await.unwrap();
    assert_eq!(notice.command, Command::Notice);
    assert!(notice.trailing().unwrap().contains("offline"));

    // Now Bob connects.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 1, "Bob should receive one offline message");
    assert!(matches!(offline_msgs[0].command, Command::Pirc(PircSubcommand::KeyExchange)));
}

#[tokio::test]
async fn key_exchange_delivered_before_encrypted_on_reconnect() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice.
    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Bob is offline. Alice sends encrypted FIRST, then key exchange.
    // The server should reorder so key exchange comes first on delivery.

    // Send encrypted message first.
    let enc_msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "encrypted-payload".to_owned()],
    );
    handle_message(
        &enc_msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap(); // offline notice

    // Send key exchange message second.
    let ke_data = vec![2, 10, 20]; // Tag 2 = InitMessage
    let encoded = pirc_crypto::protocol::encode_for_wire(&ke_data);
    let ke_msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded],
    );
    handle_message(
        &ke_msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap(); // offline notice

    // Now Bob connects.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 2, "Bob should receive two offline messages");
    assert!(
        matches!(offline_msgs[0].command, Command::Pirc(PircSubcommand::KeyExchange)),
        "first message should be key exchange, got {:?}",
        offline_msgs[0].command,
    );
    assert!(
        matches!(offline_msgs[1].command, Command::Pirc(PircSubcommand::Encrypted)),
        "second message should be encrypted, got {:?}",
        offline_msgs[1].command,
    );
}

#[tokio::test]
async fn sender_prefix_preserved_in_offline_messages() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice.
    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Send encrypted message to offline Bob.
    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "payload".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Bob connects.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 1);
    let delivered = &offline_msgs[0];
    // The message should have Alice's prefix.
    if let Some(Prefix::User { nick, user, host }) = &delivered.prefix {
        assert_eq!(nick.to_string(), "Alice");
        assert_eq!(user, "alice");
        assert_eq!(host, "127.0.0.1");
    } else {
        panic!("expected user prefix on delivered offline message, got {:?}", delivered.prefix);
    }
}

#[tokio::test]
async fn multiple_senders_queued_for_same_offline_user() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice and Carol.
    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );
    let (tx_carol, _rx_carol, mut state_carol) = register_user(
        "Carol", "carol", 3, "127.0.0.2", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Both send encrypted messages to offline Bob.
    let msg1 = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "from-alice".to_owned()],
    );
    handle_message(
        &msg1, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let msg2 = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "from-carol".to_owned()],
    );
    handle_message(
        &msg2, 3, &registry, &channels, &tx_carol, &mut state_carol, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Bob connects.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 2);
    assert_eq!(offline_msgs[0].params[1], "from-alice");
    assert_eq!(offline_msgs[1].params[1], "from-carol");
}

#[tokio::test]
async fn offline_queue_cleared_after_delivery() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Register Alice.
    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Send message to offline Bob.
    let msg = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "hello".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Bob connects first time — gets the message.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );
    assert_eq!(offline_msgs.len(), 1, "should get offline message");

    // Bob disconnects and reconnects — queue should be empty.
    registry.remove_by_connection(2);
    let (_tx_bob2, offline_msgs2, _state_bob2) = register_user_collect_offline(
        "Bob", "bob", 4, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );
    assert!(offline_msgs2.is_empty(), "queue should be empty on second connect");
}

#[tokio::test]
async fn no_offline_messages_when_none_queued() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    // Bob connects with no messages queued.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert!(offline_msgs.is_empty());
}

#[tokio::test]
async fn keyexchange_ack_queued_for_offline_user() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, _rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Send KEYEXCHANGE-ACK to offline Bob.
    let msg = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeAck),
        vec!["Bob".to_owned(), "ack-data".to_owned()],
    );
    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    // Bob connects and should get the queued message.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 1);
    assert!(matches!(offline_msgs[0].command, Command::Pirc(PircSubcommand::KeyExchangeAck)));
    assert_eq!(offline_msgs[0].params[1], "ack-data");
}

#[tokio::test]
async fn full_ordering_ke_ack_complete_then_encrypted() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Send messages in reverse priority order to offline Bob.
    // 1. Encrypted (priority 3)
    let enc = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["Bob".to_owned(), "secret".to_owned()],
    );
    handle_message(
        &enc, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap();

    // 2. KEYEXCHANGE-COMPLETE (priority 2)
    let kec = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
        vec!["Bob".to_owned()],
    );
    handle_message(
        &kec, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap();

    // 3. KEYEXCHANGE-ACK (priority 1)
    let kea = Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeAck),
        vec!["Bob".to_owned(), "ack".to_owned()],
    );
    handle_message(
        &kea, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap();

    // 4. KEYEXCHANGE (priority 0) — need to make it a non-RequestBundle type
    let ke_data = vec![2, 1, 2, 3]; // Tag 2 = InitMessage
    let encoded = pirc_crypto::protocol::encode_for_wire(&ke_data);
    let ke = Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec!["Bob".to_owned(), encoded],
    );
    handle_message(
        &ke, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx_alice.recv().await.unwrap();

    // Bob connects — messages should be sorted by priority.
    let (_tx_bob, offline_msgs, _state_bob) = register_user_collect_offline(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    assert_eq!(offline_msgs.len(), 4, "Bob should receive 4 offline messages");
    assert!(
        matches!(offline_msgs[0].command, Command::Pirc(PircSubcommand::KeyExchange)),
        "first should be KEYEXCHANGE, got {:?}", offline_msgs[0].command,
    );
    assert!(
        matches!(offline_msgs[1].command, Command::Pirc(PircSubcommand::KeyExchangeAck)),
        "second should be KEYEXCHANGE-ACK, got {:?}", offline_msgs[1].command,
    );
    assert!(
        matches!(offline_msgs[2].command, Command::Pirc(PircSubcommand::KeyExchangeComplete)),
        "third should be KEYEXCHANGE-COMPLETE, got {:?}", offline_msgs[2].command,
    );
    assert!(
        matches!(offline_msgs[3].command, Command::Pirc(PircSubcommand::Encrypted)),
        "fourth should be ENCRYPTED, got {:?}", offline_msgs[3].command,
    );
}
