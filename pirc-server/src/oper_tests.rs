use super::*;
use crate::config::OperConfig;
use pirc_protocol::numeric::{
    ERR_NOOPERHOST, ERR_NOPRIVILEGES, ERR_PASSWDMISMATCH, RPL_YOUREOPER,
};

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

// ---- Helper: make user an operator ----

fn make_user_oper(registry: &Arc<UserRegistry>, nick_str: &str) {
    let nick = Nickname::new(nick_str).unwrap();
    let session_arc = registry.get_by_nick(&nick).unwrap();
    let mut session = session_arc.write().unwrap();
    session.modes.insert(UserMode::Operator);
}

// ---- KILL tests ----

#[tokio::test]
async fn kill_success_removes_target() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    let (_tx2, mut rx2, _state2) =
        register_user("Victim", "victim", 2, "127.0.0.1", &registry, &channels, &config);

    let kill = Message::new(Command::Kill, vec!["Victim".to_owned(), "Bad behavior".to_owned()]);
    handle_message(&kill, 1, &registry, &channels, &tx, &mut state, &config);

    // Admin should not get any error.
    assert!(rx.try_recv().is_err());

    // Victim should receive KILL and ERROR messages.
    let kill_reply = rx2.recv().await.unwrap();
    assert_eq!(kill_reply.command, Command::Kill);

    let error_reply = rx2.recv().await.unwrap();
    assert_eq!(error_reply.command, Command::Error);
    assert!(error_reply.trailing().unwrap().contains("Killed"));

    // Victim should be removed from registry.
    let victim_nick = Nickname::new("Victim").unwrap();
    assert!(registry.get_by_nick(&victim_nick).is_none());
}

#[tokio::test]
async fn kill_non_oper_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config);

    let kill = Message::new(Command::Kill, vec!["Bob".to_owned(), "reason".to_owned()]);
    handle_message(&kill, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));

    // Bob should still be in registry.
    let bob_nick = Nickname::new("Bob").unwrap();
    assert!(registry.get_by_nick(&bob_nick).is_some());
}

#[tokio::test]
async fn kill_missing_params_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    let kill = Message::new(Command::Kill, vec![]);
    handle_message(&kill, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn kill_unknown_target_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    let kill = Message::new(Command::Kill, vec!["Ghost".to_owned(), "reason".to_owned()]);
    handle_message(&kill, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));
}

// ---- DIE tests ----

#[tokio::test]
async fn die_success_returns_shutdown() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    // Register another user to verify they receive the notice.
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config);

    let die = Message::new(Command::Die, vec![]);
    let result = handle_message(&die, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Shutdown));

    // Admin should receive the server notice.
    let notice = rx.recv().await.unwrap();
    assert_eq!(notice.command, Command::Notice);
    assert!(notice.trailing().unwrap().contains("shutting down"));
    assert!(notice.trailing().unwrap().contains("Admin"));

    // Bob should also receive the server notice.
    let notice2 = rx2.recv().await.unwrap();
    assert_eq!(notice2.command, Command::Notice);
    assert!(notice2.trailing().unwrap().contains("shutting down"));
}

#[tokio::test]
async fn die_non_oper_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let die = Message::new(Command::Die, vec![]);
    let result = handle_message(&die, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

// ---- RESTART tests ----

#[tokio::test]
async fn restart_success_returns_shutdown() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.1", &registry, &channels, &config);

    let restart = Message::new(Command::Restart, vec![]);
    let result = handle_message(&restart, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Shutdown));

    // Admin should receive the server notice.
    let notice = rx.recv().await.unwrap();
    assert_eq!(notice.command, Command::Notice);
    assert!(notice.trailing().unwrap().contains("restarting"));
    assert!(notice.trailing().unwrap().contains("Admin"));

    // Bob should also receive the server notice.
    let notice2 = rx2.recv().await.unwrap();
    assert_eq!(notice2.command, Command::Notice);
    assert!(notice2.trailing().unwrap().contains("restarting"));
}

#[tokio::test]
async fn restart_non_oper_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let restart = Message::new(Command::Restart, vec![]);
    let result = handle_message(&restart, 1, &registry, &channels, &tx, &mut state, &config);

    assert!(matches!(result, HandleResult::Continue));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

// ---- WALLOPS tests ----

#[tokio::test]
async fn wallops_success_sends_to_opers() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Register an operator.
    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    // Register another operator.
    let (_tx2, mut rx2, _state2) =
        register_user("Oper2", "oper2", 2, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Oper2");

    // Register a non-operator.
    let (_tx3, mut rx3, _state3) =
        register_user("Regular", "regular", 3, "127.0.0.1", &registry, &channels, &config);

    let wallops = Message::new(Command::Wallops, vec!["Server maintenance soon".to_owned()]);
    handle_message(&wallops, 1, &registry, &channels, &tx, &mut state, &config);

    // Admin (oper) should receive WALLOPS.
    let msg = rx.recv().await.unwrap();
    assert_eq!(msg.command, Command::Wallops);
    assert!(msg.trailing().unwrap().contains("Server maintenance soon"));
    // Should have the sender's prefix.
    assert!(msg.prefix.is_some());

    // Oper2 (oper) should also receive WALLOPS.
    let msg2 = rx2.recv().await.unwrap();
    assert_eq!(msg2.command, Command::Wallops);
    assert!(msg2.trailing().unwrap().contains("Server maintenance soon"));

    // Regular (non-oper) should NOT receive WALLOPS.
    assert!(rx3.try_recv().is_err());
}

#[tokio::test]
async fn wallops_non_oper_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let wallops = Message::new(Command::Wallops, vec!["Hello".to_owned()]);
    handle_message(&wallops, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOPRIVILEGES));
}

#[tokio::test]
async fn wallops_missing_params_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) =
        register_user("Admin", "admin", 1, "127.0.0.1", &registry, &channels, &config);
    make_user_oper(&registry, "Admin");

    let wallops = Message::new(Command::Wallops, vec![]);
    handle_message(&wallops, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

// ---- MOTD command test ----

#[tokio::test]
async fn motd_command_returns_motd() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let mut config = make_config();
    config.motd.text = Some("Welcome to pirc!".to_owned());

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let motd = Message::new(Command::Motd, vec![]);
    handle_message(&motd, 1, &registry, &channels, &tx, &mut state, &config);

    // RPL_MOTDSTART
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::RPL_MOTDSTART));

    // RPL_MOTD
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::RPL_MOTD));
    assert!(reply.trailing().unwrap().contains("Welcome to pirc!"));

    // RPL_ENDOFMOTD
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(pirc_protocol::numeric::RPL_ENDOFMOTD));
}

#[tokio::test]
async fn motd_command_no_motd_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config(); // No MOTD configured

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    let motd = Message::new(Command::Motd, vec![]);
    handle_message(&motd, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOMOTD));
}
