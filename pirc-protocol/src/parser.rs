use pirc_common::Nickname;

use crate::command::Command;
use crate::error::ProtocolError;
use crate::message::{Message, MAX_PARAMS};
use crate::prefix::Prefix;

/// Maximum message length in bytes (IRC standard).
pub const MAX_MESSAGE_LEN: usize = 512;

/// Parse a raw protocol line into a [`Message`].
///
/// The input may or may not include the trailing `\r\n` delimiter — it is
/// stripped if present. The wire format is:
///
/// ```text
/// :<prefix> <command> <params...> :<trailing>\r\n
/// ```
///
/// # Errors
///
/// Returns a [`ProtocolError`] if the input is empty, too long, contains an
/// unknown command, has a malformed prefix, or exceeds the parameter limit.
pub fn parse(input: &str) -> Result<Message, ProtocolError> {
    // Strip trailing \r\n or \n
    let line = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(input);

    // Reject empty / whitespace-only
    if line.trim().is_empty() {
        return Err(ProtocolError::EmptyMessage);
    }

    // Check length (against the original input including any CRLF)
    if input.len() > MAX_MESSAGE_LEN {
        return Err(ProtocolError::MessageTooLong {
            length: input.len(),
            max: MAX_MESSAGE_LEN,
        });
    }

    let mut rest = line;

    // Parse optional prefix (starts with ':')
    let prefix = if rest.starts_with(':') {
        // Skip the leading ':'
        rest = &rest[1..];
        let end = rest.find(' ').ok_or(ProtocolError::MissingCommand)?;
        let prefix_str = &rest[..end];
        rest = &rest[end + 1..];
        Some(parse_prefix(prefix_str)?)
    } else {
        None
    };

    // Skip any extra spaces between prefix and command
    rest = rest.trim_start();

    if rest.is_empty() {
        return Err(ProtocolError::MissingCommand);
    }

    // Extract command token
    let (cmd_str, remainder) = match rest.find(' ') {
        Some(pos) => (&rest[..pos], &rest[pos + 1..]),
        None => (rest, ""),
    };

    let command = Command::from_keyword(cmd_str)
        .ok_or_else(|| ProtocolError::UnknownCommand(cmd_str.to_owned()))?;

    // Parse parameters
    let params = parse_params(remainder)?;

    Ok(match prefix {
        Some(p) => Message::with_prefix(p, command, params),
        None => Message::new(command, params),
    })
}

/// Parse the prefix string (without the leading `:`).
///
/// If it contains `!` and `@`, it's a user prefix (`nick!user@host`).
/// Otherwise it's a server prefix.
fn parse_prefix(s: &str) -> Result<Prefix, ProtocolError> {
    if s.is_empty() {
        return Err(ProtocolError::InvalidPrefix("empty prefix".to_owned()));
    }

    // Check for user prefix pattern: nick!user@host
    if let Some(bang_pos) = s.find('!') {
        let nick_str = &s[..bang_pos];
        let after_bang = &s[bang_pos + 1..];

        let at_pos = after_bang.find('@').ok_or_else(|| {
            ProtocolError::InvalidPrefix(format!("missing '@' in user prefix: {s}"))
        })?;

        let user = &after_bang[..at_pos];
        let host = &after_bang[at_pos + 1..];

        if user.is_empty() {
            return Err(ProtocolError::InvalidPrefix(format!(
                "empty user in prefix: {s}"
            )));
        }
        if host.is_empty() {
            return Err(ProtocolError::InvalidPrefix(format!(
                "empty host in prefix: {s}"
            )));
        }

        let nick =
            Nickname::new(nick_str).map_err(|e| ProtocolError::InvalidNickname(e.to_string()))?;

        Ok(Prefix::User {
            nick,
            user: user.to_owned(),
            host: host.to_owned(),
        })
    } else {
        // Server prefix — just a hostname/server name
        Ok(Prefix::Server(s.to_owned()))
    }
}

