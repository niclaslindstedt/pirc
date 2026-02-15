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
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD)
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

fn whois_msg(nick: &str) -> Message {
    Message::new(Command::Whois, vec![nick.to_owned()])
}

#[tokio::test]
async fn whois_existing_user_returns_reply_sequence() {
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

    // Register a target user
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &channels, &config);

    handle_message(
        &whois_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

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
    reply.params[2]
        .parse::<u64>()
        .expect("idle secs is a number");
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
        &whois_msg("Ghost"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));
}

#[tokio::test]
async fn whois_no_param_returns_nonicknamegiven() {
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

    let msg = Message::new(Command::Whois, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));
}

#[tokio::test]
async fn whois_away_user_includes_rpl_away() {
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

    // Register Bob and set away
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &channels, &config);
    {
        let bob_nick = Nickname::new("Bob").unwrap();
        let session_arc = registry.get_by_nick(&bob_nick).unwrap();
        let mut session = session_arc.write().unwrap();
        session.away_message = Some("Gone fishing".to_owned());
    }

    handle_message(
        &whois_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

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

    // Register Bob and set operator mode
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &channels, &config);
    {
        let bob_nick = Nickname::new("Bob").unwrap();
        let session_arc = registry.get_by_nick(&bob_nick).unwrap();
        let mut session = session_arc.write().unwrap();
        session.modes.insert(pirc_common::UserMode::Operator);
    }

    handle_message(
        &whois_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

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

    // Register Bob
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "10.0.0.2", &registry, &channels, &config);

    handle_message(
        &whois_msg("Bob"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

    // Skip to RPL_WHOISIDLE
    let _ = rx.recv().await.unwrap(); // 311
    let _ = rx.recv().await.unwrap(); // 312

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));
    let idle_secs: u64 = reply.params[2].parse().unwrap();
    // Should be near 0 since we just registered
    assert!(idle_secs < 5, "idle time should be near 0, got {idle_secs}");
}
