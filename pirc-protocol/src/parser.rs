use pirc_common::Nickname;

use crate::command::{Command, PircSubcommand};
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

    // Handle PIRC extension commands specially: the subcommand keyword
    // is the first token after PIRC (e.g., "PIRC VERSION 1.0").
    if cmd_str == "PIRC" {
        return parse_pirc_command(prefix, remainder);
    }

    let command = Command::from_keyword(cmd_str)
        .ok_or_else(|| ProtocolError::UnknownCommand(cmd_str.to_owned()))?;

    // Parse parameters
    let params = parse_params(remainder)?;

    Ok(match prefix {
        Some(p) => Message::with_prefix(p, command, params),
        None => Message::new(command, params),
    })
}

/// Parse a `PIRC` extension command.
///
/// The subcommand keyword (e.g., `VERSION`, `CAP`) is extracted from the first
/// token of `remainder`. For namespaced subcommands (`CLUSTER`, `P2P`), the
/// next token is also consumed as the inner keyword. Everything after the
/// subcommand keyword(s) is parsed as normal parameters of the resulting message.
fn parse_pirc_command(prefix: Option<Prefix>, remainder: &str) -> Result<Message, ProtocolError> {
    let remainder = remainder.trim_start();

    if remainder.is_empty() {
        return Err(ProtocolError::UnknownCommand(
            "PIRC (missing subcommand)".to_owned(),
        ));
    }

    let (sub_str, after_sub) = match remainder.find(' ') {
        Some(pos) => (&remainder[..pos], &remainder[pos + 1..]),
        None => (remainder, ""),
    };

    // Check for namespaced subcommands (CLUSTER, P2P) first
    if sub_str == "CLUSTER" || sub_str == "P2P" {
        let after_sub = after_sub.trim_start();
        if after_sub.is_empty() {
            return Err(ProtocolError::UnknownCommand(format!(
                "PIRC {sub_str} (missing subcommand)"
            )));
        }

        let (inner_str, params_str) = match after_sub.find(' ') {
            Some(pos) => (&after_sub[..pos], &after_sub[pos + 1..]),
            None => (after_sub, ""),
        };

        let subcommand =
            PircSubcommand::from_namespace(sub_str, inner_str).ok_or_else(|| {
                ProtocolError::UnknownCommand(format!("PIRC {sub_str} {inner_str}"))
            })?;

        let command = Command::Pirc(subcommand);
        let params = parse_params(params_str)?;

        return Ok(match prefix {
            Some(p) => Message::with_prefix(p, command, params),
            None => Message::new(command, params),
        });
    }

    // Flat subcommand (VERSION, CAP, encryption keywords)
    let subcommand = PircSubcommand::from_keyword(sub_str)
        .ok_or_else(|| ProtocolError::UnknownCommand(format!("PIRC {sub_str}")))?;

    let command = Command::Pirc(subcommand);
    let params = parse_params(after_sub)?;

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

    fn assert_parse_roundtrip(input: &str) {
        let parsed = parse(input).unwrap();
        let serialized = format!("{parsed}\r\n");
        let reparsed = parse(&serialized).unwrap();
        assert_eq!(parsed, reparsed, "round-trip failed for: {input}");
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

    // ---- PIRC extension commands ----

    #[test]
    fn parse_pirc_version() {
        let msg = parse("PIRC VERSION 1.0\r\n").unwrap();
        assert!(msg.prefix.is_none());
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Version));
        assert_eq!(msg.params, vec!["1.0"]);
    }

    #[test]
    fn parse_pirc_version_with_prefix() {
        let msg = parse(":irc.example.com PIRC VERSION 1.0\r\n").unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::Server("irc.example.com".to_owned()))
        );
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Version));
        assert_eq!(msg.params, vec!["1.0"]);
    }

    #[test]
    fn parse_pirc_version_higher() {
        let msg = parse("PIRC VERSION 2.3\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Version));
        assert_eq!(msg.params, vec!["2.3"]);
    }

    #[test]
    fn parse_pirc_cap() {
        let msg = parse("PIRC CAP encryption\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Cap));
        assert_eq!(msg.params, vec!["encryption"]);
    }

    #[test]
    fn parse_pirc_cap_multiple() {
        let msg = parse("PIRC CAP encryption clustering p2p\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Cap));
        assert_eq!(msg.params, vec!["encryption", "clustering", "p2p"]);
    }

    #[test]
    fn parse_pirc_cap_no_params() {
        let msg = parse("PIRC CAP\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Cap));
        assert!(msg.params.is_empty());
    }

    #[test]
    fn parse_pirc_unknown_subcommand() {
        let err = parse("PIRC FOOBAR arg\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    #[test]
    fn parse_pirc_missing_subcommand() {
        let err = parse("PIRC\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    #[test]
    fn parse_pirc_missing_subcommand_with_spaces() {
        let err = parse("PIRC   \r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    // ---- PIRC round-trips ----

    #[test]
    fn roundtrip_pirc_version() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::Version),
            vec!["1.0".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC VERSION 1.0\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_version_with_prefix() {
        let original = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Pirc(PircSubcommand::Version),
            vec!["1.0".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, ":irc.example.com PIRC VERSION 1.0\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cap() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::Cap),
            vec!["encryption".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CAP encryption\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cap_multiple() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::Cap),
            vec![
                "encryption".to_owned(),
                "clustering".to_owned(),
                "p2p".to_owned(),
            ],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CAP encryption clustering p2p\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cap_no_params() {
        let original = Message::new(Command::Pirc(PircSubcommand::Cap), vec![]);
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CAP\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    // ---- PIRC encryption extension commands ----

    #[test]
    fn parse_pirc_keyexchange() {
        let msg = parse("PIRC KEYEXCHANGE alice :base64pubkeydata\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
        assert_eq!(msg.params, vec!["alice", "base64pubkeydata"]);
    }

    #[test]
    fn parse_pirc_keyexchange_ack() {
        let msg = parse("PIRC KEYEXCHANGE-ACK alice :base64pubkeydata\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchangeAck));
        assert_eq!(msg.params, vec!["alice", "base64pubkeydata"]);
    }

    #[test]
    fn parse_pirc_keyexchange_complete() {
        let msg = parse("PIRC KEYEXCHANGE-COMPLETE alice\r\n").unwrap();
        assert_eq!(
            msg.command,
            Command::Pirc(PircSubcommand::KeyExchangeComplete)
        );
        assert_eq!(msg.params, vec!["alice"]);
    }

    #[test]
    fn parse_pirc_fingerprint() {
        let msg = parse("PIRC FINGERPRINT alice :ABCD1234EF567890\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Fingerprint));
        assert_eq!(msg.params, vec!["alice", "ABCD1234EF567890"]);
    }

    #[test]
    fn parse_pirc_encrypted() {
        let msg = parse("PIRC ENCRYPTED alice :encryptedpayloaddata==\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::Encrypted));
        assert_eq!(msg.params, vec!["alice", "encryptedpayloaddata=="]);
    }

    #[test]
    fn parse_pirc_keyexchange_with_prefix() {
        let msg =
            parse(":nick!user@host PIRC KEYEXCHANGE bob :base64pubkey\r\n").unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::User {
                nick: nick("nick"),
                user: "user".to_owned(),
                host: "host".to_owned(),
            })
        );
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
        assert_eq!(msg.params, vec!["bob", "base64pubkey"]);
    }

    // ---- PIRC encryption round-trips ----

    #[test]
    fn roundtrip_pirc_keyexchange() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::KeyExchange),
            vec!["alice".to_owned(), "base64pubkeydata".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC KEYEXCHANGE alice base64pubkeydata\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_keyexchange_ack() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::KeyExchangeAck),
            vec!["alice".to_owned(), "base64pubkeydata".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC KEYEXCHANGE-ACK alice base64pubkeydata\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_keyexchange_complete() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::KeyExchangeComplete),
            vec!["alice".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC KEYEXCHANGE-COMPLETE alice\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_fingerprint() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::Fingerprint),
            vec!["alice".to_owned(), "ABCD1234EF567890".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC FINGERPRINT alice ABCD1234EF567890\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_encrypted() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::Encrypted),
            vec!["alice".to_owned(), "encryptedpayloaddata==".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC ENCRYPTED alice encryptedpayloaddata==\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    // ---- PIRC cluster extension commands ----

    #[test]
    fn parse_pirc_cluster_join() {
        let msg = parse("PIRC CLUSTER JOIN :invite-key-abc123\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterJoin));
        assert_eq!(msg.params, vec!["invite-key-abc123"]);
    }

    #[test]
    fn parse_pirc_cluster_welcome() {
        let msg =
            parse("PIRC CLUSTER WELCOME server-42 :cluster-config-json\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterWelcome));
        assert_eq!(msg.params, vec!["server-42", "cluster-config-json"]);
    }

    #[test]
    fn parse_pirc_cluster_sync() {
        let msg = parse("PIRC CLUSTER SYNC :state-data-blob\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterSync));
        assert_eq!(msg.params, vec!["state-data-blob"]);
    }

    #[test]
    fn parse_pirc_cluster_heartbeat() {
        let msg = parse("PIRC CLUSTER HEARTBEAT server-42\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterHeartbeat));
        assert_eq!(msg.params, vec!["server-42"]);
    }

    #[test]
    fn parse_pirc_cluster_migrate() {
        let msg = parse("PIRC CLUSTER MIGRATE user-123 server-99\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterMigrate));
        assert_eq!(msg.params, vec!["user-123", "server-99"]);
    }

    #[test]
    fn parse_pirc_cluster_raft() {
        let msg = parse("PIRC CLUSTER RAFT :raft-message-payload\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterRaft));
        assert_eq!(msg.params, vec!["raft-message-payload"]);
    }

    #[test]
    fn parse_pirc_cluster_with_prefix() {
        let msg =
            parse(":cluster.node1.example.com PIRC CLUSTER HEARTBEAT server-1\r\n")
                .unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::Server("cluster.node1.example.com".to_owned()))
        );
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterHeartbeat));
        assert_eq!(msg.params, vec!["server-1"]);
    }

    #[test]
    fn parse_pirc_cluster_missing_inner_subcommand() {
        let err = parse("PIRC CLUSTER\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    #[test]
    fn parse_pirc_cluster_unknown_inner_subcommand() {
        let err = parse("PIRC CLUSTER FOOBAR arg\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    // ---- PIRC cluster round-trips ----

    #[test]
    fn roundtrip_pirc_cluster_join() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterJoin),
            vec!["invite-key-abc123".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CLUSTER JOIN invite-key-abc123\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cluster_welcome() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterWelcome),
            vec![
                "server-42".to_owned(),
                "cluster-config-json".to_owned(),
            ],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(
            wire,
            "PIRC CLUSTER WELCOME server-42 cluster-config-json\r\n"
        );
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cluster_sync() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterSync),
            vec!["state-data-blob".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CLUSTER SYNC state-data-blob\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cluster_heartbeat() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterHeartbeat),
            vec!["server-42".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CLUSTER HEARTBEAT server-42\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cluster_migrate() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterMigrate),
            vec!["user-123".to_owned(), "server-99".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CLUSTER MIGRATE user-123 server-99\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_cluster_raft() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::ClusterRaft),
            vec!["raft-message-payload".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC CLUSTER RAFT raft-message-payload\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    // ---- PIRC P2P extension commands ----

    #[test]
    fn parse_pirc_p2p_offer() {
        let msg = parse("PIRC P2P OFFER bob :sdp-offer-data\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pOffer));
        assert_eq!(msg.params, vec!["bob", "sdp-offer-data"]);
    }

    #[test]
    fn parse_pirc_p2p_answer() {
        let msg = parse("PIRC P2P ANSWER bob :sdp-answer-data\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pAnswer));
        assert_eq!(msg.params, vec!["bob", "sdp-answer-data"]);
    }

    #[test]
    fn parse_pirc_p2p_ice() {
        let msg = parse("PIRC P2P ICE bob :candidate-data\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pIce));
        assert_eq!(msg.params, vec!["bob", "candidate-data"]);
    }

    #[test]
    fn parse_pirc_p2p_established() {
        let msg = parse("PIRC P2P ESTABLISHED bob\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pEstablished));
        assert_eq!(msg.params, vec!["bob"]);
    }

    #[test]
    fn parse_pirc_p2p_failed() {
        let msg = parse("PIRC P2P FAILED bob :connection timed out\r\n").unwrap();
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pFailed));
        assert_eq!(msg.params, vec!["bob", "connection timed out"]);
    }

    #[test]
    fn parse_pirc_p2p_with_prefix() {
        let msg =
            parse(":nick!user@host PIRC P2P OFFER bob :sdp-offer-data\r\n").unwrap();
        assert_eq!(
            msg.prefix,
            Some(Prefix::User {
                nick: nick("nick"),
                user: "user".to_owned(),
                host: "host".to_owned(),
            })
        );
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::P2pOffer));
        assert_eq!(msg.params, vec!["bob", "sdp-offer-data"]);
    }

    #[test]
    fn parse_pirc_p2p_missing_inner_subcommand() {
        let err = parse("PIRC P2P\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    #[test]
    fn parse_pirc_p2p_unknown_inner_subcommand() {
        let err = parse("PIRC P2P FOOBAR arg\r\n").unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownCommand(_)));
    }

    // ---- PIRC P2P round-trips ----

    #[test]
    fn roundtrip_pirc_p2p_offer() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::P2pOffer),
            vec!["bob".to_owned(), "sdp-offer-data".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC P2P OFFER bob sdp-offer-data\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_p2p_answer() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::P2pAnswer),
            vec!["bob".to_owned(), "sdp-answer-data".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC P2P ANSWER bob sdp-answer-data\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_p2p_ice() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::P2pIce),
            vec!["bob".to_owned(), "candidate-data".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC P2P ICE bob candidate-data\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_p2p_established() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::P2pEstablished),
            vec!["bob".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC P2P ESTABLISHED bob\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_pirc_p2p_failed() {
        let original = Message::new(
            Command::Pirc(PircSubcommand::P2pFailed),
            vec!["bob".to_owned(), "connection timed out".to_owned()],
        );
        let wire = format!("{original}\r\n");
        assert_eq!(wire, "PIRC P2P FAILED bob :connection timed out\r\n");
        let parsed = parse(&wire).unwrap();
        assert_eq!(parsed, original);
    }

    // ---- PIRC extension: parse → serialize → parse round-trips ----

    #[test]
    fn roundtrip_parse_pirc_keyexchange() {
        assert_parse_roundtrip("PIRC KEYEXCHANGE alice base64pubkey\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_keyexchange_ack() {
        assert_parse_roundtrip("PIRC KEYEXCHANGE-ACK alice base64pubkey\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_keyexchange_complete() {
        assert_parse_roundtrip("PIRC KEYEXCHANGE-COMPLETE alice\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_fingerprint() {
        assert_parse_roundtrip("PIRC FINGERPRINT alice ABCD1234\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_encrypted() {
        assert_parse_roundtrip("PIRC ENCRYPTED alice :encrypted payload data\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_join() {
        assert_parse_roundtrip("PIRC CLUSTER JOIN invite-key\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_welcome() {
        assert_parse_roundtrip("PIRC CLUSTER WELCOME server-1 :config data\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_sync() {
        assert_parse_roundtrip("PIRC CLUSTER SYNC :state data blob\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_heartbeat() {
        assert_parse_roundtrip("PIRC CLUSTER HEARTBEAT server-1\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_migrate() {
        assert_parse_roundtrip("PIRC CLUSTER MIGRATE user-1 server-2\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_raft() {
        assert_parse_roundtrip("PIRC CLUSTER RAFT :raft payload\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_offer() {
        assert_parse_roundtrip("PIRC P2P OFFER bob :sdp offer data\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_answer() {
        assert_parse_roundtrip("PIRC P2P ANSWER bob :sdp answer data\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_ice() {
        assert_parse_roundtrip("PIRC P2P ICE bob :candidate data\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_established() {
        assert_parse_roundtrip("PIRC P2P ESTABLISHED bob\r\n");
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_failed() {
        assert_parse_roundtrip("PIRC P2P FAILED bob :connection timed out\r\n");
    }

    // ---- PIRC extension: with prefixes ----

    #[test]
    fn roundtrip_parse_pirc_encrypted_with_user_prefix() {
        assert_parse_roundtrip(
            ":alice!alice@example.com PIRC ENCRYPTED bob :encrypted data\r\n",
        );
    }

    #[test]
    fn roundtrip_parse_pirc_cluster_with_server_prefix() {
        assert_parse_roundtrip(
            ":cluster.node1 PIRC CLUSTER HEARTBEAT server-1\r\n",
        );
    }

    #[test]
    fn roundtrip_parse_pirc_p2p_with_user_prefix() {
        assert_parse_roundtrip(
            ":alice!alice@host PIRC P2P OFFER bob :sdp data\r\n",
        );
    }
}
