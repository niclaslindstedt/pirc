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
    );
    handle_message(
        &user_msg(username, &format!("{nick} Test")),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD)
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- QUIT command tests ----

#[tokio::test]
async fn quit_with_message_sends_error_and_removes_user() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    assert_eq!(registry.connection_count(), 1);

    let quit = Message::new(Command::Quit, vec!["Goodbye everyone".to_owned()]);
    let result = handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Quit));

    // Should receive an ERROR closing link message.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Error);
    let trailing = reply.trailing().unwrap();
    assert!(trailing.contains("Closing Link"));
    assert!(trailing.contains("Goodbye everyone"));

    // User should be removed from registry.
    assert_eq!(registry.connection_count(), 0);
    let nick = Nickname::new("Alice").unwrap();
    assert!(registry.get_by_nick(&nick).is_none());
}

#[tokio::test]
async fn quit_without_message_uses_default() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Bob", "bob", 1, "127.0.0.1", &registry, &channels, &config);

    let quit = Message::new(Command::Quit, vec![]);
    let result = handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Quit));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Error);
    let trailing = reply.trailing().unwrap();
    assert!(trailing.contains("Client Quit"));

    assert_eq!(registry.connection_count(), 0);
}

#[tokio::test]
async fn quit_pre_registration_returns_quit() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx) = make_sender();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    // Send NICK but not USER (not registered yet)
    handle_message(&nick_msg("Alice"), 1, &registry, &channels, &tx, &mut state, &config);
    assert!(!state.registered);

    let quit = Message::new(Command::Quit, vec![]);
    let result = handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Quit));
}

#[tokio::test]
async fn quit_sets_registered_false() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    assert!(state.registered);

    let quit = Message::new(Command::Quit, vec!["Bye".to_owned()]);
    handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config);

    // State should reflect unregistered after quit.
    assert!(!state.registered);
}

// ---- PING/PONG handler tests ----

#[tokio::test]
async fn ping_returns_pong_with_token() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let ping = Message::new(Command::Ping, vec!["mytoken123".to_owned()]);
    let result = handle_message(&ping, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "mytoken123");

    // Verify server prefix
    let prefix = reply.prefix.as_ref().unwrap();
    assert_eq!(prefix.to_string(), "pircd");
}

#[tokio::test]
async fn ping_pre_registration_returns_pong() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let ping = Message::new(Command::Ping, vec!["preregtoken".to_owned()]);
    let result = handle_message(&ping, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "preregtoken");
}

#[tokio::test]
async fn pong_is_absorbed_silently() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let pong = Message::new(Command::Pong, vec!["pircd".to_owned()]);
    let result = handle_message(&pong, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));

    // No response should be sent for PONG.
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn pong_pre_registration_is_absorbed() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new("127.0.0.1".to_owned());

    let pong = Message::new(Command::Pong, vec!["pircd".to_owned()]);
    let result = handle_message(&pong, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));
    assert!(rx.try_recv().is_err());
}

// ---- Idle tracking tests ----

#[tokio::test]
async fn idle_time_updated_on_regular_command() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // Wait a small amount to ensure time passes.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send a WHOIS command (a regular non-PING/PONG command).
    let whois = Message::new(Command::Whois, vec!["Alice".to_owned()]);
    handle_message(&whois, 1, &registry, &channels, &tx, &mut state, &config);

    // Check that last_active was updated recently.
    let session_arc = registry.get_by_connection(1).unwrap();
    let session = session_arc.read().unwrap();
    let idle_secs = session.last_active.elapsed().as_millis();
    assert!(idle_secs < 50, "idle time should be very small after activity");
}

#[tokio::test]
async fn idle_time_not_updated_on_ping() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // Record last_active right after registration.
    let initial_last_active = {
        let session_arc = registry.get_by_connection(1).unwrap();
        let session = session_arc.read().unwrap();
        session.last_active
    };

    // Wait to create a gap.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send a PING (should NOT update last_active).
    let ping = Message::new(Command::Ping, vec!["token".to_owned()]);
    handle_message(&ping, 1, &registry, &channels, &tx, &mut state, &config);

    let session_arc = registry.get_by_connection(1).unwrap();
    let session = session_arc.read().unwrap();
    assert_eq!(session.last_active, initial_last_active);
}

#[tokio::test]
async fn idle_time_not_updated_on_pong() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let initial_last_active = {
        let session_arc = registry.get_by_connection(1).unwrap();
        let session = session_arc.read().unwrap();
        session.last_active
    };

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send a PONG (should NOT update last_active).
    let pong = Message::new(Command::Pong, vec!["pircd".to_owned()]);
    handle_message(&pong, 1, &registry, &channels, &tx, &mut state, &config);

    let session_arc = registry.get_by_connection(1).unwrap();
    let session = session_arc.read().unwrap();
    assert_eq!(session.last_active, initial_last_active);
}

// ---- HandleResult return value tests ----

#[tokio::test]
async fn regular_commands_return_continue() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, _rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let whois = Message::new(Command::Whois, vec!["Alice".to_owned()]);
    let result = handle_message(&whois, 1, &registry, &channels, &tx, &mut state, &config);
    assert!(matches!(result, HandleResult::Continue));

    let away = Message::new(Command::Away, vec!["brb".to_owned()]);
    let result = handle_message(&away, 1, &registry, &channels, &tx, &mut state, &config);
    assert!(matches!(result, HandleResult::Continue));

    let nick = Message::new(Command::Nick, vec!["NewAlice".to_owned()]);
    let result = handle_message(&nick, 1, &registry, &channels, &tx, &mut state, &config);
    assert!(matches!(result, HandleResult::Continue));
}
