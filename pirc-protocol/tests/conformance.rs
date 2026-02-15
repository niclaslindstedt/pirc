//! Protocol conformance integration tests.
//!
//! These tests verify end-to-end protocol behavior through the public API,
//! covering edge cases, boundary conditions, and conformance requirements
//! that go beyond individual unit tests.

use pirc_protocol::parser::MAX_MESSAGE_LEN;
use pirc_protocol::{parse, Command, Message, PircSubcommand, Prefix, ProtocolError};

// ============================================================================
// Helpers
// ============================================================================

/// Parse input, serialize, re-parse, and assert equality.
fn assert_roundtrip(input: &str) {
    let parsed = parse(input).expect("initial parse failed");
    let wire = format!("{parsed}\r\n");
    let reparsed = parse(&wire).expect("re-parse failed");
    assert_eq!(parsed, reparsed, "round-trip mismatch for input: {input:?}");
}

/// Build a message, serialize, parse, and assert equality.
fn assert_build_roundtrip(msg: Message) {
    let wire = format!("{msg}\r\n");
    let parsed = parse(&wire).expect("parse of built message failed");
    assert_eq!(msg, parsed, "build round-trip mismatch for wire: {wire:?}");
}

// ============================================================================
// 1. Message length limits
// ============================================================================

#[test]
fn length_exactly_at_512_byte_limit() {
    // Build a message that is exactly 512 bytes including \r\n
    // "PRIVMSG #t :" = 12 bytes, "\r\n" = 2 bytes => need 498 bytes of payload
    let payload = "x".repeat(498);
    let input = format!("PRIVMSG #t :{payload}\r\n");
    assert_eq!(input.len(), 512);
    let msg = parse(&input).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params.len(), 2);
    assert_eq!(msg.params[1].len(), 498);
}

#[test]
fn length_one_byte_over_limit() {
    let payload = "x".repeat(499);
    let input = format!("PRIVMSG #t :{payload}\r\n");
    assert_eq!(input.len(), 513);
    let err = parse(&input).unwrap_err();
    assert_eq!(
        err,
        ProtocolError::MessageTooLong {
            length: 513,
            max: MAX_MESSAGE_LEN,
        }
    );
}

#[test]
fn length_well_over_limit() {
    let input = format!("PRIVMSG #test :{}\r\n", "a".repeat(1000));
    let err = parse(&input).unwrap_err();
    match err {
        ProtocolError::MessageTooLong { length, max } => {
            assert!(length > MAX_MESSAGE_LEN);
            assert_eq!(max, MAX_MESSAGE_LEN);
        }
        other => panic!("expected MessageTooLong, got: {other:?}"),
    }
}

#[test]
fn length_empty_input() {
    assert_eq!(parse("").unwrap_err(), ProtocolError::EmptyMessage);
}

#[test]
fn length_only_crlf() {
    assert_eq!(parse("\r\n").unwrap_err(), ProtocolError::EmptyMessage);
}

#[test]
fn length_only_whitespace() {
    assert_eq!(parse("   ").unwrap_err(), ProtocolError::EmptyMessage);
}

#[test]
fn length_whitespace_with_crlf() {
    assert_eq!(parse("   \r\n").unwrap_err(), ProtocolError::EmptyMessage);
}

#[test]
fn length_only_lf() {
    assert_eq!(parse("\n").unwrap_err(), ProtocolError::EmptyMessage);
}

// ============================================================================
// 2. Prefix parsing edge cases
// ============================================================================

#[test]
fn prefix_full_user_components() {
    let msg = parse(":alice!alice@irc.example.com PRIVMSG #general :hi\r\n").unwrap();
    match msg.prefix {
        Some(Prefix::User {
            ref nick,
            ref user,
            ref host,
        }) => {
            assert_eq!(nick.to_string(), "alice");
            assert_eq!(user, "alice");
            assert_eq!(host, "irc.example.com");
        }
        other => panic!("expected User prefix, got: {other:?}"),
    }
}

