use super::*;

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

#[tokio::test]
async fn nick_then_user_completes_registration() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("Alice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.nick.is_some());
    assert!(!state.registered);

    handle_message(
        &user_msg("alice", "Alice Test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.registered);
    assert_eq!(registry.connection_count(), 1);

    // Should receive RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD
    let welcome = rx.recv().await.unwrap();
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
    assert!(welcome
        .trailing()
        .unwrap()
        .contains("Welcome to the pirc network"));

    let yourhost = rx.recv().await.unwrap();
    assert_eq!(yourhost.numeric_code(), Some(RPL_YOURHOST));

    let created = rx.recv().await.unwrap();
    assert_eq!(created.numeric_code(), Some(RPL_CREATED));

    let nomotd = rx.recv().await.unwrap();
    assert_eq!(nomotd.numeric_code(), Some(ERR_NOMOTD));
}

#[tokio::test]
async fn user_then_nick_completes_registration() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &user_msg("bob", "Bob Test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(!state.registered);

    handle_message(
        &nick_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.registered);
    assert_eq!(registry.connection_count(), 1);

    let welcome = rx.recv().await.unwrap();
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
}

#[tokio::test]
async fn nick_no_param_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::Nick, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn nick_invalid_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("123invalid"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ERRONEUSNICKNAME));
}

#[tokio::test]
async fn nick_in_use_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx1, _rx1) = make_sender();
    let config = make_config();

    // Register a user first
    let now = Instant::now();
    let session = UserSession {
        connection_id: 99,
        nickname: Nickname::new("Alice").unwrap(),
        username: "alice".to_owned(),
        realname: "Alice".to_owned(),
        hostname: "127.0.0.1".to_owned(),
        modes: HashSet::new(),
        away_message: None,
        connected_at: now,
        signon_time: 0,
        last_active: now,
        registered: true,
        sender: tx1,
    };
    registry.register(session).unwrap();

    let (tx2, mut rx2) = make_sender();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("Alice"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));
    assert!(state.nick.is_none());
}

#[tokio::test]
async fn user_missing_params_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::User, vec!["alice".to_owned()]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn user_after_registration_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    // Register first
    handle_message(
        &nick_msg("Alice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &user_msg("alice", "Alice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.registered);

    // Drain the welcome messages
    while rx.try_recv().is_ok() {}

    // Try USER again
    handle_message(
        &user_msg("alice2", "Alice2"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ALREADYREGISTERED));
}

#[tokio::test]
async fn ping_gets_pong_response() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::Ping, vec!["token123".to_owned()]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "token123");
}

#[tokio::test]
async fn welcome_message_contains_nick_and_host() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("10.0.0.1".to_owned());

    handle_message(
        &nick_msg("TestNick"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &user_msg("testuser", "Test User"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let welcome = rx.recv().await.unwrap();
    let trailing = welcome.trailing().unwrap();
    assert!(trailing.contains("TestNick!testuser@10.0.0.1"));
    // First param after server prefix should be the nick
    assert_eq!(welcome.params[0], "TestNick");
}

#[tokio::test]
async fn registration_race_condition_handled() {
    // Two connections try to register with the same nick concurrently.
    // One should succeed, the other should get ERR_NICKNAMEINUSE.
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx1, mut rx1) = make_sender();
    let mut state1 = PreRegistrationState::new("127.0.0.1".to_owned());
    handle_message(
        &nick_msg("SameNick"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &user_msg("user1", "User One"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state1.registered);

    let (tx2, mut rx2) = make_sender();
    let mut state2 = PreRegistrationState::new("127.0.0.2".to_owned());
    handle_message(
        &nick_msg("SameNick"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    // Second connection should get ERR_NICKNAMEINUSE when trying nick
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));
    assert!(!state2.registered);

    // First connection should have welcome
    let welcome = rx1.recv().await.unwrap();
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
}

/// Helper: register a user and drain the welcome burst.
fn register_user(
    nick: &str,
    username: &str,
    connection_id: u64,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    config: &ServerConfig,
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
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
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
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD)
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

#[tokio::test]
async fn nick_change_after_registration_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &nick_msg("NewAlice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Nick);
    assert_eq!(reply.params[0], "NewAlice");

    // Verify old-nick prefix
    let prefix = reply.prefix.as_ref().unwrap();
    assert_eq!(prefix.to_string(), "Alice!alice@127.0.0.1");

    // Registry should have the new nick, not the old one
    let old = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&old).is_none());
    let new = Nickname::new("NewAlice").unwrap();
    assert!(registry.get_by_nick(&new).is_some());
}

#[tokio::test]
async fn nick_change_collision_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice tries to change to Bob's nick
    handle_message(
        &nick_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));

    // Alice should still have her old nick
    let alice = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&alice).is_some());
}

