use super::*;
use crate::config::OperConfig;

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
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new(hostname.to_owned());
    handle_message(&nick_msg(nick), connection_id, registry, channels, &tx, &mut state, config);
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
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

fn oper_msg(name: &str, password: &str) -> Message {
    Message::new(Command::Oper, vec![name.to_owned(), password.to_owned()])
}

fn config_with_oper(name: &str, password: &str, host_mask: Option<&str>) -> ServerConfig {
    let mut config = make_config();
    config.operators.push(OperConfig {
        name: name.to_owned(),
        password: password.to_owned(),
        host_mask: host_mask.map(|s| s.to_owned()),
    });
    config
}

// ---- OPER success ----

#[tokio::test]
async fn oper_success_grants_operator_mode() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", None);
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&oper_msg("admin", "secret"), 1, &registry, &channels, &tx, &mut state, &config);

    // RPL_YOUREOPER (381)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_YOUREOPER));
    assert!(reply.trailing().unwrap().contains("IRC operator"));

    // MODE notification
    let mode_reply = rx.recv().await.unwrap();
    assert_eq!(mode_reply.command, Command::Mode);
    assert_eq!(mode_reply.params[0], "Alice");
    assert!(mode_reply.trailing().unwrap().contains("+o"));

    // Verify mode was set in session
    let nick = Nickname::new("Alice").unwrap();
    let session_arc = registry.get_by_nick(&nick).unwrap();
    let session = session_arc.read().unwrap();
    assert!(session.modes.contains(&UserMode::Operator));
}

// ---- OPER missing params ----

#[tokio::test]
async fn oper_missing_params_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", None);
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // No params
    let msg = Message::new(Command::Oper, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn oper_one_param_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", None);
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // Only one param (name but no password)
    let msg = Message::new(Command::Oper, vec!["admin".to_owned()]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

// ---- OPER wrong password ----

#[tokio::test]
async fn oper_wrong_password_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", None);
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(
        &oper_msg("admin", "wrongpassword"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_PASSWDMISMATCH));

    // Verify mode was NOT set
    let nick = Nickname::new("Alice").unwrap();
    let session_arc = registry.get_by_nick(&nick).unwrap();
    let session = session_arc.read().unwrap();
    assert!(!session.modes.contains(&UserMode::Operator));
}

// ---- OPER unknown name ----

#[tokio::test]
async fn oper_unknown_name_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", None);
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(
        &oper_msg("unknown", "secret"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_PASSWDMISMATCH));
}

// ---- OPER host mask ----

#[tokio::test]
async fn oper_host_mask_matches() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", Some("127.0.0.*"));
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&oper_msg("admin", "secret"), 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_YOUREOPER));
}

#[tokio::test]
async fn oper_host_mask_mismatch_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", Some("10.0.0.*"));
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&oper_msg("admin", "secret"), 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOOPERHOST));

    // Verify mode was NOT set
    let nick = Nickname::new("Alice").unwrap();
    let session_arc = registry.get_by_nick(&nick).unwrap();
    let session = session_arc.read().unwrap();
    assert!(!session.modes.contains(&UserMode::Operator));
}

#[tokio::test]
async fn oper_host_mask_exact_match() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = config_with_oper("admin", "secret", Some("127.0.0.1"));
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&oper_msg("admin", "secret"), 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_YOUREOPER));
}

// ---- OPER no operators configured ----

#[tokio::test]
async fn oper_no_operators_configured_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config(); // no operators
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&oper_msg("admin", "secret"), 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_PASSWDMISMATCH));
}

// ---- is_oper helper ----

#[test]
fn is_oper_returns_false_for_regular_user() {
    let (tx, _rx) = make_sender();
    let now = Instant::now();
    let session = UserSession {
        connection_id: 1,
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
        sender: tx,
    };
    assert!(!is_oper(&session));
}

#[test]
fn is_oper_returns_true_for_operator() {
    let (tx, _rx) = make_sender();
    let now = Instant::now();
    let mut modes = HashSet::new();
    modes.insert(UserMode::Operator);
    let session = UserSession {
        connection_id: 1,
        nickname: Nickname::new("Alice").unwrap(),
        username: "alice".to_owned(),
        realname: "Alice".to_owned(),
        hostname: "127.0.0.1".to_owned(),
        modes,
        away_message: None,
        connected_at: now,
        signon_time: 0,
        last_active: now,
        registered: true,
        sender: tx,
    };
    assert!(is_oper(&session));
}

// ---- host_matches_mask unit tests ----

#[test]
fn host_mask_wildcard_suffix() {
    assert!(host_matches_mask("127.0.0.1", "127.0.0.*"));
    assert!(host_matches_mask("127.0.0.255", "127.0.0.*"));
    assert!(!host_matches_mask("10.0.0.1", "127.0.0.*"));
}

#[test]
fn host_mask_exact() {
    assert!(host_matches_mask("localhost", "localhost"));
    assert!(!host_matches_mask("otherhost", "localhost"));
}

#[test]
fn host_mask_wildcard_prefix() {
    assert!(host_matches_mask("user.example.com", "*.example.com"));
    assert!(!host_matches_mask("user.other.com", "*.example.com"));
}

#[test]
fn host_mask_all_wildcard() {
    assert!(host_matches_mask("anything", "*"));
    assert!(host_matches_mask("127.0.0.1", "*"));
}

// ---- Config TOML parsing with operators ----

#[test]
fn config_with_operators_parses() {
    let toml_str = r#"
[[operators]]
name = "admin"
password = "secret"
host_mask = "127.0.0.*"

[[operators]]
name = "helper"
password = "helperpass"
"#;
    let config: ServerConfig = toml::from_str(toml_str).expect("parse operators config");
    assert_eq!(config.operators.len(), 2);
    assert_eq!(config.operators[0].name, "admin");
    assert_eq!(config.operators[0].password, "secret");
    assert_eq!(config.operators[0].host_mask.as_deref(), Some("127.0.0.*"));
    assert_eq!(config.operators[1].name, "helper");
    assert_eq!(config.operators[1].password, "helperpass");
    assert!(config.operators[1].host_mask.is_none());
}

#[test]
fn config_without_operators_defaults_to_empty() {
    let config = ServerConfig::default();
    assert!(config.operators.is_empty());
}
