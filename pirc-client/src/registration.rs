use pirc_protocol::numeric;
use pirc_protocol::{Command, Message};

/// Tracks the IRC registration handshake and nick-collision recovery.
///
/// On TCP connect the caller should call [`RegistrationState::nick_message`] and
/// [`RegistrationState::user_message`] to obtain the NICK/USER messages to send.
/// Incoming server messages are fed to [`RegistrationState::handle_message`] which
/// returns a [`RegistrationEvent`] describing what the event loop should do.
pub struct RegistrationState {
    /// The nick currently being tried.
    current_nick: String,
    /// Username for the USER command.
    username: String,
    /// Realname for the USER command (trailing parameter).
    realname: String,
    /// Alt nicks to cycle through on collision.
    alt_nicks: Vec<String>,
    /// Index into `alt_nicks` for the next fallback. Once exhausted we append
    /// underscores to the original nick.
    alt_nick_index: usize,
    /// The original (primary) nick — used as the base for underscore fallback.
    primary_nick: String,
    /// Number of underscore suffixes appended after alt_nicks are exhausted.
    underscore_count: usize,
}

/// Outcome of processing a server message during registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationEvent {
    /// Registration is complete. Contains the server name extracted from
    /// the RPL_WELCOME prefix and the welcome text.
    Welcome {
        server_name: String,
        nick: String,
        message: String,
    },
    /// An informational numeric (002, 003, 004) — display in status buffer.
    Info(String),
    /// A nick collision occurred; the contained [`Message`] is the NICK
    /// command to re-send with the next candidate nick.
    NickRetry {
        new_nick: String,
        nick_message: Message,
    },
    /// Erroneous nickname (432) — display the error, stay in Registering.
    NickError(String),
    /// The message is not registration-related; let the normal handler deal
    /// with it.
    Unhandled,
}

impl RegistrationState {
    /// Build a new registration state from config values.
    ///
    /// - `nick`: primary nick (already resolved from config, with fallback).
    /// - `alt_nicks`: alternative nicks from config.
    /// - `realname`: realname for the USER command (defaults to nick if empty).
    pub fn new(nick: String, alt_nicks: Vec<String>, username: String, realname: String) -> Self {
        Self {
            primary_nick: nick.clone(),
            current_nick: nick,
            username,
            realname,
            alt_nicks,
            alt_nick_index: 0,
            underscore_count: 0,
        }
    }

    /// The nick currently being attempted.
    pub fn current_nick(&self) -> &str {
        &self.current_nick
    }

    /// Build the NICK message to send at the start of registration.
    pub fn nick_message(&self) -> Message {
        Message::new(Command::Nick, vec![self.current_nick.clone()])
    }

    /// Build the USER message to send at the start of registration.
    pub fn user_message(&self) -> Message {
        Message::new(
            Command::User,
            vec![
                self.username.clone(),
                "0".to_string(),
                "*".to_string(),
                self.realname.clone(),
            ],
        )
    }

    /// Process an inbound server message during registration.
    ///
    /// Returns `RegistrationEvent::Unhandled` for messages that are not part
    /// of the registration flow — the caller should pass those to the normal
    /// message handler.
    pub fn handle_message(&mut self, msg: &Message) -> RegistrationEvent {
        match msg.command {
            // RPL_WELCOME (001) — registration complete
            Command::Numeric(numeric::RPL_WELCOME) => {
                let server_name = msg
                    .prefix
                    .as_ref()
                    .map_or_else(|| String::new(), ToString::to_string);

                // The server may confirm a different nick in param[0]
                if let Some(confirmed_nick) = msg.params.first() {
                    if !confirmed_nick.is_empty() {
                        self.current_nick = confirmed_nick.clone();
                    }
                }

                let welcome_text = msg.trailing().unwrap_or_default().to_string();
                // If trailing is the same as first param (single-param message),
                // use the full params joined
                let message = if msg.params.len() >= 2 {
                    welcome_text
                } else {
                    msg.params.last().cloned().unwrap_or_default()
                };

                RegistrationEvent::Welcome {
                    server_name,
                    nick: self.current_nick.clone(),
                    message,
                }
            }

            // RPL_YOURHOST (002), RPL_CREATED (003), RPL_MYINFO (004)
            Command::Numeric(numeric::RPL_YOURHOST)
            | Command::Numeric(numeric::RPL_CREATED)
            | Command::Numeric(numeric::RPL_MYINFO) => {
                let text = msg.trailing().unwrap_or_default().to_string();
                RegistrationEvent::Info(text)
            }

            // ERR_NICKNAMEINUSE (433) or ERR_NICKCOLLISION (436)
            Command::Numeric(numeric::ERR_NICKNAMEINUSE)
            | Command::Numeric(numeric::ERR_NICKCOLLISION) => {
                let new_nick = self.next_nick();
                let nick_message = Message::new(Command::Nick, vec![new_nick.clone()]);
                RegistrationEvent::NickRetry {
                    new_nick,
                    nick_message,
                }
            }

            // ERR_ERRONEUSNICKNAME (432)
            Command::Numeric(numeric::ERR_ERRONEUSNICKNAME) => {
                let reason = msg.trailing().unwrap_or("Erroneous nickname").to_string();
                RegistrationEvent::NickError(reason)
            }

            _ => RegistrationEvent::Unhandled,
        }
    }