/// Parse the parameter portion of a message into a `Vec<String>`.
///
/// The trailing parameter (prefixed with `:`) consumes the rest of the line
/// and may contain spaces. Normal parameters are space-separated.
fn parse_params(input: &str) -> Result<Vec<String>, ProtocolError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut params = Vec::new();
    let mut rest = input;

    while !rest.is_empty() {
        if let Some(trailing) = rest.strip_prefix(':') {
            // Trailing parameter — everything after the ':' is one param
            params.push(trailing.to_owned());
            break;
        }

        let (param, remainder) = match rest.find(' ') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, ""),
        };

        if !param.is_empty() {
            params.push(param.to_owned());
        }
        rest = remainder;

        if params.len() == MAX_PARAMS - 1 && !rest.is_empty() {
            // The 15th (last) param consumes the rest, even without ':'
            params.push(rest.to_owned());
            break;
        }
    }

    if params.len() > MAX_PARAMS {
        return Err(ProtocolError::TooManyParams {
            count: params.len(),
            max: MAX_PARAMS,
        });
    }

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Helper ----

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    // ---- Basic parsing ----

    #[test]
    fn parse_simple_command_no_params() {
        let msg = parse("QUIT\r\n").unwrap();
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Quit);
        assert!(msg.params.is_empty());
    }

    #[test]
    fn parse_command_without_crlf() {
        let msg = parse("QUIT").unwrap();
        assert_eq!(msg.command, Command::Quit);
    }

    #[test]
    fn parse_command_with_lf_only() {
        let msg = parse("QUIT\n").unwrap();
        assert_eq!(msg.command, Command::Quit);
    }

    // ---- Prefix parsing ----

    #[test]
    fn parse_server_prefix() {
        let msg = parse(":irc.example.com NOTICE * :Server restarting\r\n").unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::Server("irc.example.com".to_owned()))
        );
        assert_eq!(msg.command, Command::Notice);
        assert_eq!(msg.params, vec!["*", "Server restarting"]);
    }

    #[test]
    fn parse_user_prefix() {
        let msg = parse(":nick!user@host PRIVMSG #channel :Hello, world!\r\n").unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::User {
                nick: nick("nick"),
                user: "user".to_owned(),
                host: "host".to_owned(),
            })
        );
        assert_eq!(msg.command, Command::Privmsg);
        assert_eq!(msg.params, vec!["#channel", "Hello, world!"]);
    }

    // ---- All command types ----

    #[test]
    fn parse_nick() {
        let msg = parse("NICK newnick\r\n").unwrap();
        assert_eq!(msg.command, Command::Nick);
        assert_eq!(msg.params, vec!["newnick"]);
    }

    #[test]
    fn parse_join() {
        let msg = parse(":nick!user@host JOIN #channel\r\n").unwrap();
        assert_eq!(msg.command, Command::Join);
        assert_eq!(msg.params, vec!["#channel"]);
    }

    #[test]
    fn parse_part_with_message() {
        let msg = parse(":nick!user@host PART #channel :Goodbye\r\n").unwrap();
        assert_eq!(msg.command, Command::Part);
        assert_eq!(msg.params, vec!["#channel", "Goodbye"]);
    }

    #[test]
    fn parse_privmsg() {
        let msg = parse(":nick!user@host PRIVMSG #channel :Hello, world!\r\n").unwrap();
        assert_eq!(msg.command, Command::Privmsg);
        assert_eq!(msg.params, vec!["#channel", "Hello, world!"]);
    }

    #[test]
    fn parse_notice() {
        let msg = parse(":server NOTICE * :Welcome\r\n").unwrap();
        assert_eq!(msg.command, Command::Notice);
        assert_eq!(msg.params, vec!["*", "Welcome"]);
    }

    #[test]
    fn parse_quit_with_message() {
        let msg = parse(":nick!user@host QUIT :Leaving\r\n").unwrap();
        assert_eq!(msg.command, Command::Quit);
        assert_eq!(msg.params, vec!["Leaving"]);
    }

    #[test]
    fn parse_kick() {
        let msg = parse(":nick!user@host KICK #channel target :Reason\r\n").unwrap();
        assert_eq!(msg.command, Command::Kick);
        assert_eq!(msg.params, vec!["#channel", "target", "Reason"]);
    }

    #[test]
    fn parse_ban() {
        let msg = parse(":nick!user@host BAN #channel target\r\n").unwrap();
        assert_eq!(msg.command, Command::Ban);
        assert_eq!(msg.params, vec!["#channel", "target"]);
    }

    #[test]
    fn parse_mode() {
        let msg = parse("MODE #channel +o nick\r\n").unwrap();
        assert_eq!(msg.command, Command::Mode);
        assert_eq!(msg.params, vec!["#channel", "+o", "nick"]);
    }

    #[test]
    fn parse_topic() {
        let msg = parse(":nick!user@host TOPIC #channel :New topic\r\n").unwrap();
        assert_eq!(msg.command, Command::Topic);
        assert_eq!(msg.params, vec!["#channel", "New topic"]);
    }

    #[test]
    fn parse_whois() {
        let msg = parse("WHOIS nick\r\n").unwrap();
        assert_eq!(msg.command, Command::Whois);
        assert_eq!(msg.params, vec!["nick"]);
    }

    #[test]
    fn parse_list() {
        let msg = parse("LIST\r\n").unwrap();
        assert_eq!(msg.command, Command::List);
        assert!(msg.params.is_empty());
    }

    #[test]
    fn parse_invite() {
        let msg = parse(":nick!user@host INVITE target #channel\r\n").unwrap();
        assert_eq!(msg.command, Command::Invite);
        assert_eq!(msg.params, vec!["target", "#channel"]);
    }

    #[test]
    fn parse_away() {
        let msg = parse("AWAY :Gone fishing\r\n").unwrap();
        assert_eq!(msg.command, Command::Away);
        assert_eq!(msg.params, vec!["Gone fishing"]);
    }

    #[test]
    fn parse_ping() {
        let msg = parse("PING :server1\r\n").unwrap();
        assert_eq!(msg.command, Command::Ping);
        assert_eq!(msg.params, vec!["server1"]);
    }

    #[test]
    fn parse_pong() {
        let msg = parse("PONG server1\r\n").unwrap();
        assert_eq!(msg.command, Command::Pong);
        assert_eq!(msg.params, vec!["server1"]);
    }

    #[test]
    fn parse_error() {
        let msg = parse("ERROR :Closing link\r\n").unwrap();
        assert_eq!(msg.command, Command::Error);
        assert_eq!(msg.params, vec!["Closing link"]);
    }

    // ---- Numeric replies ----

    #[test]
    fn parse_numeric_reply() {
        let msg = parse(":server 001 nick :Welcome to the pirc network\r\n").unwrap();
        assert_eq!(msg.command, Command::Numeric(1));
        assert_eq!(msg.params, vec!["nick", "Welcome to the pirc network"]);
    }

    #[test]
    fn parse_numeric_353() {
        let msg = parse(":server 353 nick = #channel :nick1 nick2 nick3\r\n").unwrap();
        assert_eq!(msg.command, Command::Numeric(353));
        assert_eq!(
            msg.params,
            vec!["nick", "=", "#channel", "nick1 nick2 nick3"]
        );
    }

    #[test]
    fn parse_numeric_433() {
        let msg = parse(":server 433 * nick :Nickname is already in use\r\n").unwrap();
        assert_eq!(msg.command, Command::Numeric(433));
        assert_eq!(msg.params, vec!["*", "nick", "Nickname is already in use"]);
    }

    // ---- Edge cases ----

    #[test]
    fn parse_no_prefix_no_trailing() {
        let msg = parse("MODE #channel +o nick\r\n").unwrap();
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Mode);
        assert_eq!(msg.params, vec!["#channel", "+o", "nick"]);
    }

    #[test]
    fn parse_trailing_with_spaces() {
        let msg = parse("PRIVMSG #test :Hello world with spaces\r\n").unwrap();
        assert_eq!(msg.params, vec!["#test", "Hello world with spaces"]);
    }

    #[test]
    fn parse_trailing_empty() {
        let msg = parse("TOPIC #test :\r\n").unwrap();
        assert_eq!(msg.command, Command::Topic);
        assert_eq!(msg.params, vec!["#test", ""]);
    }

    #[test]
    fn parse_trailing_starts_with_colon() {
        let msg = parse("PRIVMSG #test ::)\r\n").unwrap();
        assert_eq!(msg.params, vec!["#test", ":)"]);
    }

    #[test]
    fn parse_no_params() {
        let msg = parse("LIST\r\n").unwrap();
        assert!(msg.params.is_empty());
    }

    // ---- Error cases ----

    #[test]
    fn parse_empty_string() {
        let err = parse("").unwrap_err();
        assert_eq!(err, ProtocolError::EmptyMessage);
    }

    #[test]
    fn parse_whitespace_only() {
        let err = parse("   \r\n").unwrap_err();
        assert_eq!(err, ProtocolError::EmptyMessage);
    }

    #[test]
    fn parse_just_crlf() {
        let err = parse("\r\n").unwrap_err();
        assert_eq!(err, ProtocolError::EmptyMessage);
    }

    #[test]
    fn parse_unknown_command() {
        let err = parse("FOOBAR arg\r\n").unwrap_err();
        assert_eq!(err, ProtocolError::UnknownCommand("FOOBAR".to_owned()));
    }

    #[test]
    fn parse_prefix_only_no_command() {
        let err = parse(":server\r\n").unwrap_err();
        assert_eq!(err, ProtocolError::MissingCommand);
    }

    #[test]
    fn parse_invalid_prefix_missing_at() {
        let err = parse(":nick!user PRIVMSG #test :hi\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
    }

    #[test]
    fn parse_invalid_prefix_empty_user() {
        let err = parse(":nick!@host PRIVMSG #test :hi\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
    }

    #[test]
    fn parse_invalid_prefix_empty_host() {
        let err = parse(":nick!user@ PRIVMSG #test :hi\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
    }

    #[test]
    fn parse_message_too_long() {
        let long = format!("PRIVMSG #test :{}\r\n", "x".repeat(500));
        assert!(long.len() > MAX_MESSAGE_LEN);
        let err = parse(&long).unwrap_err();
        assert!(matches!(err, ProtocolError::MessageTooLong { .. }));
    }

    #[test]
    fn parse_invalid_nickname_in_prefix() {
        // '1nick' starts with a digit, invalid per Nickname rules
        let err = parse(":1nick!user@host PRIVMSG #test :hi\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidNickname(_)));
    }

    // ---- Round-trip: Display -> parse ----

    #[test]
    fn roundtrip_simple() {
        let original = Message::new(Command::Quit, vec![]);
        let wire = format!("{original}\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_with_prefix_and_trailing() {
        let original = Message::with_prefix(
            Prefix::User {
                nick: nick("alice"),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            },
            Command::Privmsg,
            vec!["#general".to_owned(), "Hello, world!".to_owned()],
        );
        let wire = format!("{original}\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_numeric() {
        let original = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Numeric(1),
            vec![
                "alice".to_owned(),
                "Welcome to the pirc network!".to_owned(),
            ],
        );
        let wire = format!("{original}\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_ping() {
        let original = Message::new(Command::Ping, vec!["server1".to_owned()]);
        let wire = format!("{original}\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    // ---- Error kind helper ----

    #[test]
    fn error_kind_strings() {
        assert_eq!(ProtocolError::EmptyMessage.kind(), "empty_message");
        assert_eq!(
            ProtocolError::MessageTooLong {
                length: 600,
                max: 512,
            }
            .kind(),
            "message_too_long"
        );
        assert_eq!(ProtocolError::MissingCommand.kind(), "missing_command");
        assert_eq!(
            ProtocolError::UnknownCommand("X".to_owned()).kind(),
            "unknown_command"
        );
    }

    // ---- Error Display ----

    #[test]
    fn error_display() {
        let err = ProtocolError::UnknownCommand("FOOBAR".to_owned());
        assert_eq!(err.to_string(), "unknown command: FOOBAR");
    }
}