#[test]
fn prefix_server_only() {
    let msg = parse(":irc.example.com NOTICE * :hello\r\n").unwrap();
    assert_eq!(
        msg.prefix,
        Some(Prefix::Server("irc.example.com".to_owned()))
    );
}

#[test]
fn prefix_server_simple_name() {
    // A server name without dots is still valid as a server prefix
    let msg = parse(":localhost NOTICE * :hello\r\n").unwrap();
    assert_eq!(msg.prefix, Some(Prefix::Server("localhost".to_owned())));
}

#[test]
fn prefix_missing_at_in_user_prefix() {
    // nick!user without @host should error
    let err = parse(":nick!user PRIVMSG #test :hi\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
}

#[test]
fn prefix_empty_user_component() {
    let err = parse(":nick!@host PRIVMSG #test :hi\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
}

#[test]
fn prefix_empty_host_component() {
    let err = parse(":nick!user@ PRIVMSG #test :hi\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidPrefix(_)));
}

#[test]
fn prefix_invalid_nickname_starts_with_digit() {
    let err = parse(":123nick!user@host PRIVMSG #test :hi\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidNickname(_)));
}

#[test]
fn prefix_with_hyphen_in_host() {
    let msg = parse(":alice!alice@some-host.example.com QUIT :bye\r\n").unwrap();
    match msg.prefix {
        Some(Prefix::User { ref host, .. }) => {
            assert_eq!(host, "some-host.example.com");
        }
        other => panic!("expected User prefix, got: {other:?}"),
    }
}

#[test]
fn prefix_with_numeric_host() {
    let msg = parse(":alice!alice@127.0.0.1 QUIT :bye\r\n").unwrap();
    match msg.prefix {
        Some(Prefix::User { ref host, .. }) => {
            assert_eq!(host, "127.0.0.1");
        }
        other => panic!("expected User prefix, got: {other:?}"),
    }
}

#[test]
fn prefix_only_without_command() {
    let err = parse(":server\r\n").unwrap_err();
    assert_eq!(err, ProtocolError::MissingCommand);
}

// ============================================================================
// 3. Parameter edge cases
// ============================================================================

#[test]
fn params_exactly_fifteen() {
    // Build a message with exactly 15 parameters
    let params: Vec<String> = (1..=15).map(|i| format!("p{i}")).collect();
    let param_str = params[..14].join(" ");
    // 15th parameter as trailing
    let input = format!("MODE {param_str} :{}\r\n", params[14]);
    let msg = parse(&input).unwrap();
    assert_eq!(msg.params.len(), 15);
}

#[test]
fn params_overflow_into_fifteenth() {
    // When you have 14 normal params and more text, the 15th consumes everything
    let parts: Vec<String> = (1..=14).map(|i| format!("p{i}")).collect();
    let param_str = parts.join(" ");
    let input = format!("MODE {param_str} overflow1 overflow2\r\n");
    let msg = parse(&input).unwrap();
    assert_eq!(msg.params.len(), 15);
    // The 15th param should contain "overflow1 overflow2"
    assert_eq!(msg.params[14], "overflow1 overflow2");
}

#[test]
fn params_empty_trailing() {
    let msg = parse("TOPIC #channel :\r\n").unwrap();
    assert_eq!(msg.params, vec!["#channel", ""]);
}

#[test]
fn params_trailing_with_only_spaces() {
    let msg = parse("PRIVMSG #channel :   \r\n").unwrap();
    assert_eq!(msg.params, vec!["#channel", "   "]);
}

#[test]
fn params_trailing_starts_with_colon() {
    let msg = parse("PRIVMSG #test ::)\r\n").unwrap();
    assert_eq!(msg.params, vec!["#test", ":)"]);
}

#[test]
fn params_trailing_with_multiple_colons() {
    let msg = parse("PRIVMSG #test :10:30:00 time\r\n").unwrap();
    assert_eq!(msg.params, vec!["#test", "10:30:00 time"]);
}

#[test]
fn params_no_params_at_all() {
    let msg = parse("QUIT\r\n").unwrap();
    assert!(msg.params.is_empty());
}

#[test]
fn params_single_non_trailing() {
    let msg = parse("NICK alice\r\n").unwrap();
    assert_eq!(msg.params, vec!["alice"]);
}

#[test]
fn params_multiple_spaces_between() {
    // Multiple spaces between prefix and command should be handled
    let msg = parse(":server   NOTICE * :hi\r\n").unwrap();
    assert_eq!(msg.command, Command::Notice);
}

// ============================================================================
// 4. Command case handling
// ============================================================================

#[test]
fn command_case_uppercase_accepted() {
    let msg = parse("PRIVMSG #test :hello\r\n").unwrap();
    assert_eq!(msg.command, Command::Privmsg);
}

#[test]
fn command_case_lowercase_rejected() {
    // IRC protocol requires uppercase commands; lowercase should fail
    let err = parse("privmsg #test :hello\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::UnknownCommand(_)));
}

#[test]
fn command_case_mixed_rejected() {
    let err = parse("Privmsg #test :hello\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::UnknownCommand(_)));
}

#[test]
fn command_unknown_string() {
    let err = parse("XYZZY arg\r\n").unwrap_err();
    assert_eq!(err, ProtocolError::UnknownCommand("XYZZY".to_owned()));
}

#[test]
fn command_numeric_three_digit() {
    let msg = parse(":server 001 nick :Welcome\r\n").unwrap();
    assert_eq!(msg.command, Command::Numeric(1));
}

#[test]
fn command_numeric_000() {
    // 000 is technically parseable as a numeric
    let msg = parse(":server 000 nick :test\r\n").unwrap();
    assert_eq!(msg.command, Command::Numeric(0));
}

#[test]
fn command_numeric_999() {
    let msg = parse(":server 999 nick :test\r\n").unwrap();
    assert_eq!(msg.command, Command::Numeric(999));
}

#[test]
fn command_not_numeric_two_digit() {
    // "01" is not a valid 3-digit numeric
    let err = parse(":server 01 nick :test\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::UnknownCommand(_)));
}

#[test]
fn command_not_numeric_four_digit() {
    let err = parse(":server 0001 nick :test\r\n").unwrap_err();
    assert!(matches!(err, ProtocolError::UnknownCommand(_)));
}

// ============================================================================
// 5. Protocol error types
// ============================================================================

#[test]
fn error_empty_message_kind() {
    assert_eq!(ProtocolError::EmptyMessage.kind(), "empty_message");
}

#[test]
fn error_message_too_long_kind() {
    let err = ProtocolError::MessageTooLong {
        length: 600,
        max: 512,
    };
    assert_eq!(err.kind(), "message_too_long");
}

#[test]
fn error_missing_command_kind() {
    assert_eq!(ProtocolError::MissingCommand.kind(), "missing_command");
}

#[test]
fn error_unknown_command_kind() {
    let err = ProtocolError::UnknownCommand("FOO".to_owned());
    assert_eq!(err.kind(), "unknown_command");
}

#[test]
fn error_invalid_prefix_kind() {
    let err = ProtocolError::InvalidPrefix("bad".to_owned());
    assert_eq!(err.kind(), "invalid_prefix");
}

#[test]
fn error_invalid_nickname_kind() {
    let err = ProtocolError::InvalidNickname("bad".to_owned());
    assert_eq!(err.kind(), "invalid_nickname");
}

#[test]
fn error_too_many_params_kind() {
    let err = ProtocolError::TooManyParams { count: 20, max: 15 };
    assert_eq!(err.kind(), "too_many_params");
}

#[test]
fn error_missing_parameter_kind() {
    let err = ProtocolError::MissingParameter {
        command: "NICK".to_owned(),
        expected: "nickname",
    };
    assert_eq!(err.kind(), "missing_parameter");
}

#[test]
fn error_missing_parameter_display() {
    let err = ProtocolError::MissingParameter {
        command: "PRIVMSG".to_owned(),
        expected: "target and message",
    };
    assert_eq!(
        err.to_string(),
        "missing parameter for PRIVMSG: expected target and message"
    );
}

#[test]
fn error_invalid_version_kind() {
    let err = ProtocolError::InvalidVersion("bad".to_owned());
    assert_eq!(err.kind(), "invalid_version");
}

#[test]
fn error_message_too_long_display() {
    let err = ProtocolError::MessageTooLong {
        length: 600,
        max: 512,
    };
    assert_eq!(err.to_string(), "message too long (600 bytes, max 512)");
}

#[test]
fn error_too_many_params_display() {
    let err = ProtocolError::TooManyParams { count: 20, max: 15 };
    assert_eq!(err.to_string(), "too many parameters (20, max 15)");
}

// ============================================================================
// 6. Message validation
// ============================================================================

#[test]
fn validate_nick_without_param() {
    let msg = Message::new(Command::Nick, vec![]);
    let err = msg.validate().unwrap_err();
    assert!(matches!(err, ProtocolError::MissingParameter { .. }));
}

#[test]
fn validate_nick_with_param() {
    let msg = Message::new(Command::Nick, vec!["alice".to_owned()]);
    assert!(msg.validate().is_ok());
}

#[test]
fn validate_privmsg_missing_message() {
    let msg = Message::new(Command::Privmsg, vec!["#channel".to_owned()]);
    let err = msg.validate().unwrap_err();
    match err {
        ProtocolError::MissingParameter { command, expected } => {
            assert_eq!(command, "PRIVMSG");
            assert_eq!(expected, "target and message");
        }
        other => panic!("expected MissingParameter, got: {other:?}"),
    }
}

#[test]
fn validate_privmsg_valid() {
    let msg = Message::new(
        Command::Privmsg,
        vec!["#channel".to_owned(), "hello".to_owned()],
    );
    assert!(msg.validate().is_ok());
}

#[test]
fn validate_join_missing_channel() {
    let msg = Message::new(Command::Join, vec![]);
    assert!(msg.validate().is_err());
}

#[test]
fn validate_kick_missing_target() {
    let msg = Message::new(Command::Kick, vec!["#channel".to_owned()]);
    assert!(msg.validate().is_err());
}

#[test]
fn validate_quit_no_params_ok() {
    // QUIT can have zero params (no quit message)
    let msg = Message::new(Command::Quit, vec![]);
    assert!(msg.validate().is_ok());
}

#[test]
fn validate_list_no_params_ok() {
    let msg = Message::new(Command::List, vec![]);
    assert!(msg.validate().is_ok());
}

#[test]
fn validate_too_many_params() {
    let params: Vec<String> = (0..20).map(|i| format!("p{i}")).collect();
    let msg = Message::new(Command::Mode, params);
    let err = msg.validate().unwrap_err();
    assert!(matches!(err, ProtocolError::TooManyParams { .. }));
}

#[test]
fn validate_ping_missing_server() {
    let msg = Message::new(Command::Ping, vec![]);
    assert!(msg.validate().is_err());
}

#[test]
fn validate_invite_missing_channel() {
    let msg = Message::new(Command::Invite, vec!["nick".to_owned()]);
    assert!(msg.validate().is_err());
}

// ============================================================================
// 7. Round-trip conformance: all standard commands
// ============================================================================

#[test]
fn roundtrip_nick() {
    assert_roundtrip("NICK newnick\r\n");
}

#[test]
fn roundtrip_join_with_prefix() {
    assert_roundtrip(":alice!alice@host JOIN #channel\r\n");
}

#[test]
fn roundtrip_part_with_message() {
    assert_roundtrip(":alice!alice@host PART #channel :Goodbye all\r\n");
}

#[test]
fn roundtrip_privmsg_with_spaces() {
    assert_roundtrip(":alice!alice@host PRIVMSG #general :Hello world!\r\n");
}

#[test]
fn roundtrip_notice() {
    assert_roundtrip(":server NOTICE * :Server restarting\r\n");
}

#[test]
fn roundtrip_quit_with_message() {
    assert_roundtrip(":alice!alice@host QUIT :Gone fishing\r\n");
}

#[test]
fn roundtrip_kick_with_reason() {
    assert_roundtrip(":alice!alice@host KICK #channel bob :Bad behavior\r\n");
}

#[test]
fn roundtrip_ban() {
    assert_roundtrip(":alice!alice@host BAN #channel bob\r\n");
}

#[test]
fn roundtrip_mode_channel() {
    assert_roundtrip("MODE #channel +o nick\r\n");
}

#[test]
fn roundtrip_topic_with_spaces() {
    assert_roundtrip(":alice!alice@host TOPIC #channel :Welcome to the channel!\r\n");
}

#[test]
fn roundtrip_whois() {
    assert_roundtrip("WHOIS alice\r\n");
}

#[test]
fn roundtrip_list_no_params() {
    assert_roundtrip("LIST\r\n");
}

#[test]
fn roundtrip_invite() {
    assert_roundtrip(":alice!alice@host INVITE bob #channel\r\n");
}

#[test]
fn roundtrip_away_with_message() {
    assert_roundtrip("AWAY :Gone for lunch\r\n");
}

#[test]
fn roundtrip_ping() {
    assert_roundtrip("PING server1\r\n");
}

#[test]
fn roundtrip_pong() {
    assert_roundtrip("PONG server1\r\n");
}

#[test]
fn roundtrip_error() {
    assert_roundtrip("ERROR :Closing link\r\n");
}

#[test]
fn roundtrip_numeric_001() {
    assert_roundtrip(":server 001 alice :Welcome to the network\r\n");
}

// ============================================================================
// 8. Round-trip conformance: PIRC extensions
// ============================================================================

#[test]
fn roundtrip_pirc_all_encryption_commands() {
    assert_roundtrip("PIRC KEYEXCHANGE bob base64key\r\n");
    assert_roundtrip("PIRC KEYEXCHANGE-ACK bob base64key\r\n");
    assert_roundtrip("PIRC KEYEXCHANGE-COMPLETE bob\r\n");
    assert_roundtrip("PIRC FINGERPRINT bob ABCDEF123456\r\n");
    assert_roundtrip("PIRC ENCRYPTED bob :encrypted data payload\r\n");
}

#[test]
fn roundtrip_pirc_all_cluster_commands() {
    assert_roundtrip("PIRC CLUSTER JOIN invite-key\r\n");
    assert_roundtrip("PIRC CLUSTER WELCOME server-1 config-data\r\n");
    assert_roundtrip("PIRC CLUSTER SYNC :state blob\r\n");
    assert_roundtrip("PIRC CLUSTER HEARTBEAT server-1\r\n");
    assert_roundtrip("PIRC CLUSTER MIGRATE user-1 server-2\r\n");
    assert_roundtrip("PIRC CLUSTER RAFT :raft data\r\n");
}

#[test]
fn roundtrip_pirc_all_p2p_commands() {
    assert_roundtrip("PIRC P2P OFFER bob :sdp offer\r\n");
    assert_roundtrip("PIRC P2P ANSWER bob :sdp answer\r\n");
    assert_roundtrip("PIRC P2P ICE bob :candidate data\r\n");
    assert_roundtrip("PIRC P2P ESTABLISHED bob\r\n");
    assert_roundtrip("PIRC P2P FAILED bob :timeout\r\n");
}

// ============================================================================
// 9. Builder conformance
// ============================================================================

#[test]
fn builder_roundtrip_privmsg() {
    let msg = Message::builder(Command::Privmsg)
        .prefix(Prefix::user("alice", "alice", "example.com"))
        .param("#general")
        .trailing("Hello, world!")
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn builder_roundtrip_numeric_with_trailing() {
    let msg = Message::builder(Command::Numeric(1))
        .prefix(Prefix::server("irc.example.com"))
        .param("alice")
        .trailing("Welcome to the pirc network!")
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn builder_roundtrip_pirc_version() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::Version))
        .param("1.0")
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn builder_roundtrip_pirc_cluster_join() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::ClusterJoin))
        .prefix(Prefix::server("node1.cluster"))
        .param("invite-key-123")
        .build();
    assert_build_roundtrip(msg);
}

// ============================================================================
// 10. Wire format conformance
// ============================================================================

#[test]
fn wire_format_no_prefix_no_trailing() {
    let msg = Message::new(Command::Nick, vec!["alice".to_owned()]);
    assert_eq!(msg.to_string(), "NICK alice");
}

#[test]
fn wire_format_with_prefix() {
    let msg = Message::with_prefix(
        Prefix::server("irc.example.com"),
        Command::Notice,
        vec!["*".to_owned(), "hello".to_owned()],
    );
    assert_eq!(msg.to_string(), ":irc.example.com NOTICE * hello");
}

#[test]
fn wire_format_trailing_with_spaces() {
    let msg = Message::new(
        Command::Privmsg,
        vec!["#test".to_owned(), "hello world".to_owned()],
    );
    assert_eq!(msg.to_string(), "PRIVMSG #test :hello world");
}

#[test]
fn wire_format_trailing_starts_with_colon() {
    let msg = Message::new(Command::Privmsg, vec!["#test".to_owned(), ":)".to_owned()]);
    assert_eq!(msg.to_string(), "PRIVMSG #test ::)");
}

#[test]
fn wire_format_empty_trailing() {
    let msg = Message::new(Command::Topic, vec!["#test".to_owned(), String::new()]);
    assert_eq!(msg.to_string(), "TOPIC #test :");
}

#[test]
fn wire_format_numeric_zero_padded() {
    let msg = Message::with_prefix(
        Prefix::server("server"),
        Command::Numeric(1),
        vec!["nick".to_owned(), "Welcome".to_owned()],
    );
    assert_eq!(msg.to_string(), ":server 001 nick Welcome");
}

#[test]
fn wire_format_pirc_version() {
    let msg = Message::new(
        Command::Pirc(PircSubcommand::Version),
        vec!["1.0".to_owned()],
    );
    assert_eq!(msg.to_string(), "PIRC VERSION 1.0");
}

#[test]
fn wire_format_pirc_cluster_join() {
    let msg = Message::new(
        Command::Pirc(PircSubcommand::ClusterJoin),
        vec!["invite-key".to_owned()],
    );
    assert_eq!(msg.to_string(), "PIRC CLUSTER JOIN invite-key");
}

// ============================================================================
// 11. Cross-format parsing (LF vs CRLF vs bare)
// ============================================================================

#[test]
fn parse_accepts_crlf() {
    let msg = parse("QUIT\r\n").unwrap();
    assert_eq!(msg.command, Command::Quit);
}

#[test]
fn parse_accepts_lf_only() {
    let msg = parse("QUIT\n").unwrap();
    assert_eq!(msg.command, Command::Quit);
}

#[test]
fn parse_accepts_no_terminator() {
    let msg = parse("QUIT").unwrap();
    assert_eq!(msg.command, Command::Quit);
}

#[test]
fn parse_crlf_lf_bare_produce_same_result() {
    let from_crlf = parse("PRIVMSG #test :hello\r\n").unwrap();
    let from_lf = parse("PRIVMSG #test :hello\n").unwrap();
    let from_bare = parse("PRIVMSG #test :hello").unwrap();
    assert_eq!(from_crlf, from_lf);
    assert_eq!(from_lf, from_bare);
}

// ============================================================================
// 12. Special characters in messages
// ============================================================================

#[test]
fn trailing_with_unicode() {
    let msg = parse("PRIVMSG #test :Hello, 世界!\r\n").unwrap();
    assert_eq!(msg.params[1], "Hello, 世界!");
}

#[test]
fn trailing_with_emoji() {
    let msg = parse("PRIVMSG #test :thumbs up 👍\r\n").unwrap();
    assert_eq!(msg.params[1], "thumbs up 👍");
}

#[test]
fn param_with_special_irc_chars() {
    let msg = parse("MODE #test +o-v nick1 nick2\r\n").unwrap();
    assert_eq!(msg.params, vec!["#test", "+o-v", "nick1", "nick2"]);
}

// ============================================================================
// 13. Error conversion
// ============================================================================

#[test]
fn protocol_error_converts_to_pirc_error() {
    let proto_err = ProtocolError::EmptyMessage;
    let pirc_err: pirc_common::PircError = proto_err.into();
    match pirc_err {
        pirc_common::PircError::ProtocolError { message } => {
            assert_eq!(message, "empty message");
        }
        other => panic!("expected ProtocolError variant, got: {other:?}"),
    }
}

#[test]
fn protocol_error_message_too_long_converts() {
    let proto_err = ProtocolError::MessageTooLong {
        length: 600,
        max: 512,
    };
    let pirc_err: pirc_common::PircError = proto_err.into();
    match pirc_err {
        pirc_common::PircError::ProtocolError { message } => {
            assert!(message.contains("600"));
            assert!(message.contains("512"));
        }
        other => panic!("expected ProtocolError variant, got: {other:?}"),
    }
}

// ============================================================================
// 14. Protocol error equality and cloning
// ============================================================================

#[test]
fn protocol_error_equality() {
    assert_eq!(ProtocolError::EmptyMessage, ProtocolError::EmptyMessage);
    assert_ne!(ProtocolError::EmptyMessage, ProtocolError::MissingCommand);
    assert_eq!(
        ProtocolError::UnknownCommand("FOO".to_owned()),
        ProtocolError::UnknownCommand("FOO".to_owned())
    );
    assert_ne!(
        ProtocolError::UnknownCommand("FOO".to_owned()),
        ProtocolError::UnknownCommand("BAR".to_owned())
    );
}

#[test]
fn protocol_error_clone() {
    let err = ProtocolError::MessageTooLong {
        length: 600,
        max: 512,
    };
    let cloned = err.clone();
    assert_eq!(err, cloned);
}

// ============================================================================
// 15. PIRC extension error cases
// ============================================================================

#[test]
fn pirc_missing_subcommand_error() {
    let err = parse("PIRC\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("PIRC"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

#[test]
fn pirc_unknown_subcommand_error() {
    let err = parse("PIRC BADCMD arg\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("PIRC"));
            assert!(s.contains("BADCMD"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

#[test]
fn pirc_cluster_missing_inner_error() {
    let err = parse("PIRC CLUSTER\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("CLUSTER"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

#[test]
fn pirc_cluster_unknown_inner_error() {
    let err = parse("PIRC CLUSTER BADCMD arg\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("CLUSTER"));
            assert!(s.contains("BADCMD"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

#[test]
fn pirc_p2p_missing_inner_error() {
    let err = parse("PIRC P2P\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("P2P"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

#[test]
fn pirc_p2p_unknown_inner_error() {
    let err = parse("PIRC P2P BADCMD arg\r\n").unwrap_err();
    match err {
        ProtocolError::UnknownCommand(s) => {
            assert!(s.contains("P2P"));
            assert!(s.contains("BADCMD"));
        }
        other => panic!("expected UnknownCommand, got: {other:?}"),
    }
}

// ============================================================================
// 16. Message Display idempotency
// ============================================================================

#[test]
fn display_idempotent_for_all_standard_commands() {
    let commands = vec![
        "NICK alice\r\n",
        ":alice!alice@host JOIN #channel\r\n",
        ":alice!alice@host PART #channel :Bye\r\n",
        ":alice!alice@host PRIVMSG #ch :Hello world\r\n",
        ":server NOTICE * :Welcome\r\n",
        ":alice!alice@host QUIT :Leaving\r\n",
        ":alice!alice@host KICK #ch bob :Reason here\r\n",
        ":alice!alice@host BAN #ch bob\r\n",
        "MODE #ch +o alice\r\n",
        ":alice!alice@host TOPIC #ch :New topic text\r\n",
        "WHOIS alice\r\n",
        "LIST\r\n",
        ":alice!alice@host INVITE bob #ch\r\n",
        "AWAY :Gone fishing\r\n",
        "PING server1\r\n",
        "PONG server1\r\n",
        "ERROR :Closing link\r\n",
        ":server 001 alice :Welcome to pirc\r\n",
    ];

    for input in commands {
        let msg = parse(input).unwrap();
        let wire1 = format!("{msg}\r\n");
        let msg2 = parse(&wire1).unwrap();
        let wire2 = format!("{msg2}\r\n");
        assert_eq!(wire1, wire2, "display not idempotent for: {input}");
    }
}

// ============================================================================
// 14. Key exchange wire protocol messages
// ============================================================================

#[test]
fn keyexchange_with_base64_payload_roundtrip() {
    // Simulate a KEYEXCHANGE message carrying a base64-encoded pre-key bundle
    let payload = "SGVsbG8gV29ybGQ="; // "Hello World" in base64
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
        .param("bob")
        .param(payload)
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn keyexchange_ack_with_base64_payload_roundtrip() {
    let payload = "dGVzdCBkYXRh"; // "test data" in base64
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchangeAck))
        .param("alice")
        .param(payload)
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn keyexchange_complete_roundtrip() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchangeComplete))
        .param("alice")
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn keyexchange_parse_extracts_base64_payload() {
    let input = "PIRC KEYEXCHANGE bob SGVsbG8gV29ybGQ=\r\n";
    let msg = parse(input).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
    assert_eq!(msg.params.len(), 2);
    assert_eq!(msg.params[0], "bob");
    assert_eq!(msg.params[1], "SGVsbG8gV29ybGQ=");
}

#[test]
fn keyexchange_ack_parse_extracts_base64_payload() {
    let input = "PIRC KEYEXCHANGE-ACK alice dGVzdCBkYXRh\r\n";
    let msg = parse(input).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchangeAck));
    assert_eq!(msg.params.len(), 2);
    assert_eq!(msg.params[0], "alice");
    assert_eq!(msg.params[1], "dGVzdCBkYXRh");
}

#[test]
fn keyexchange_with_prefix_roundtrip() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
        .prefix(Prefix::User {
            nick: pirc_common::Nickname::new("alice").unwrap(),
            user: "alice".into(),
            host: "example.com".into(),
        })
        .param("bob")
        .param("AQID") // base64 for [1, 2, 3]
        .build();
    assert_build_roundtrip(msg);
}

#[test]
fn keyexchange_large_base64_payload_roundtrip() {
    // Simulate a large payload (like a real pre-key bundle ~13KB base64)
    // but fit within 512 byte IRC limit by using trailing param
    let payload = "A".repeat(200); // 200 chars of valid base64
    let input = format!("PIRC KEYEXCHANGE bob :{payload}\r\n");
    assert_roundtrip(&input);
}

#[test]
fn fingerprint_with_hex_payload_roundtrip() {
    let fingerprint = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
    let msg = Message::builder(Command::Pirc(PircSubcommand::Fingerprint))
        .param("bob")
        .param(fingerprint)
        .build();
    assert_build_roundtrip(msg);
}
