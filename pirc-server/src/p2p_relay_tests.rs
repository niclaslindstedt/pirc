use std::sync::Arc;

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

// ---- PIRC P2P OFFER relay ----

#[tokio::test]
async fn p2p_offer_relay_to_target() {
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

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pOffer),
        vec!["Bob".to_owned(), "sdp-offer-data".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pOffer)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "sdp-offer-data");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed P2P OFFER message");
    }
}

#[tokio::test]
async fn p2p_offer_target_offline_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pOffer),
        vec!["Ghost".to_owned(), "sdp-data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

#[tokio::test]
async fn p2p_offer_no_params_returns_needmoreparams() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(Command::Pirc(PircSubcommand::P2pOffer), vec![]);

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS));
}

// ---- PIRC P2P ANSWER relay ----

#[tokio::test]
async fn p2p_answer_relay_to_target() {
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

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pAnswer),
        vec!["Bob".to_owned(), "sdp-answer-data".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pAnswer)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "sdp-answer-data");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed P2P ANSWER message");
    }
}

#[tokio::test]
async fn p2p_answer_target_offline_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pAnswer),
        vec!["Ghost".to_owned(), "sdp-data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

// ---- PIRC P2P ICE relay ----

#[tokio::test]
async fn p2p_ice_relay_to_target() {
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

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pIce),
        vec!["Bob".to_owned(), "candidate:1 1 udp 2130706431 192.168.1.1 5060 typ host".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pIce)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "candidate:1 1 udp 2130706431 192.168.1.1 5060 typ host");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed P2P ICE message");
    }
}

#[tokio::test]
async fn p2p_ice_target_offline_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pIce),
        vec!["Ghost".to_owned(), "candidate-data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

#[tokio::test]
async fn p2p_ice_no_params_returns_needmoreparams() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(Command::Pirc(PircSubcommand::P2pIce), vec![]);

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS));
}

// ---- PIRC P2P ESTABLISHED relay ----

#[tokio::test]
async fn p2p_established_relay_to_target() {
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

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pEstablished),
        vec!["Bob".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pEstablished)));
    assert_eq!(relay.params[0], "Bob");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed P2P ESTABLISHED message");
    }
}

#[tokio::test]
async fn p2p_established_target_offline_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pEstablished),
        vec!["Ghost".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

// ---- PIRC P2P FAILED relay ----

#[tokio::test]
async fn p2p_failed_relay_to_target() {
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

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pFailed),
        vec!["Bob".to_owned(), "timeout".to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pFailed)));
    assert_eq!(relay.params[0], "Bob");
    assert_eq!(relay.params[1], "timeout");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    } else {
        panic!("expected user prefix on relayed P2P FAILED message");
    }
}

#[tokio::test]
async fn p2p_failed_target_offline_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pFailed),
        vec!["Ghost".to_owned(), "no-connectivity".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));
}

// ---- P2P signaling does NOT queue offline ----

#[tokio::test]
async fn p2p_offer_does_not_queue_offline() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pOffer),
        vec!["Ghost".to_owned(), "sdp-data".to_owned()],
    );

    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &prekey_store, &offline_store, &Arc::new(GroupRegistry::new()));

    // Should get ERR_NOSUCHNICK, not an offline notice.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::ERR_NOSUCHNICK));

    // Verify nothing was queued in the offline store.
    let ghost_nick = pirc_common::Nickname::new("Ghost").unwrap();
    let queued = offline_store.take_messages(&ghost_nick);
    assert!(queued.is_empty(), "P2P signaling must not queue offline messages");
}

// ---- Payload forwarded verbatim ----

#[tokio::test]
async fn p2p_ice_payload_forwarded_verbatim() {
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

    let candidate = "candidate:0 1 udp 2113937151 192.168.1.100 54321 typ host generation 0";
    let msg = Message::new(
        Command::Pirc(PircSubcommand::P2pIce),
        vec!["Bob".to_owned(), candidate.to_owned()],
    );

    handle_message(
        &msg, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert_eq!(relay.params[1], candidate, "ICE candidate payload must be forwarded verbatim");
}

// ---- Bidirectional relay ----

#[tokio::test]
async fn p2p_bidirectional_offer_answer() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let prekey_store = Arc::new(PreKeyBundleStore::new());
    let offline_store = Arc::new(OfflineMessageStore::default());

    let (tx_alice, mut rx_alice, mut state_alice) = register_user(
        "Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );
    let (tx_bob, mut rx_bob, mut state_bob) = register_user(
        "Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config, &prekey_store, &offline_store,
    );

    // Alice sends OFFER to Bob.
    let offer = Message::new(
        Command::Pirc(PircSubcommand::P2pOffer),
        vec!["Bob".to_owned(), "offer-sdp".to_owned()],
    );
    handle_message(
        &offer, 1, &registry, &channels, &tx_alice, &mut state_alice, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_bob.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pOffer)));
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Alice");
    }

    // Bob sends ANSWER back to Alice.
    let answer = Message::new(
        Command::Pirc(PircSubcommand::P2pAnswer),
        vec!["Alice".to_owned(), "answer-sdp".to_owned()],
    );
    handle_message(
        &answer, 2, &registry, &channels, &tx_bob, &mut state_bob, &config, None, &prekey_store, &offline_store,
    &Arc::new(GroupRegistry::new()),
    );

    let relay = rx_alice.recv().await.unwrap();
    assert!(matches!(relay.command, Command::Pirc(PircSubcommand::P2pAnswer)));
    assert_eq!(relay.params[1], "answer-sdp");
    if let Some(pirc_protocol::Prefix::User { nick, .. }) = &relay.prefix {
        assert_eq!(nick.to_string(), "Bob");
    }
}