#[tokio::test]
async fn nick_change_invalid_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &nick_msg("123bad"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ERRONEUSNICKNAME));

    // Alice should still have her old nick
    let alice = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&alice).is_some());
}

#[tokio::test]
async fn nick_change_case_only_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &nick_msg("ALICE"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Nick);
    assert_eq!(reply.params[0], "ALICE");

    // Prefix should have the old casing
    let prefix = reply.prefix.as_ref().unwrap();
    assert_eq!(prefix.to_string(), "alice!alice@127.0.0.1");

    // Registry should reflect the updated casing
    let lookup = Nickname::new("alice").unwrap();
    let session_arc = registry.get_by_nick(&lookup).unwrap();
    let session = session_arc.read().unwrap();
    assert_eq!(session.nickname.to_string(), "ALICE");
}

#[tokio::test]
async fn nick_change_no_param_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    let msg = Message::new(Command::Nick, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn nick_change_prefix_has_correct_old_nick() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "OldNick", "theuser", 1, "10.0.0.5", &registry, &channels, &config,
    );

    handle_message(
        &nick_msg("NewNick"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Nick);
    assert_eq!(reply.params[0], "NewNick");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "OldNick!theuser@10.0.0.5"
    );
}

#[tokio::test]
async fn motd_text_is_sent_when_configured() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let (tx, mut rx) = make_sender();
    let mut config = make_config();
    config.motd.text = Some("Welcome!\nEnjoy your stay.".to_owned());
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("MotdUser"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &user_msg("motduser", "Motd User"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    // Skip RPL_WELCOME, RPL_YOURHOST, RPL_CREATED
    let _ = rx.recv().await.unwrap(); // 001
    let _ = rx.recv().await.unwrap(); // 002
    let _ = rx.recv().await.unwrap(); // 003

    // RPL_MOTDSTART
    let motd_start = rx.recv().await.unwrap();
    assert_eq!(
        motd_start.numeric_code(),
        Some(pirc_protocol::numeric::RPL_MOTDSTART)
    );

    // RPL_MOTD lines
    let motd_line1 = rx.recv().await.unwrap();
    assert_eq!(
        motd_line1.numeric_code(),
        Some(pirc_protocol::numeric::RPL_MOTD)
    );
    assert!(motd_line1.trailing().unwrap().contains("Welcome!"));

    let motd_line2 = rx.recv().await.unwrap();
    assert_eq!(
        motd_line2.numeric_code(),
        Some(pirc_protocol::numeric::RPL_MOTD)
    );
    assert!(motd_line2.trailing().unwrap().contains("Enjoy your stay."));

    // RPL_ENDOFMOTD
    let motd_end = rx.recv().await.unwrap();
    assert_eq!(
        motd_end.numeric_code(),
        Some(pirc_protocol::numeric::RPL_ENDOFMOTD)
    );
}

// ---- AWAY command tests ----

fn away_msg(text: &str) -> Message {
    Message::new(Command::Away, vec![text.to_owned()])
}

fn away_clear() -> Message {
    Message::new(Command::Away, vec![])
}

#[tokio::test]
async fn away_set_returns_rpl_nowaway() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &away_msg("Gone fishing"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_NOWAWAY)
    );
    assert!(reply.trailing().unwrap().contains("marked as being away"));

    // Verify session has away message
    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert_eq!(s.away_message.as_deref(), Some("Gone fishing"));
}

