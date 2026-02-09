use std::fmt;

use crate::command::Command;
use crate::prefix::Prefix;

/// Maximum number of parameters in an IRC message (per RFC 2812).
pub const MAX_PARAMS: usize = 15;

/// A parsed IRC protocol message.
///
/// Represents the wire format: `:<prefix> <command> <params...> :<trailing>\r\n`
///
/// - `prefix` is optional and identifies the message source.
/// - `command` is the IRC command or numeric reply.
/// - `params` holds up to [`MAX_PARAMS`] parameters. The last parameter may
///   contain spaces if it was a "trailing" parameter (prefixed with `:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Optional message source (server or user).
    pub prefix: Option<Prefix>,
    /// The command or numeric reply.
    pub command: Command,
    /// Command parameters (max 15).
    pub params: Vec<String>,
}

impl Message {
    /// Create a new message with no prefix.
    pub fn new(command: Command, params: Vec<String>) -> Self {
        Self {
            prefix: None,
            command,
            params,
        }
    }

    /// Create a new message with a prefix.
    pub fn with_prefix(prefix: Prefix, command: Command, params: Vec<String>) -> Self {
        Self {
            prefix: Some(prefix),
            command,
            params,
        }
    }

    /// Returns `true` if this message is a numeric reply.
    pub fn is_numeric(&self) -> bool {
        matches!(self.command, Command::Numeric(_))
    }

    /// Returns the numeric code if this is a numeric reply, or `None`.
    pub fn numeric_code(&self) -> Option<u16> {
        match self.command {
            Command::Numeric(code) => Some(code),
            _ => None,
        }
    }

    /// Returns the trailing (last) parameter, if any.
    pub fn trailing(&self) -> Option<&str> {
        self.params.last().map(String::as_str)
    }

    /// Create a builder for constructing a message.
    ///
    /// # Examples
    ///
    /// ```
    /// use pirc_protocol::{Command, Message, Prefix};
    ///
    /// let msg = Message::builder(Command::Privmsg)
    ///     .prefix(Prefix::Server("irc.example.com".to_owned()))
    ///     .param("#general")
    ///     .trailing("Hello, world!")
    ///     .build();
    ///
    /// assert_eq!(msg.to_string(), ":irc.example.com PRIVMSG #general :Hello, world!");
    /// ```
    pub fn builder(command: Command) -> MessageBuilder {
        MessageBuilder {
            prefix: None,
            command,
            params: Vec::new(),
        }
    }
}

/// Builder for constructing [`Message`] values ergonomically.
///
/// Obtained via [`Message::builder`].
#[derive(Debug, Clone)]
#[must_use]
pub struct MessageBuilder {
    prefix: Option<Prefix>,
    command: Command,
    params: Vec<String>,
}

impl MessageBuilder {
    /// Set the message prefix (source).
    pub fn prefix(mut self, prefix: Prefix) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Append a parameter.
    pub fn param(mut self, value: &str) -> Self {
        self.params.push(value.to_owned());
        self
    }

    /// Append a trailing parameter (the last parameter, which may contain spaces).
    ///
    /// This is semantically identical to [`param`](Self::param) — the
    /// `Display` implementation decides whether to format the last parameter
    /// with a `:` prefix based on its content. This method exists for clarity
    /// when building messages.
    pub fn trailing(self, value: &str) -> Self {
        self.param(value)
    }

    /// Consume the builder and produce the [`Message`].
    pub fn build(self) -> Message {
        Message {
            prefix: self.prefix,
            command: self.command,
            params: self.params,
        }
    }
}

