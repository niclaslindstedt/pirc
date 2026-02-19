//! Protocol codec round-trip integration tests.
//!
//! Validates that pirc-protocol messages survive full encode/decode round-trips
//! through pirc-network's framed codec over TCP.

use std::net::SocketAddr;

use pirc_network::connection::AsyncTransport;
use pirc_network::Connection;
use pirc_protocol::parser::MAX_MESSAGE_LEN;
use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
use tokio::net::TcpListener;

/// Create a pair of connected [`Connection`] endpoints via TCP loopback.
async fn connection_pair() -> (Connection, Connection) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let (client_result, server_result) =
        tokio::join!(tokio::net::TcpStream::connect(addr), listener.accept());
    let client = Connection::new(client_result.unwrap()).unwrap();
    let server = Connection::new(server_result.unwrap().0).unwrap();
    (client, server)
}

/// Send a message from `tx` and receive it on `rx`, returning the received message.
async fn roundtrip(tx: &mut Connection, rx: &mut Connection, msg: Message) -> Message {
    tx.send(msg).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for message")
        .expect("recv error")
        .expect("unexpected EOF")
}

// ---------------------------------------------------------------------------
// Message round-trip via TCP — standard commands
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_nick() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Nick, vec!["TestUser".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_user() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::User,
        vec!["uname".into(), "0".into(), "*".into(), "Real Name".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_join() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Join, vec!["#channel".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_part() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Part, vec!["#channel".into(), "leaving".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_privmsg() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Privmsg,
        vec!["#channel".into(), "hello world".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_notice() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Notice,
        vec!["target".into(), "notice text".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_quit() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Quit, vec!["goodbye".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_kick() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Kick,
        vec!["#channel".into(), "baduser".into(), "behave".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_ban() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Ban, vec!["#channel".into(), "baduser".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_mode() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Mode, vec!["#channel".into(), "+o".into(), "nick".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_topic() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Topic,
        vec!["#channel".into(), "New topic with spaces".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_whois() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Whois, vec!["nick".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_list() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::List, vec![]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_names() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Names, vec!["#channel".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_invite() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Invite, vec!["nick".into(), "#channel".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_away() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Away, vec!["Gone fishing".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_oper() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Oper, vec!["admin".into(), "secret".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_kill() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Kill, vec!["baduser".into(), "reason text".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_die() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Die, vec![]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_restart() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Restart, vec![]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_wallops() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Wallops, vec!["broadcast message".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_motd() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Motd, vec![]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_ping() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Ping, vec!["token123".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_pong() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Pong, vec!["token123".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_error() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Error, vec!["Closing link".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

// ---------------------------------------------------------------------------
// Numeric replies round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_numeric_welcome() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Numeric(1),
        vec!["nick".into(), "Welcome to the server".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Numeric(1));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_numeric_error() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Numeric(433),
        vec!["*".into(), "nick".into(), "Nickname is already in use".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Numeric(433));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_numeric_353_namreply() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Numeric(353),
        vec![
            "nick".into(),
            "=".into(),
            "#channel".into(),
            "alice bob charlie".into(),
        ],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Numeric(353));
    assert_eq!(got.params, sent.params);
}

// ---------------------------------------------------------------------------
// Messages with prefixes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_server_prefix() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::with_prefix(
        Prefix::server("irc.example.com"),
        Command::Numeric(1),
        vec!["nick".into(), "Welcome".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.prefix, Some(Prefix::server("irc.example.com")));
    assert_eq!(got.command, Command::Numeric(1));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_user_prefix() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::with_prefix(
        Prefix::user("alice", "alice", "example.com"),
        Command::Privmsg,
        vec!["#channel".into(), "hello everyone".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(
        got.prefix,
        Some(Prefix::user("alice", "alice", "example.com"))
    );
    assert_eq!(got.command, Command::Privmsg);
    assert_eq!(got.params, sent.params);
}

// ---------------------------------------------------------------------------
// MAX_MESSAGE_LEN boundary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_message_at_max_length() {
    let (mut tx, mut rx) = connection_pair().await;
    // Build a PRIVMSG that uses up to 510 bytes of content (+ 2 for \r\n = 512).
    // Wire format: "PRIVMSG #ch <trailing>\r\n"
    // When trailing has no spaces, Display writes "PRIVMSG #ch <trailing>" (no colon).
    // "PRIVMSG #ch " = 12 bytes. So trailing_len = 510 - 12 = 498.
    let prefix_len = "PRIVMSG #ch ".len(); // 12
    let trailing_len = MAX_MESSAGE_LEN - 2 - prefix_len;
    let trailing: String = "A".repeat(trailing_len);
    let sent = Message::new(Command::Privmsg, vec!["#ch".into(), trailing.clone()]);
    // Verify our message is exactly at the limit
    assert_eq!(sent.to_string().len() + 2, MAX_MESSAGE_LEN);

    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Privmsg);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_message_just_under_max_length() {
    let (mut tx, mut rx) = connection_pair().await;
    let prefix_len = "PRIVMSG #ch ".len(); // 12
    let trailing_len = MAX_MESSAGE_LEN - 2 - prefix_len - 1; // one byte under
    let trailing: String = "B".repeat(trailing_len);
    let sent = Message::new(Command::Privmsg, vec!["#ch".into(), trailing]);
    assert!(sent.to_string().len() + 2 < MAX_MESSAGE_LEN);

    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, sent.command);
    assert_eq!(got.params, sent.params);
}

// ---------------------------------------------------------------------------
// Special characters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_trailing_with_spaces() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Privmsg,
        vec!["#ch".into(), "hello world with many spaces".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params[1], "hello world with many spaces");
}

#[tokio::test]
async fn roundtrip_trailing_with_colons() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Privmsg,
        vec!["#ch".into(), ":leading colon and mid:colon".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params[1], ":leading colon and mid:colon");
}

#[tokio::test]
async fn roundtrip_ascii_special_chars() {
    let (mut tx, mut rx) = connection_pair().await;
    let text = "!@#$%^&*()_+-=[]{}|;'\"<>?,./~`";
    let sent = Message::new(Command::Privmsg, vec!["#ch".into(), text.into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params[1], text);
}

#[tokio::test]
async fn roundtrip_utf8_characters() {
    let (mut tx, mut rx) = connection_pair().await;
    // Note: UTF-8 multi-byte chars consume more bytes — keep total under 512.
    let text = "cafe\u{0301} naïve Stra\u{00df}e";
    let sent = Message::new(Command::Privmsg, vec!["#ch".into(), text.into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params[1], text);
}

#[tokio::test]
async fn roundtrip_empty_trailing() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Privmsg, vec!["#ch".into(), String::new()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Privmsg);
    assert_eq!(got.params[1], "");
}

// ---------------------------------------------------------------------------
// Protocol edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_max_params() {
    let (mut tx, mut rx) = connection_pair().await;
    // Build a message with 15 params (the maximum). Use Numeric(353) since standard
    // commands have validation constraints.
    // Note: with 15 params, the parser's "15th param consumes rest" rule applies
    // before the trailing-`:` strip, so the 15th param must not need a `:` prefix
    // (i.e., no spaces, not empty, doesn't start with ':') for a clean round-trip.
    let params: Vec<String> = (0..15).map(|i| format!("p{i}")).collect();
    let sent = Message::new(Command::Numeric(353), params);
    assert_eq!(sent.params.len(), 15);

    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params.len(), 15);
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_single_param_no_trailing() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::Nick, vec!["simple".into()]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.params, vec!["simple"]);
}

#[tokio::test]
async fn roundtrip_no_params() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(Command::List, vec![]);
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::List);
    assert!(got.params.is_empty());
}

#[tokio::test]
async fn roundtrip_prefix_with_all_components() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::with_prefix(
        Prefix::user("nick", "user", "host.example.com"),
        Command::Privmsg,
        vec!["#ch".into(), "hello".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    let prefix = got.prefix.unwrap();
    assert_eq!(prefix, Prefix::user("nick", "user", "host.example.com"));
}

// ---------------------------------------------------------------------------
// PIRC extension command round-trips
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_pirc_version() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Pirc(PircSubcommand::Version),
        vec!["1.0".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Pirc(PircSubcommand::Version));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_pirc_encrypted() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec!["target".into(), "base64payload==".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Pirc(PircSubcommand::Encrypted));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_pirc_cluster_join() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Pirc(PircSubcommand::ClusterJoin),
        vec!["invite-key-abc".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Pirc(PircSubcommand::ClusterJoin));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_pirc_group_message() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Pirc(PircSubcommand::GroupMessage),
        vec!["group-id-123".into(), "encrypted payload data".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Pirc(PircSubcommand::GroupMessage));
    assert_eq!(got.params, sent.params);
}

#[tokio::test]
async fn roundtrip_pirc_p2p_offer() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::new(
        Command::Pirc(PircSubcommand::P2pOffer),
        vec!["target".into(), "sdp-offer-data".into()],
    );
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.command, Command::Pirc(PircSubcommand::P2pOffer));
    assert_eq!(got.params, sent.params);
}

// ---------------------------------------------------------------------------
// Multi-message streams — burst ordering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn burst_100_messages_arrive_in_order() {
    let (mut tx, mut rx) = connection_pair().await;

    for i in 0..100 {
        let msg = Message::new(Command::Privmsg, vec!["#ch".into(), format!("msg-{i}")]);
        tx.send(msg).await.unwrap();
    }

    for i in 0..100 {
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error")
            .expect("unexpected EOF");
        assert_eq!(got.command, Command::Privmsg);
        assert_eq!(got.params[1], format!("msg-{i}"));
    }
}

// ---------------------------------------------------------------------------
// Multi-message streams — interleaved connections
// ---------------------------------------------------------------------------

#[tokio::test]
async fn interleaved_sends_no_corruption() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Two client connections to the same listener
    let (stream_a, accept_a) = tokio::join!(
        tokio::net::TcpStream::connect(addr),
        listener.accept()
    );
    let mut client_a = Connection::new(stream_a.unwrap()).unwrap();
    let mut server_a = Connection::new(accept_a.unwrap().0).unwrap();

    let (stream_b, accept_b) = tokio::join!(
        tokio::net::TcpStream::connect(addr),
        listener.accept()
    );
    let mut client_b = Connection::new(stream_b.unwrap()).unwrap();
    let mut server_b = Connection::new(accept_b.unwrap().0).unwrap();

    // Interleave sends from both connections
    for i in 0..50 {
        let msg_a = Message::new(Command::Privmsg, vec!["#a".into(), format!("a-{i}")]);
        let msg_b = Message::new(Command::Privmsg, vec!["#b".into(), format!("b-{i}")]);
        client_a.send(msg_a).await.unwrap();
        client_b.send(msg_b).await.unwrap();
    }

    // Verify each stream received its own messages in order, uncorrupted
    for i in 0..50 {
        let got_a = tokio::time::timeout(std::time::Duration::from_secs(5), server_a.recv())
            .await
            .expect("timeout a")
            .expect("recv error a")
            .expect("unexpected EOF a");
        assert_eq!(got_a.params[0], "#a");
        assert_eq!(got_a.params[1], format!("a-{i}"));

        let got_b = tokio::time::timeout(std::time::Duration::from_secs(5), server_b.recv())
            .await
            .expect("timeout b")
            .expect("recv error b")
            .expect("unexpected EOF b");
        assert_eq!(got_b.params[0], "#b");
        assert_eq!(got_b.params[1], format!("b-{i}"));
    }
}

// ---------------------------------------------------------------------------
// Multi-message streams — varying sizes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn varying_message_sizes_in_same_stream() {
    let (mut tx, mut rx) = connection_pair().await;

    let sizes = [0, 1, 10, 50, 100, 200, 400];
    for &size in &sizes {
        let trailing: String = "X".repeat(size);
        let msg = Message::new(Command::Privmsg, vec!["#ch".into(), trailing]);
        tx.send(msg).await.unwrap();
    }

    for &size in &sizes {
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error")
            .expect("unexpected EOF");
        assert_eq!(got.command, Command::Privmsg);
        assert_eq!(got.params[1].len(), size);
    }
}

// ---------------------------------------------------------------------------
// Builder API round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_via_builder() {
    let (mut tx, mut rx) = connection_pair().await;
    let sent = Message::builder(Command::Privmsg)
        .prefix(Prefix::server("irc.example.com"))
        .param("#general")
        .trailing("Hello, world!")
        .build();
    let got = roundtrip(&mut tx, &mut rx, sent.clone()).await;
    assert_eq!(got.prefix, Some(Prefix::server("irc.example.com")));
    assert_eq!(got.command, Command::Privmsg);
    assert_eq!(got.params, vec!["#general", "Hello, world!"]);
}
