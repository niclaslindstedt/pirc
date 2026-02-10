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
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(&nick_msg("Alice"), 1, &registry, &tx, &mut state, &config);
    assert!(state.nick.is_some());
    assert!(!state.registered);

    handle_message(
        &user_msg("alice", "Alice Test"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
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
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &user_msg("bob", "Bob Test"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );
    assert!(!state.registered);

    handle_message(&nick_msg("Bob"), 1, &registry, &tx, &mut state, &config);
    assert!(state.registered);
    assert_eq!(registry.connection_count(), 1);

    let welcome = rx.recv().await.unwrap();
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
}

#[tokio::test]
async fn nick_no_param_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::Nick, vec![]);
    handle_message(&msg, 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn nick_invalid_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("123invalid"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ERRONEUSNICKNAME));
}

#[tokio::test]
async fn nick_in_use_returns_err() {
    let registry = Arc::new(UserRegistry::new());
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

    handle_message(&nick_msg("Alice"), 2, &registry, &tx2, &mut state, &config);

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));
    assert!(state.nick.is_none());
}

#[tokio::test]
async fn user_missing_params_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::User, vec!["alice".to_owned()]);
    handle_message(&msg, 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn user_after_registration_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    // Register first
    handle_message(&nick_msg("Alice"), 1, &registry, &tx, &mut state, &config);
    handle_message(
        &user_msg("alice", "Alice"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );
    assert!(state.registered);

    // Drain the welcome messages
    while rx.try_recv().is_ok() {}

    // Try USER again
    handle_message(
        &user_msg("alice2", "Alice2"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ALREADYREGISTERED));
}

#[tokio::test]
async fn ping_gets_pong_response() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let msg = Message::new(Command::Ping, vec!["token123".to_owned()]);
    handle_message(&msg, 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "token123");
}

#[tokio::test]
async fn welcome_message_contains_nick_and_host() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let config = make_config();
    let mut state = PreRegistrationState::new("10.0.0.1".to_owned());

    handle_message(
        &nick_msg("TestNick"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );
    handle_message(
        &user_msg("testuser", "Test User"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
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
    let config = make_config();

    let (tx1, mut rx1) = make_sender();
    let mut state1 = PreRegistrationState::new("127.0.0.1".to_owned());
    handle_message(
        &nick_msg("SameNick"),
        1,
        &registry,
        &tx1,
        &mut state1,
        &config,
    );
    handle_message(
        &user_msg("user1", "User One"),
        1,
        &registry,
        &tx1,
        &mut state1,
        &config,
    );
    assert!(state1.registered);

    let (tx2, mut rx2) = make_sender();
    let mut state2 = PreRegistrationState::new("127.0.0.2".to_owned());
    handle_message(
        &nick_msg("SameNick"),
        2,
        &registry,
        &tx2,
        &mut state2,
        &config,
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
    config: &ServerConfig,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new(hostname.to_owned());
    handle_message(&nick_msg(nick), connection_id, registry, &tx, &mut state, config);
    handle_message(
        &user_msg(username, &format!("{nick} Test")),
        connection_id,
        registry,
        &tx,
        &mut state,
        config,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD)
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

#[tokio::test]
async fn nick_change_after_registration_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    handle_message(&nick_msg("NewAlice"), 1, &registry, &tx, &mut state, &config);

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
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &config);

    // Alice tries to change to Bob's nick
    handle_message(&nick_msg("Bob"), 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));

    // Alice should still have her old nick
    let alice = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&alice).is_some());
}

#[tokio::test]
async fn nick_change_invalid_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    handle_message(&nick_msg("123bad"), 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_ERRONEUSNICKNAME));

    // Alice should still have her old nick
    let alice = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&alice).is_some());
}

#[tokio::test]
async fn nick_change_case_only_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("alice", "alice", 1, "127.0.0.1", &registry, &config);

    handle_message(&nick_msg("ALICE"), 1, &registry, &tx, &mut state, &config);

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
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    let msg = Message::new(Command::Nick, vec![]);
    handle_message(&msg, 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn nick_change_prefix_has_correct_old_nick() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("OldNick", "theuser", 1, "10.0.0.5", &registry, &config);

    handle_message(&nick_msg("NewNick"), 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Nick);
    assert_eq!(reply.params[0], "NewNick");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "OldNick!theuser@10.0.0.5"
    );
}

