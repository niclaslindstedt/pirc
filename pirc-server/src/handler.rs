use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_ERRONEUSNICKNAME, ERR_NEEDMOREPARAMS, ERR_NICKNAMEINUSE, ERR_NOMOTD,
    ERR_NONICKNAMEGIVEN, RPL_CREATED, RPL_WELCOME, RPL_YOURHOST,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::warn;

use crate::config::ServerConfig;
use crate::registry::UserRegistry;
use crate::user::UserSession;

const SERVER_NAME: &str = "pircd";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tracks partial client state before NICK + USER are both received.
pub struct PreRegistrationState {
    pub nick: Option<Nickname>,
    pub username: Option<String>,
    pub realname: Option<String>,
    pub hostname: String,
    pub registered: bool,
}

impl PreRegistrationState {
    pub fn new(hostname: String) -> Self {
        Self {
            nick: None,
            username: None,
            realname: None,
            hostname,
            registered: false,
        }
    }

    fn is_ready(&self) -> bool {
        self.nick.is_some() && self.username.is_some()
    }
}

/// Handle a single parsed message from a client connection.
///
/// For pre-registration clients, only NICK, USER, PING, and QUIT are processed.
/// Once both NICK and USER have been received, the client is registered in the
/// `UserRegistry` and the welcome burst is sent.
pub fn handle_message(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
    config: &ServerConfig,
) {
    match &msg.command {
        Command::Nick => handle_nick(msg, registry, sender, state),
        Command::User => handle_user(msg, sender, state),
        Command::Ping => {
            handle_ping(msg, sender);
            return;
        }
        _ => return,
    }

    // After processing NICK or USER, check if registration can complete.
    if state.is_ready() && !state.registered {
        complete_registration(connection_id, registry, sender, state, config);
    }
}

fn handle_nick(
    msg: &Message,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    if msg.params.is_empty() {
        send_numeric(sender, ERR_NONICKNAMEGIVEN, &["*"], "No nickname given");
        return;
    }

    let nick_str = &msg.params[0];
    let Ok(nick) = Nickname::new(nick_str) else {
        send_numeric(
            sender,
            ERR_ERRONEUSNICKNAME,
            &[nick_str],
            "Erroneous nickname",
        );
        return;
    };

    if registry.nick_in_use(&nick) {
        send_numeric(
            sender,
            ERR_NICKNAMEINUSE,
            &[nick.as_ref()],
            "Nickname is already in use",
        );
        return;
    }

    state.nick = Some(nick);
}

fn handle_user(
    msg: &Message,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    if state.registered {
        send_numeric(
            sender,
            ERR_ALREADYREGISTERED,
            &["*"],
            "You may not reregister",
        );
        return;
    }

    // USER <username> <mode> <unused> :<realname>
    if msg.params.len() < 4 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &["USER"],
            "Not enough parameters",
        );
        return;
    }

    state.username = Some(msg.params[0].clone());
    state.realname = Some(msg.params[3].clone());
}

fn complete_registration(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
    config: &ServerConfig,
) {
    let nick = state.nick.clone().expect("nick set before registration");
    let username = state
        .username
        .clone()
        .expect("username set before registration");
    let realname = state
        .realname
        .clone()
        .expect("realname set before registration");

    let now = Instant::now();
    let session = UserSession {
        connection_id,
        nickname: nick.clone(),
        username: username.clone(),
        realname: realname.clone(),
        hostname: state.hostname.clone(),
        modes: HashSet::new(),
        away_message: None,
        connected_at: now,
        last_active: now,
        registered: true,
        sender: sender.clone(),
    };

    if let Err(e) = registry.register(session) {
        warn!(connection_id, "registration failed: {e}");
        send_numeric(
            sender,
            ERR_NICKNAMEINUSE,
            &[nick.as_ref()],
            "Nickname is already in use",
        );
        state.nick = None;
        return;
    }

    state.registered = true;

    let nick_str = nick.as_ref();
    let user_host = format!("{username}@{}", state.hostname);

    // RPL_WELCOME (001)
    send_numeric(
        sender,
        RPL_WELCOME,
        &[nick_str],
        &format!("Welcome to the pirc network, {nick_str}!{user_host}"),
    );

    // RPL_YOURHOST (002)
    send_numeric(
        sender,
        RPL_YOURHOST,
        &[nick_str],
        &format!("Your host is {SERVER_NAME}, running version {SERVER_VERSION}"),
    );

    // RPL_CREATED (003)
    send_numeric(
        sender,
        RPL_CREATED,
        &[nick_str],
        &format!("This server was created {SERVER_NAME}"),
    );

    // MOTD or ERR_NOMOTD
    send_motd(sender, nick_str, config);
}

fn send_motd(sender: &mpsc::UnboundedSender<Message>, nick: &str, config: &ServerConfig) {
    let file_motd;
    let motd = if let Some(ref text) = config.motd.text {
        Some(text.as_str())
    } else if let Some(ref path) = config.motd.path {
        file_motd = std::fs::read_to_string(path).ok();
        file_motd.as_deref()
    } else {
        None
    };

    match motd {
        Some(text) => {
            send_numeric(
                sender,
                pirc_protocol::numeric::RPL_MOTDSTART,
                &[nick],
                &format!("- {SERVER_NAME} Message of the day -"),
            );
            for line in text.lines() {
                send_numeric(
                    sender,
                    pirc_protocol::numeric::RPL_MOTD,
                    &[nick],
                    &format!("- {line}"),
                );
            }
            send_numeric(
                sender,
                pirc_protocol::numeric::RPL_ENDOFMOTD,
                &[nick],
                "End of /MOTD command",
            );
        }
        None => {
            send_numeric(sender, ERR_NOMOTD, &[nick], "MOTD File is missing");
        }
    }
}

fn handle_ping(msg: &Message, sender: &mpsc::UnboundedSender<Message>) {
    if let Some(token) = msg.params.first() {
        let pong = Message::builder(Command::Pong)
            .prefix(Prefix::server(SERVER_NAME))
            .param(token)
            .build();
        let _ = sender.send(pong);
    }
}

fn send_numeric(
    sender: &mpsc::UnboundedSender<Message>,
    code: u16,
    params: &[&str],
    trailing: &str,
) {
    let mut builder = Message::builder(Command::Numeric(code)).prefix(Prefix::server(SERVER_NAME));
    for p in params {
        builder = builder.param(p);
    }
    builder = builder.trailing(trailing);
    let _ = sender.send(builder.build());
}

#[cfg(test)]
mod tests {
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
}