    /// Advance to the next candidate nick. First cycles through alt_nicks,
    /// then appends underscores to the primary nick.
    fn next_nick(&mut self) -> String {
        if self.alt_nick_index < self.alt_nicks.len() {
            let nick = self.alt_nicks[self.alt_nick_index].clone();
            self.alt_nick_index += 1;
            self.current_nick = nick;
        } else {
            self.underscore_count += 1;
            let nick = format!("{}{}", self.primary_nick, "_".repeat(self.underscore_count));
            self.current_nick = nick;
        }
        self.current_nick.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirc_protocol::Prefix;

    fn make_reg() -> RegistrationState {
        RegistrationState::new(
            "mynick".into(),
            vec!["alt1".into(), "alt2".into()],
            "myuser".into(),
            "My Real Name".into(),
        )
    }

    fn make_reg_no_alts() -> RegistrationState {
        RegistrationState::new(
            "mynick".into(),
            vec![],
            "myuser".into(),
            "My Real Name".into(),
        )
    }

    // ── Construction ─────────────────────────────────────────────

    #[test]
    fn new_sets_current_nick() {
        let reg = make_reg();
        assert_eq!(reg.current_nick(), "mynick");
    }

    #[test]
    fn nick_message_uses_current_nick() {
        let reg = make_reg();
        let msg = reg.nick_message();
        assert_eq!(msg.command, Command::Nick);
        assert_eq!(msg.params, vec!["mynick"]);
    }

    #[test]
    fn user_message_has_correct_params() {
        let reg = make_reg();
        let msg = reg.user_message();
        assert_eq!(msg.command, Command::User);
        assert_eq!(msg.params.len(), 4);
        assert_eq!(msg.params[0], "myuser");
        assert_eq!(msg.params[1], "0");
        assert_eq!(msg.params[2], "*");
        assert_eq!(msg.params[3], "My Real Name");
    }

    // ── RPL_WELCOME (001) ────────────────────────────────────────

    #[test]
    fn handle_rpl_welcome() {
        let mut reg = make_reg();
        let msg = Message::with_prefix(
            Prefix::Server("irc.test.net".into()),
            Command::Numeric(1),
            vec!["mynick".into(), "Welcome to the test network!".into()],
        );
        let event = reg.handle_message(&msg);
        assert_eq!(
            event,
            RegistrationEvent::Welcome {
                server_name: "irc.test.net".into(),
                nick: "mynick".into(),
                message: "Welcome to the test network!".into(),
            }
        );
    }

    #[test]
    fn handle_rpl_welcome_updates_nick_from_server() {
        let mut reg = make_reg();
        let msg = Message::with_prefix(
            Prefix::Server("irc.test.net".into()),
            Command::Numeric(1),
            vec!["servernick".into(), "Welcome!".into()],
        );
        let event = reg.handle_message(&msg);
        assert_eq!(reg.current_nick(), "servernick");
        match event {
            RegistrationEvent::Welcome { nick, .. } => assert_eq!(nick, "servernick"),
            _ => panic!("expected Welcome"),
        }
    }

    #[test]
    fn handle_rpl_welcome_no_prefix() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(1),
            vec!["mynick".into(), "Welcome!".into()],
        );
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::Welcome { server_name, .. } => {
                assert_eq!(server_name, "");
            }
            _ => panic!("expected Welcome"),
        }
    }

    // ── Info numerics (002, 003, 004) ────────────────────────────

    #[test]
    fn handle_rpl_yourhost() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(2),
            vec!["mynick".into(), "Your host is irc.test.net".into()],
        );
        let event = reg.handle_message(&msg);
        assert_eq!(
            event,
            RegistrationEvent::Info("Your host is irc.test.net".into())
        );
    }

    #[test]
    fn handle_rpl_created() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(3),
            vec!["mynick".into(), "This server was created today".into()],
        );
        let event = reg.handle_message(&msg);
        assert_eq!(
            event,
            RegistrationEvent::Info("This server was created today".into())
        );
    }

    #[test]
    fn handle_rpl_myinfo() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(4),
            vec![
                "mynick".into(),
                "irc.test.net".into(),
                "ircd-1.0".into(),
                "oiwszcrkfydnxbauglZCD".into(),
            ],
        );
        let event = reg.handle_message(&msg);
        // trailing() returns last param
        assert_eq!(
            event,
            RegistrationEvent::Info("oiwszcrkfydnxbauglZCD".into())
        );
    }

    // ── Nick collision (433, 436) ────────────────────────────────

    #[test]
    fn handle_nick_in_use_cycles_alt_nicks() {
        let mut reg = make_reg();

        // First collision → alt1
        let msg = Message::new(
            Command::Numeric(433),
            vec![
                "*".into(),
                "mynick".into(),
                "Nickname is already in use".into(),
            ],
        );
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry {
                new_nick,
                nick_message,
            } => {
                assert_eq!(new_nick, "alt1");
                assert_eq!(nick_message.params, vec!["alt1"]);
            }
            _ => panic!("expected NickRetry"),
        }
        assert_eq!(reg.current_nick(), "alt1");

        // Second collision → alt2
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry { new_nick, .. } => {
                assert_eq!(new_nick, "alt2");
            }
            _ => panic!("expected NickRetry"),
        }
        assert_eq!(reg.current_nick(), "alt2");

        // Third collision → underscore fallback: mynick_
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry { new_nick, .. } => {
                assert_eq!(new_nick, "mynick_");
            }
            _ => panic!("expected NickRetry"),
        }
        assert_eq!(reg.current_nick(), "mynick_");

        // Fourth collision → mynick__
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry { new_nick, .. } => {
                assert_eq!(new_nick, "mynick__");
            }
            _ => panic!("expected NickRetry"),
        }
    }

    #[test]
    fn handle_nick_collision_436() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(436),
            vec!["*".into(), "mynick".into(), "Nick collision".into()],
        );
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry { new_nick, .. } => {
                assert_eq!(new_nick, "alt1");
            }
            _ => panic!("expected NickRetry"),
        }
    }

    #[test]
    fn handle_nick_in_use_no_alts_goes_straight_to_underscore() {
        let mut reg = make_reg_no_alts();
        let msg = Message::new(
            Command::Numeric(433),
            vec![
                "*".into(),
                "mynick".into(),
                "Nickname is already in use".into(),
            ],
        );
        let event = reg.handle_message(&msg);
        match event {
            RegistrationEvent::NickRetry { new_nick, .. } => {
                assert_eq!(new_nick, "mynick_");
            }
            _ => panic!("expected NickRetry"),
        }
    }

    // ── ERR_ERRONEUSNICKNAME (432) ───────────────────────────────

    #[test]
    fn handle_erroneous_nickname() {
        let mut reg = make_reg();
        let msg = Message::new(
            Command::Numeric(432),
            vec!["*".into(), "bad!nick".into(), "Erroneous Nickname".into()],
        );
        let event = reg.handle_message(&msg);
        assert_eq!(
            event,
            RegistrationEvent::NickError("Erroneous Nickname".into())
        );
    }

    // ── Unhandled messages ───────────────────────────────────────

    #[test]
    fn handle_unrelated_message_returns_unhandled() {
        let mut reg = make_reg();
        let msg = Message::new(Command::Ping, vec!["server".into()]);
        assert_eq!(reg.handle_message(&msg), RegistrationEvent::Unhandled);
    }

    #[test]
    fn handle_privmsg_returns_unhandled() {
        let mut reg = make_reg();
        let msg = Message::new(Command::Privmsg, vec!["#channel".into(), "hello".into()]);
        assert_eq!(reg.handle_message(&msg), RegistrationEvent::Unhandled);
    }
}
