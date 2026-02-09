use std::fmt;

/// Subcommands for the `PIRC` extension command namespace.
///
/// The PIRC namespace groups pirc-specific protocol extensions that go beyond
/// standard IRC. Each subcommand has its own wire-format keyword that appears
/// as the first parameter after `PIRC`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PircSubcommand {
    /// Protocol version announcement: `PIRC VERSION <version>`.
    Version,
    /// Capability announcement (for future use): `PIRC CAP <capability> [...]`.
    Cap,
}

impl PircSubcommand {
    /// Returns the wire-format keyword for this subcommand.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Version => "VERSION",
            Self::Cap => "CAP",
        }
    }

    /// Parses a subcommand keyword string into a `PircSubcommand`.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "VERSION" => Some(Self::Version),
            "CAP" => Some(Self::Cap),
            _ => None,
        }
    }
}

impl fmt::Display for PircSubcommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// IRC-style protocol commands supported by pirc.
///
/// Each variant represents a distinct protocol action. The wire format uses
/// the uppercase keyword (e.g., `PRIVMSG`, `JOIN`) as the command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Set or change nickname.
    Nick,
    /// Join a channel.
    Join,
    /// Leave a channel.
    Part,
    /// Send a message to a channel or user.
    Privmsg,
    /// Send a notice (no auto-reply expected).
    Notice,
    /// Disconnect from the server.
    Quit,
    /// Kick a user from a channel.
    Kick,
    /// Ban a user from a channel (pirc extension).
    Ban,
    /// Set channel or user modes.
    Mode,
    /// Get or set a channel topic.
    Topic,
    /// Query user information.
    Whois,
    /// List channels.
    List,
    /// Invite a user to a channel.
    Invite,
    /// Set away status.
    Away,
    /// Connection keepalive request.
    Ping,
    /// Connection keepalive response.
    Pong,
    /// Server error message.
    Error,
    /// A numeric reply code (e.g., 001 for `RPL_WELCOME`).
    Numeric(u16),
    /// Pirc extension command with a subcommand (e.g., `PIRC VERSION 1.0`).
    Pirc(PircSubcommand),
}

impl Command {
    /// Returns the wire-format string for this command.
    ///
    /// For named commands, returns the uppercase keyword. For numeric
    /// replies, returns the zero-padded three-digit code.
    pub fn as_str(&self) -> String {
        match self {
            Self::Nick => "NICK".to_owned(),
            Self::Join => "JOIN".to_owned(),
            Self::Part => "PART".to_owned(),
            Self::Privmsg => "PRIVMSG".to_owned(),
            Self::Notice => "NOTICE".to_owned(),
            Self::Quit => "QUIT".to_owned(),
            Self::Kick => "KICK".to_owned(),
            Self::Ban => "BAN".to_owned(),
            Self::Mode => "MODE".to_owned(),
            Self::Topic => "TOPIC".to_owned(),
            Self::Whois => "WHOIS".to_owned(),
            Self::List => "LIST".to_owned(),
            Self::Invite => "INVITE".to_owned(),
            Self::Away => "AWAY".to_owned(),
            Self::Ping => "PING".to_owned(),
            Self::Pong => "PONG".to_owned(),
            Self::Error => "ERROR".to_owned(),
            Self::Numeric(code) => format!("{code:03}"),
            Self::Pirc(_) => "PIRC".to_owned(),
        }
    }
}