#[tokio::test]
async fn away_clear_returns_rpl_unaway() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // First set away
    handle_message(
        &away_msg("BRB"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx.recv().await.unwrap(); // drain RPL_NOWAWAY

    // Now clear away
    handle_message(
        &away_clear(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UNAWAY)
    );
    assert!(reply
        .trailing()
        .unwrap()
        .contains("no longer marked as being away"));

    // Verify session has no away message
    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert!(s.away_message.is_none());
}

#[tokio::test]
async fn away_set_then_update_message() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &away_msg("First"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    let _ = rx.recv().await.unwrap();

    handle_message(
        &away_msg("Second"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_NOWAWAY)
    );

    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert_eq!(s.away_message.as_deref(), Some("Second"));
}

// ---- MODE command tests ----

fn mode_query(nick: &str) -> Message {
    Message::new(Command::Mode, vec![nick.to_owned()])
}

fn mode_set(nick: &str, modestring: &str) -> Message {
    Message::new(Command::Mode, vec![nick.to_owned(), modestring.to_owned()])
}

fn mode_no_params() -> Message {
    Message::new(Command::Mode, vec![])
}

#[tokio::test]
async fn mode_query_own_returns_rpl_umodeis() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &mode_query("Alice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
    assert_eq!(reply.params[1], "+"); // no modes set
}

#[tokio::test]
async fn mode_query_other_returns_err_usersdontmatch() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(
        &mode_query("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::ERR_USERSDONTMATCH)
    );
}

#[tokio::test]
async fn mode_no_params_returns_err_needmoreparams() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &mode_no_params(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::ERR_NEEDMOREPARAMS)
    );
}

#[tokio::test]
async fn mode_set_voiced_on_self() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &mode_set("Alice", "+v"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
    assert_eq!(reply.params[1], "+v");

    // Verify mode was set
    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert!(s.modes.contains(&UserMode::Voiced));
}

#[tokio::test]
async fn mode_set_operator_self_ignored() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Trying +o should not self-promote
    handle_message(
        &mode_set("Alice", "+o"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
    assert_eq!(reply.params[1], "+"); // operator not added

    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert!(!s.modes.contains(&UserMode::Operator));
}

#[tokio::test]
async fn mode_remove_operator() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Manually set operator
    {
        let nick = Nickname::new("Alice").unwrap();
        let session = registry.get_by_nick(&nick).unwrap();
        let mut s = session.write().unwrap();
        s.modes.insert(UserMode::Operator);
    }

    handle_message(
        &mode_set("Alice", "-o"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
    assert_eq!(reply.params[1], "+"); // operator removed

    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert!(!s.modes.contains(&UserMode::Operator));
}

#[tokio::test]
async fn mode_unknown_flag_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &mode_set("Alice", "+x"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::ERR_UMODEUNKNOWNFLAG)
    );

    // Also sends RPL_UMODEIS after unknown flag
    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
}

#[tokio::test]
async fn mode_set_other_returns_err_usersdontmatch() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(
        &mode_set("Bob", "+v"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::ERR_USERSDONTMATCH)
    );
}

#[tokio::test]
async fn mode_query_case_insensitive() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Query with different casing
    handle_message(
        &mode_query("alice"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
}

#[tokio::test]
async fn mode_combined_modestring() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Set +v then -v in one string
    handle_message(
        &mode_set("Alice", "+v-v"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(
        reply.numeric_code(),
        Some(pirc_protocol::numeric::RPL_UMODEIS)
    );
    assert_eq!(reply.params[1], "+"); // v was added then removed

    let nick = Nickname::new("Alice").unwrap();
    let session = registry.get_by_nick(&nick).unwrap();
    let s = session.read().unwrap();
    assert!(!s.modes.contains(&UserMode::Voiced));
}
