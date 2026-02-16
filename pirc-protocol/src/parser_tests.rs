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