impl Command {
    /// Parses a command string (uppercase keyword or 3-digit numeric) into a `Command`.
    ///
    /// Returns `None` if the string is not a recognized command keyword and is
    /// not a valid 3-digit numeric code.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "NICK" => Some(Self::Nick),
            "JOIN" => Some(Self::Join),
            "PART" => Some(Self::Part),
            "PRIVMSG" => Some(Self::Privmsg),
            "NOTICE" => Some(Self::Notice),
            "QUIT" => Some(Self::Quit),
            "KICK" => Some(Self::Kick),
            "BAN" => Some(Self::Ban),
            "MODE" => Some(Self::Mode),
            "TOPIC" => Some(Self::Topic),
            "WHOIS" => Some(Self::Whois),
            "LIST" => Some(Self::List),
            "INVITE" => Some(Self::Invite),
            "AWAY" => Some(Self::Away),
            "PING" => Some(Self::Ping),
            "PONG" => Some(Self::Pong),
            "ERROR" => Some(Self::Error),
            _ => {
                // Try parsing as a 3-digit numeric code
                if s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()) {
                    s.parse::<u16>().ok().map(Self::Numeric)
                } else {
                    None
                }
            }
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_as_str() {
        assert_eq!(Command::Nick.as_str(), "NICK");
        assert_eq!(Command::Join.as_str(), "JOIN");
        assert_eq!(Command::Part.as_str(), "PART");
        assert_eq!(Command::Privmsg.as_str(), "PRIVMSG");
        assert_eq!(Command::Notice.as_str(), "NOTICE");
        assert_eq!(Command::Quit.as_str(), "QUIT");
        assert_eq!(Command::Kick.as_str(), "KICK");
        assert_eq!(Command::Ban.as_str(), "BAN");
        assert_eq!(Command::Mode.as_str(), "MODE");
        assert_eq!(Command::Topic.as_str(), "TOPIC");
        assert_eq!(Command::Whois.as_str(), "WHOIS");
        assert_eq!(Command::List.as_str(), "LIST");
        assert_eq!(Command::Invite.as_str(), "INVITE");
        assert_eq!(Command::Away.as_str(), "AWAY");
        assert_eq!(Command::Ping.as_str(), "PING");
        assert_eq!(Command::Pong.as_str(), "PONG");
        assert_eq!(Command::Error.as_str(), "ERROR");
        assert_eq!(Command::Pirc(PircSubcommand::Version).as_str(), "PIRC");
        assert_eq!(Command::Pirc(PircSubcommand::Cap).as_str(), "PIRC");
    }

    #[test]
    fn numeric_command_as_str_zero_padded() {
        assert_eq!(Command::Numeric(1).as_str(), "001");
        assert_eq!(Command::Numeric(2).as_str(), "002");
        assert_eq!(Command::Numeric(3).as_str(), "003");
        assert_eq!(Command::Numeric(353).as_str(), "353");
        assert_eq!(Command::Numeric(433).as_str(), "433");
    }

    #[test]
    fn command_display() {
        assert_eq!(Command::Privmsg.to_string(), "PRIVMSG");
        assert_eq!(Command::Numeric(1).to_string(), "001");
    }

    #[test]
    fn command_equality() {
        assert_eq!(Command::Nick, Command::Nick);
        assert_eq!(Command::Numeric(1), Command::Numeric(1));
        assert_ne!(Command::Nick, Command::Join);
        assert_ne!(Command::Numeric(1), Command::Numeric(2));
        assert_ne!(Command::Nick, Command::Numeric(1));
    }

    #[test]
    fn command_clone() {
        let cmd = Command::Privmsg;
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn numeric_command_clone() {
        let cmd = Command::Numeric(353);
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn pirc_subcommand_as_str() {
        assert_eq!(PircSubcommand::Version.as_str(), "VERSION");
        assert_eq!(PircSubcommand::Cap.as_str(), "CAP");
    }

    #[test]
    fn pirc_subcommand_from_keyword() {
        assert_eq!(
            PircSubcommand::from_keyword("VERSION"),
            Some(PircSubcommand::Version)
        );
        assert_eq!(
            PircSubcommand::from_keyword("CAP"),
            Some(PircSubcommand::Cap)
        );
        assert_eq!(PircSubcommand::from_keyword("UNKNOWN"), None);
    }

    #[test]
    fn pirc_subcommand_display() {
        assert_eq!(PircSubcommand::Version.to_string(), "VERSION");
        assert_eq!(PircSubcommand::Cap.to_string(), "CAP");
    }

    #[test]
    fn pirc_command_equality() {
        assert_eq!(
            Command::Pirc(PircSubcommand::Version),
            Command::Pirc(PircSubcommand::Version)
        );
        assert_ne!(
            Command::Pirc(PircSubcommand::Version),
            Command::Pirc(PircSubcommand::Cap)
        );
    }

    #[test]
    fn command_debug() {
        let debug = format!("{:?}", Command::Privmsg);
        assert_eq!(debug, "Privmsg");

        let debug = format!("{:?}", Command::Numeric(1));
        assert!(debug.contains("Numeric"));
        assert!(debug.contains("1"));
    }
}
