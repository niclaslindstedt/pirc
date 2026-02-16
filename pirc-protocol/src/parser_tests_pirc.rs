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
    let msg = parse(":nick!user@host PIRC KEYEXCHANGE bob :base64pubkey\r\n").unwrap();
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
    let msg = parse("PIRC CLUSTER WELCOME server-42 :cluster-config-json\r\n").unwrap();
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
    let msg = parse(":cluster.node1.example.com PIRC CLUSTER HEARTBEAT server-1\r\n").unwrap();
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
        vec!["server-42".to_owned(), "cluster-config-json".to_owned()],
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
    let msg = parse(":nick!user@host PIRC P2P OFFER bob :sdp-offer-data\r\n").unwrap();
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

// ---- PIRC extension: parse -> serialize -> parse round-trips ----

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
    assert_parse_roundtrip(":alice!alice@example.com PIRC ENCRYPTED bob :encrypted data\r\n");
}

#[test]
fn roundtrip_parse_pirc_cluster_with_server_prefix() {
    assert_parse_roundtrip(":cluster.node1 PIRC CLUSTER HEARTBEAT server-1\r\n");
}

#[test]
fn roundtrip_parse_pirc_p2p_with_user_prefix() {
    assert_parse_roundtrip(":alice!alice@host PIRC P2P OFFER bob :sdp data\r\n");
}

// ---- PIRC extension: GROUP ----

#[test]
fn parse_pirc_group_create() {
    let msg = parse("PIRC GROUP CREATE 42 my-group\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupCreate)
    );
    assert_eq!(msg.params, vec!["42", "my-group"]);
}

#[test]
fn parse_pirc_group_invite() {
    let msg = parse("PIRC GROUP INVITE 42 bob\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupInvite)
    );
    assert_eq!(msg.params, vec!["42", "bob"]);
}

#[test]
fn parse_pirc_group_join() {
    let msg = parse("PIRC GROUP JOIN 42\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupJoin)
    );
    assert_eq!(msg.params, vec!["42"]);
}

#[test]
fn parse_pirc_group_leave() {
    let msg = parse("PIRC GROUP LEAVE 42\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupLeave)
    );
    assert_eq!(msg.params, vec!["42"]);
}

#[test]
fn parse_pirc_group_msg() {
    let msg = parse("PIRC GROUP MSG 42 :encrypted payload here\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupMessage)
    );
    assert_eq!(msg.params, vec!["42", "encrypted payload here"]);
}

#[test]
fn parse_pirc_group_members() {
    let msg = parse("PIRC GROUP MEMBERS 42 alice bob charlie\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupMembers)
    );
    assert_eq!(msg.params, vec!["42", "alice", "bob", "charlie"]);
}

#[test]
fn parse_pirc_group_keyex() {
    let msg = parse("PIRC GROUP KEYEX 42 bob :key exchange data\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupKeyExchange)
    );
    assert_eq!(msg.params, vec!["42", "bob", "key exchange data"]);
}

#[test]
fn parse_pirc_group_p2p_offer() {
    let msg = parse("PIRC GROUP P2P-OFFER 42 bob :sdp offer data\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupP2pOffer)
    );
    assert_eq!(msg.params, vec!["42", "bob", "sdp offer data"]);
}

#[test]
fn parse_pirc_group_p2p_answer() {
    let msg = parse("PIRC GROUP P2P-ANSWER 42 bob :sdp answer data\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupP2pAnswer)
    );
    assert_eq!(msg.params, vec!["42", "bob", "sdp answer data"]);
}

#[test]
fn parse_pirc_group_p2p_ice() {
    let msg = parse("PIRC GROUP P2P-ICE 42 bob :candidate data\r\n").unwrap();
    assert_eq!(
        msg.command,
        Command::Pirc(PircSubcommand::GroupP2pIce)
    );
    assert_eq!(msg.params, vec!["42", "bob", "candidate data"]);
}

#[test]
fn parse_pirc_group_unknown_subcommand() {
    let result = parse("PIRC GROUP UNKNOWN 42\r\n");
    assert!(result.is_err());
}

#[test]
fn parse_pirc_group_missing_subcommand() {
    let result = parse("PIRC GROUP\r\n");
    assert!(result.is_err());
}

// ---- PIRC GROUP: round-trips ----

#[test]
fn roundtrip_parse_pirc_group_create() {
    assert_parse_roundtrip("PIRC GROUP CREATE 42 my-group\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_invite() {
    assert_parse_roundtrip("PIRC GROUP INVITE 42 bob\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_join() {
    assert_parse_roundtrip("PIRC GROUP JOIN 42\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_leave() {
    assert_parse_roundtrip("PIRC GROUP LEAVE 42\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_msg() {
    assert_parse_roundtrip("PIRC GROUP MSG 42 :encrypted payload\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_members() {
    assert_parse_roundtrip("PIRC GROUP MEMBERS 42 alice bob\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_keyex() {
    assert_parse_roundtrip("PIRC GROUP KEYEX 42 bob :key data\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_p2p_offer() {
    assert_parse_roundtrip("PIRC GROUP P2P-OFFER 42 bob :sdp data\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_p2p_answer() {
    assert_parse_roundtrip("PIRC GROUP P2P-ANSWER 42 bob :sdp data\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_p2p_ice() {
    assert_parse_roundtrip("PIRC GROUP P2P-ICE 42 bob :candidate data\r\n");
}

#[test]
fn roundtrip_parse_pirc_group_with_prefix() {
    assert_parse_roundtrip(":alice!alice@host PIRC GROUP CREATE 42 my-group\r\n");
}