fn whois_msg(nick: &str) -> Message {
    Message::new(Command::Whois, vec![nick.to_owned()])
}

#[tokio::test]
async fn whois_existing_user_returns_reply_sequence() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    // Register a target user
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &config);

    handle_message(&whois_msg("Bob"), 1, &registry, &tx, &mut state, &config);

    // RPL_WHOISUSER (311)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));
    assert_eq!(reply.params[0], "Alice"); // requestor
    assert_eq!(reply.params[1], "Bob"); // target nick
    assert_eq!(reply.params[2], "bob"); // username
    assert_eq!(reply.params[3], "10.0.0.2"); // hostname
    assert_eq!(reply.params[4], "*");
    assert!(reply.trailing().unwrap().contains("Bob Test")); // realname

    // RPL_WHOISSERVER (312)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));
    assert_eq!(reply.params[1], "Bob");
    assert_eq!(reply.params[2], SERVER_NAME);

    // RPL_WHOISIDLE (317)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));
    assert_eq!(reply.params[1], "Bob");
    // idle_secs should be a number
    reply.params[2].parse::<u64>().expect("idle secs is a number");
    // signon should be a number
    reply.params[3].parse::<u64>().expect("signon is a number");

    // RPL_ENDOFWHOIS (318)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));
    assert_eq!(reply.params[1], "Bob");
}

#[tokio::test]
async fn whois_nonexistent_nick_returns_nosuchnick() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    handle_message(
        &whois_msg("Ghost"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));
}

#[tokio::test]
async fn whois_no_param_returns_nonicknamegiven() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    let msg = Message::new(Command::Whois, vec![]);
    handle_message(&msg, 1, &registry, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn whois_away_user_includes_rpl_away() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    // Register Bob and set away
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &config);
    {
        let bob_nick = Nickname::new("Bob").unwrap();
        let session_arc = registry.get_by_nick(&bob_nick).unwrap();
        let mut session = session_arc.write().unwrap();
        session.away_message = Some("Gone fishing".to_owned());
    }

    handle_message(&whois_msg("Bob"), 1, &registry, &tx, &mut state, &config);

    // RPL_WHOISUSER (311)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));

    // RPL_WHOISSERVER (312)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));

    // RPL_AWAY (301) - because Bob is away
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_AWAY));
    assert!(reply.trailing().unwrap().contains("Gone fishing"));

    // RPL_WHOISIDLE (317)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));

    // RPL_ENDOFWHOIS (318)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));
}

#[tokio::test]
async fn whois_operator_includes_rpl_whoisoperator() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    // Register Bob and set operator mode
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &config);
    {
        let bob_nick = Nickname::new("Bob").unwrap();
        let session_arc = registry.get_by_nick(&bob_nick).unwrap();
        let mut session = session_arc.write().unwrap();
        session.modes.insert(pirc_common::UserMode::Operator);
    }

    handle_message(&whois_msg("Bob"), 1, &registry, &tx, &mut state, &config);

    // RPL_WHOISUSER (311)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));

    // RPL_WHOISSERVER (312)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));

    // RPL_WHOISOPERATOR (313)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISOPERATOR));
    assert!(reply.trailing().unwrap().contains("IRC operator"));

    // RPL_WHOISIDLE (317)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));

    // RPL_ENDOFWHOIS (318)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));
}

#[tokio::test]
async fn whois_idle_time_is_reasonable() {
    let registry = Arc::new(UserRegistry::new());
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &config);

    // Register Bob
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &config);

    handle_message(&whois_msg("Bob"), 1, &registry, &tx, &mut state, &config);

    // Skip to RPL_WHOISIDLE
    let _ = rx.recv().await.unwrap(); // 311
    let _ = rx.recv().await.unwrap(); // 312

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));
    let idle_secs: u64 = reply.params[2].parse().unwrap();
    // Should be near 0 since we just registered
    assert!(idle_secs < 5, "idle time should be near 0, got {idle_secs}");
}

#[tokio::test]
async fn motd_text_is_sent_when_configured() {
    let registry = Arc::new(UserRegistry::new());
    let (tx, mut rx) = make_sender();
    let mut config = make_config();
    config.motd.text = Some("Welcome!\nEnjoy your stay.".to_owned());
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    handle_message(
        &nick_msg("MotdUser"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
    );
    handle_message(
        &user_msg("motduser", "Motd User"),
        1,
        &registry,
        &tx,
        &mut state,
        &config,
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