impl fmt::Display for Message {
    /// Formats the message in IRC wire format.
    ///
    /// The trailing `\r\n` is **not** included — callers should append it
    /// when writing to a transport.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(prefix) = &self.prefix {
            write!(f, ":{prefix} ")?;
        }
        write!(f, "{}", self.command)?;

        if !self.params.is_empty() {
            // All params except possibly the last are simple (no spaces).
            let (last, rest) = self.params.split_last().expect("non-empty params");
            for param in rest {
                write!(f, " {param}")?;
            }
            // The last parameter is written as trailing if it contains
            // spaces or is empty, or if it starts with ':'.
            if last.is_empty() || last.contains(' ') || last.starts_with(':') {
                write!(f, " :{last}")?;
            } else {
                write!(f, " {last}")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric;
    use pirc_common::Nickname;

    // ---- Construction ----

    #[test]
    fn message_new_no_params() {
        let msg = Message::new(Command::Quit, vec![]);
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Quit);
        assert!(msg.params.is_empty());
    }

    #[test]
    fn message_new_with_params() {
        let msg = Message::new(
            Command::Privmsg,
            vec!["#general".to_owned(), "Hello, world!".to_owned()],
        );
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Privmsg);
        assert_eq!(msg.params.len(), 2);
        assert_eq!(msg.params[0], "#general");
        assert_eq!(msg.params[1], "Hello, world!");
    }

    #[test]
    fn message_with_server_prefix() {
        let msg = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Notice,
            vec!["*".to_owned(), "Server restarting".to_owned()],
        );
        assert_eq!(
            msg.prefix,
            Some(Prefix::Server("irc.example.com".to_owned()))
        );
        assert_eq!(msg.command, Command::Notice);
    }

    #[test]
    fn message_with_user_prefix() {
        let msg = Message::with_prefix(
            Prefix::User {
                nick: Nickname::new("alice").unwrap(),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            },
            Command::Privmsg,
            vec!["#general".to_owned(), "hi there".to_owned()],
        );
        assert!(msg.prefix.is_some());
        assert_eq!(msg.command, Command::Privmsg);
    }

    // ---- Helpers ----

    #[test]
    fn is_numeric_true() {
        let msg = Message::new(Command::Numeric(numeric::RPL_WELCOME), vec![]);
        assert!(msg.is_numeric());
    }

    #[test]
    fn is_numeric_false() {
        let msg = Message::new(Command::Privmsg, vec![]);
        assert!(!msg.is_numeric());
    }

    #[test]
    fn numeric_code_some() {
        let msg = Message::new(Command::Numeric(433), vec![]);
        assert_eq!(msg.numeric_code(), Some(433));
    }

    #[test]
    fn numeric_code_none() {
        let msg = Message::new(Command::Join, vec![]);
        assert_eq!(msg.numeric_code(), None);
    }

    #[test]
    fn trailing_returns_last_param() {
        let msg = Message::new(
            Command::Privmsg,
            vec!["#general".to_owned(), "Hello!".to_owned()],
        );
        assert_eq!(msg.trailing(), Some("Hello!"));
    }

    #[test]
    fn trailing_returns_none_when_no_params() {
        let msg = Message::new(Command::List, vec![]);
        assert_eq!(msg.trailing(), None);
    }

    // ---- Display (wire format) ----

    #[test]
    fn display_simple_command() {
        let msg = Message::new(Command::Quit, vec![]);
        assert_eq!(msg.to_string(), "QUIT");
    }

    #[test]
    fn display_command_with_simple_param() {
        let msg = Message::new(Command::Join, vec!["#general".to_owned()]);
        assert_eq!(msg.to_string(), "JOIN #general");
    }

    #[test]
    fn display_command_with_trailing_param() {
        let msg = Message::new(
            Command::Privmsg,
            vec!["#general".to_owned(), "Hello, world!".to_owned()],
        );
        assert_eq!(msg.to_string(), "PRIVMSG #general :Hello, world!");
    }

    #[test]
    fn display_with_prefix() {
        let msg = Message::with_prefix(
            Prefix::User {
                nick: Nickname::new("alice").unwrap(),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            },
            Command::Privmsg,
            vec!["#general".to_owned(), "hello".to_owned()],
        );
        assert_eq!(
            msg.to_string(),
            ":alice!alice@example.com PRIVMSG #general hello"
        );
    }

    #[test]
    fn display_numeric_reply() {
        let msg = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Numeric(numeric::RPL_WELCOME),
            vec![
                "alice".to_owned(),
                "Welcome to the pirc network!".to_owned(),
            ],
        );
        assert_eq!(
            msg.to_string(),
            ":irc.example.com 001 alice :Welcome to the pirc network!"
        );
    }

    #[test]
    fn display_ping() {
        let msg = Message::new(Command::Ping, vec!["irc.example.com".to_owned()]);
        assert_eq!(msg.to_string(), "PING irc.example.com");
    }

    #[test]
    fn display_trailing_starts_with_colon() {
        let msg = Message::new(Command::Privmsg, vec!["#test".to_owned(), ":)".to_owned()]);
        assert_eq!(msg.to_string(), "PRIVMSG #test ::)");
    }

    #[test]
    fn display_empty_trailing() {
        let msg = Message::new(Command::Topic, vec!["#test".to_owned(), String::new()]);
        assert_eq!(msg.to_string(), "TOPIC #test :");
    }

    // ---- Equality ----

    #[test]
    fn message_equality() {
        let a = Message::new(Command::Nick, vec!["alice".to_owned()]);
        let b = Message::new(Command::Nick, vec!["alice".to_owned()]);
        assert_eq!(a, b);
    }

    #[test]
    fn message_inequality_different_command() {
        let a = Message::new(Command::Nick, vec!["alice".to_owned()]);
        let b = Message::new(Command::Join, vec!["alice".to_owned()]);
        assert_ne!(a, b);
    }

    #[test]
    fn message_inequality_different_params() {
        let a = Message::new(Command::Nick, vec!["alice".to_owned()]);
        let b = Message::new(Command::Nick, vec!["bob".to_owned()]);
        assert_ne!(a, b);
    }

    #[test]
    fn message_inequality_different_prefix() {
        let a = Message::new(Command::Nick, vec!["alice".to_owned()]);
        let b = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Nick,
            vec!["alice".to_owned()],
        );
        assert_ne!(a, b);
    }

    // ---- Clone ----

    #[test]
    fn message_clone() {
        let msg = Message::with_prefix(
            Prefix::User {
                nick: Nickname::new("alice").unwrap(),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            },
            Command::Privmsg,
            vec!["#general".to_owned(), "hello".to_owned()],
        );
        let cloned = msg.clone();
        assert_eq!(msg, cloned);
    }

    // ---- Debug ----

    #[test]
    fn message_debug() {
        let msg = Message::new(Command::Ping, vec!["test".to_owned()]);
        let debug = format!("{msg:?}");
        assert!(debug.contains("Ping"));
        assert!(debug.contains("test"));
    }

    // ---- Builder ----

    #[test]
    fn builder_simple_command() {
        let msg = Message::builder(Command::Quit).build();
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Quit);
        assert!(msg.params.is_empty());
    }

    #[test]
    fn builder_with_prefix_and_params() {
        let msg = Message::builder(Command::Privmsg)
            .prefix(Prefix::user("alice", "alice", "example.com"))
            .param("#general")
            .trailing("Hello, world!")
            .build();
        assert_eq!(
            msg.prefix,
            Some(Prefix::User {
                nick: Nickname::new("alice").unwrap(),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            })
        );
        assert_eq!(msg.command, Command::Privmsg);
        assert_eq!(msg.params, vec!["#general", "Hello, world!"]);
    }

    #[test]
    fn builder_with_server_prefix() {
        let msg = Message::builder(Command::Notice)
            .prefix(Prefix::server("irc.example.com"))
            .param("*")
            .trailing("Server notice")
            .build();
        assert_eq!(
            msg.prefix,
            Some(Prefix::Server("irc.example.com".to_owned()))
        );
        assert_eq!(msg.to_string(), ":irc.example.com NOTICE * :Server notice");
    }

    #[test]
    fn builder_multiple_params() {
        let msg = Message::builder(Command::Mode)
            .param("#channel")
            .param("+o")
            .param("nick")
            .build();
        assert_eq!(msg.params, vec!["#channel", "+o", "nick"]);
        assert_eq!(msg.to_string(), "MODE #channel +o nick");
    }

    #[test]
    fn builder_numeric_reply() {
        let msg = Message::builder(Command::Numeric(numeric::RPL_WELCOME))
            .prefix(Prefix::server("irc.example.com"))
            .param("alice")
            .trailing("Welcome to pirc!")
            .build();
        assert_eq!(
            msg.to_string(),
            ":irc.example.com 001 alice :Welcome to pirc!"
        );
    }

    #[test]
    fn builder_no_prefix() {
        let msg = Message::builder(Command::Ping).param("server1").build();
        assert!(msg.prefix.is_none());
        assert_eq!(msg.to_string(), "PING server1");
    }

    // ---- Display: every command type ----

    #[test]
    fn display_nick() {
        let msg = Message::builder(Command::Nick).param("newnick").build();
        assert_eq!(msg.to_string(), "NICK newnick");
    }

    #[test]
    fn display_join() {
        let msg = Message::builder(Command::Join).param("#channel").build();
        assert_eq!(msg.to_string(), "JOIN #channel");
    }

    #[test]
    fn display_part_with_message() {
        let msg = Message::builder(Command::Part)
            .prefix(Prefix::user("nick", "user", "host"))
            .param("#channel")
            .trailing("Goodbye")
            .build();
        assert_eq!(msg.to_string(), ":nick!user@host PART #channel Goodbye");
    }

    #[test]
    fn display_privmsg() {
        let msg = Message::builder(Command::Privmsg)
            .param("#channel")
            .trailing("Hello, world!")
            .build();
        assert_eq!(msg.to_string(), "PRIVMSG #channel :Hello, world!");
    }

    #[test]
    fn display_notice() {
        let msg = Message::builder(Command::Notice)
            .prefix(Prefix::server("server"))
            .param("*")
            .trailing("Welcome")
            .build();
        assert_eq!(msg.to_string(), ":server NOTICE * Welcome");
    }

    #[test]
    fn display_quit_no_params() {
        let msg = Message::builder(Command::Quit).build();
        assert_eq!(msg.to_string(), "QUIT");
    }

    #[test]
    fn display_quit_with_message() {
        let msg = Message::builder(Command::Quit)
            .trailing("Leaving the network")
            .build();
        assert_eq!(msg.to_string(), "QUIT :Leaving the network");
    }

    #[test]
    fn display_kick() {
        let msg = Message::builder(Command::Kick)
            .prefix(Prefix::user("op", "op", "host"))
            .param("#channel")
            .param("baduser")
            .trailing("Behave yourself")
            .build();
        assert_eq!(
            msg.to_string(),
            ":op!op@host KICK #channel baduser :Behave yourself"
        );
    }

    #[test]
    fn display_ban() {
        let msg = Message::builder(Command::Ban)
            .param("#channel")
            .param("baduser")
            .build();
        assert_eq!(msg.to_string(), "BAN #channel baduser");
    }

    #[test]
    fn display_mode() {
        let msg = Message::builder(Command::Mode)
            .param("#channel")
            .param("+o")
            .param("nick")
            .build();
        assert_eq!(msg.to_string(), "MODE #channel +o nick");
    }

    #[test]
    fn display_topic_set() {
        let msg = Message::builder(Command::Topic)
            .param("#channel")
            .trailing("New topic here")
            .build();
        assert_eq!(msg.to_string(), "TOPIC #channel :New topic here");
    }

    #[test]
    fn display_topic_query() {
        let msg = Message::builder(Command::Topic).param("#channel").build();
        assert_eq!(msg.to_string(), "TOPIC #channel");
    }

    #[test]
    fn display_whois() {
        let msg = Message::builder(Command::Whois).param("nick").build();
        assert_eq!(msg.to_string(), "WHOIS nick");
    }

    #[test]
    fn display_list_no_params() {
        let msg = Message::builder(Command::List).build();
        assert_eq!(msg.to_string(), "LIST");
    }

    #[test]
    fn display_invite() {
        let msg = Message::builder(Command::Invite)
            .param("target")
            .param("#channel")
            .build();
        assert_eq!(msg.to_string(), "INVITE target #channel");
    }

    #[test]
    fn display_away() {
        let msg = Message::builder(Command::Away)
            .trailing("Gone fishing")
            .build();
        assert_eq!(msg.to_string(), "AWAY :Gone fishing");
    }

    #[test]
    fn display_pong() {
        let msg = Message::builder(Command::Pong).param("server1").build();
        assert_eq!(msg.to_string(), "PONG server1");
    }

    #[test]
    fn display_error() {
        let msg = Message::builder(Command::Error)
            .trailing("Closing link")
            .build();
        assert_eq!(msg.to_string(), "ERROR :Closing link");
    }

    #[test]
    fn display_numeric_zero_padded() {
        let msg = Message::builder(Command::Numeric(1))
            .prefix(Prefix::server("srv"))
            .param("nick")
            .trailing("Welcome")
            .build();
        assert_eq!(msg.to_string(), ":srv 001 nick Welcome");
    }

    #[test]
    fn display_numeric_three_digit() {
        let msg = Message::builder(Command::Numeric(433))
            .prefix(Prefix::server("srv"))
            .param("*")
            .param("nick")
            .trailing("Nickname is already in use")
            .build();
        assert_eq!(
            msg.to_string(),
            ":srv 433 * nick :Nickname is already in use"
        );
    }

    // ---- Round-trip: build → serialize → parse ----

    #[test]
    fn roundtrip_builder_privmsg() {
        let msg = Message::builder(Command::Privmsg)
            .prefix(Prefix::user("alice", "alice", "example.com"))
            .param("#general")
            .trailing("Hello, world!")
            .build();
        let wire = format!("{msg}\r\n");
        let parsed = crate::parse(&wire).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn roundtrip_builder_numeric() {
        let msg = Message::builder(Command::Numeric(353))
            .prefix(Prefix::server("irc.example.com"))
            .param("alice")
            .param("=")
            .param("#channel")
            .trailing("nick1 nick2 nick3")
            .build();
        let wire = format!("{msg}\r\n");
        let parsed = crate::parse(&wire).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn roundtrip_builder_quit() {
        let msg = Message::builder(Command::Quit)
            .prefix(Prefix::user("bob", "bob", "host.com"))
            .trailing("Leaving")
            .build();
        let wire = format!("{msg}\r\n");
        let parsed = crate::parse(&wire).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn roundtrip_builder_mode() {
        let msg = Message::builder(Command::Mode)
            .param("#channel")
            .param("+o")
            .param("nick")
            .build();
        let wire = format!("{msg}\r\n");
        let parsed = crate::parse(&wire).unwrap();
        assert_eq!(parsed, msg);
    }

    // ---- Round-trip: parse → serialize → parse ----

    fn assert_parse_roundtrip(input: &str) {
        let parsed = crate::parse(input).unwrap();
        let serialized = format!("{parsed}\r\n");
        let reparsed = crate::parse(&serialized).unwrap();
        assert_eq!(parsed, reparsed, "round-trip failed for: {input}");
    }

    #[test]
    fn roundtrip_parse_nick() {
        assert_parse_roundtrip("NICK newnick\r\n");
    }

    #[test]
    fn roundtrip_parse_join() {
        assert_parse_roundtrip(":nick!user@host JOIN #channel\r\n");
    }

    #[test]
    fn roundtrip_parse_part() {
        assert_parse_roundtrip(":nick!user@host PART #channel :Goodbye\r\n");
    }

    #[test]
    fn roundtrip_parse_privmsg() {
        assert_parse_roundtrip(":nick!user@host PRIVMSG #channel :Hello, world!\r\n");
    }

    #[test]
    fn roundtrip_parse_notice() {
        assert_parse_roundtrip(":server NOTICE * :Welcome\r\n");
    }

    #[test]
    fn roundtrip_parse_quit() {
        assert_parse_roundtrip(":nick!user@host QUIT :Leaving\r\n");
    }

    #[test]
    fn roundtrip_parse_kick() {
        assert_parse_roundtrip(":nick!user@host KICK #channel target :Reason\r\n");
    }

    #[test]
    fn roundtrip_parse_ban() {
        assert_parse_roundtrip(":nick!user@host BAN #channel target\r\n");
    }

    #[test]
    fn roundtrip_parse_mode() {
        assert_parse_roundtrip("MODE #channel +o nick\r\n");
    }

    #[test]
    fn roundtrip_parse_topic() {
        assert_parse_roundtrip(":nick!user@host TOPIC #channel :New topic\r\n");
    }

    #[test]
    fn roundtrip_parse_whois() {
        assert_parse_roundtrip("WHOIS nick\r\n");
    }

    #[test]
    fn roundtrip_parse_list() {
        assert_parse_roundtrip("LIST\r\n");
    }

    #[test]
    fn roundtrip_parse_invite() {
        assert_parse_roundtrip(":nick!user@host INVITE target #channel\r\n");
    }

    #[test]
    fn roundtrip_parse_away() {
        assert_parse_roundtrip("AWAY :Gone fishing\r\n");
    }

    #[test]
    fn roundtrip_parse_ping() {
        assert_parse_roundtrip("PING :server1\r\n");
    }

    #[test]
    fn roundtrip_parse_pong() {
        assert_parse_roundtrip("PONG server1\r\n");
    }

    #[test]
    fn roundtrip_parse_error() {
        assert_parse_roundtrip("ERROR :Closing link\r\n");
    }

    #[test]
    fn roundtrip_parse_numeric_001() {
        assert_parse_roundtrip(":server 001 nick :Welcome to the pirc network\r\n");
    }

    #[test]
    fn roundtrip_parse_numeric_353() {
        assert_parse_roundtrip(":server 353 nick = #channel :nick1 nick2 nick3\r\n");
    }

    #[test]
    fn roundtrip_parse_numeric_433() {
        assert_parse_roundtrip(":server 433 * nick :Nickname is already in use\r\n");
    }

    #[test]
    fn roundtrip_parse_trailing_colon() {
        assert_parse_roundtrip("PRIVMSG #test ::)\r\n");
    }

    #[test]
    fn roundtrip_parse_empty_trailing() {
        assert_parse_roundtrip("TOPIC #test :\r\n");
    }
}
