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

fn make_channels() -> Arc<ChannelRegistry> {
    Arc::new(ChannelRegistry::new())
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

fn join_msg(channels: &str) -> Message {
    Message::new(Command::Join, vec![channels.to_owned()])
}

fn privmsg(target: &str, text: &str) -> Message {
    Message::new(Command::Privmsg, vec![target.to_owned(), text.to_owned()])
}

fn notice(target: &str, text: &str) -> Message {
    Message::new(Command::Notice, vec![target.to_owned(), text.to_owned()])
}

/// Helper: register a user and drain the welcome burst.
fn register_user(
    nick: &str,
    username: &str,
    connection_id: u64,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    config: &ServerConfig,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Message>,
    PreRegistrationState,
) {
    let (tx, mut rx) = make_sender();
    let mut state = PreRegistrationState::new(hostname.to_owned());
    handle_message(
        &nick_msg(nick),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    handle_message(
        &user_msg(username, &format!("{nick} Test")),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- CTCP ACTION to channel ----

#[tokio::test]
async fn ctcp_action_to_channel_preserves_delimiters() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &_tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Alice sends ACTION (/me waves) to #general.
    let action_text = "\x01ACTION waves\x01";
    handle_message(
        &privmsg("#general", action_text),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Bob should receive the message with \x01 delimiters preserved.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "\x01ACTION waves\x01");
}

// ---- CTCP ACTION to user ----

#[tokio::test]
async fn ctcp_action_to_user_preserves_delimiters() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice sends ACTION to Bob directly.
    let action_text = "\x01ACTION waves at Bob\x01";
    handle_message(
        &privmsg("Bob", action_text),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Bob should receive the message with \x01 delimiters preserved.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "\x01ACTION waves at Bob\x01");

    // Alice should NOT receive anything back.
    assert!(rx1.try_recv().is_err());
}

// ---- CTCP VERSION request ----

#[tokio::test]
async fn ctcp_version_request_passes_through() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, _rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice sends CTCP VERSION request to Bob.
    let version_req = "\x01VERSION\x01";
    handle_message(
        &privmsg("Bob", version_req),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "\x01VERSION\x01");
}

// ---- CTCP PING request ----

#[tokio::test]
async fn ctcp_ping_request_passes_through() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, _rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice sends CTCP PING request to Bob.
    let ping_req = "\x01PING 1234567890\x01";
    handle_message(
        &privmsg("Bob", ping_req),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "\x01PING 1234567890\x01");
}

// ---- CTCP reply via NOTICE ----

#[tokio::test]
async fn ctcp_version_reply_via_notice_passes_through() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, _rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice sends CTCP VERSION reply to Bob via NOTICE (standard CTCP reply mechanism).
    let version_reply = "\x01VERSION pirc:1.0:Rust\x01";
    handle_message(
        &notice("Bob", version_reply),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "\x01VERSION pirc:1.0:Rust\x01");
}

#[tokio::test]
async fn ctcp_ping_reply_via_notice_passes_through() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, _rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Bob replies to Alice's CTCP PING via NOTICE.
    let ping_reply = "\x01PING 1234567890\x01";
    handle_message(
        &notice("Bob", ping_reply),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "\x01PING 1234567890\x01");
}

// ---- Verify \x01 bytes are preserved byte-for-byte ----

#[tokio::test]
async fn ctcp_preserves_soh_bytes_exactly() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, _rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (_tx2, mut rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    let ctcp_msg = "\x01ACTION does something complex with spaces and symbols!@#$%\x01";
    handle_message(
        &privmsg("Bob", ctcp_msg),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx2.recv().await.unwrap();
    let text = &reply.params[1];

    // Verify the first byte is SOH (\x01).
    assert_eq!(text.as_bytes()[0], 0x01, "first byte should be SOH (\\x01)");
    // Verify the last byte is SOH (\x01).
    assert_eq!(
        text.as_bytes()[text.len() - 1],
        0x01,
        "last byte should be SOH (\\x01)"
    );
    // Verify the full content matches exactly.
    assert_eq!(text, ctcp_msg);
}

// ---- CTCP ACTION in channel with multiple recipients ----

#[tokio::test]
async fn ctcp_action_channel_broadcast_to_all_members() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);
    let (tx3, mut rx3, mut state3) = register_user(
        "Charlie",
        "charlie",
        3,
        "127.0.0.3",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    handle_message(
        &join_msg("#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    handle_message(
        &join_msg("#test"),
        3,
        &registry,
        &channels,
        &tx3,
        &mut state3,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}
    while rx3.try_recv().is_ok() {}

    // Alice sends ACTION to #test.
    let action_text = "\x01ACTION dances\x01";
    handle_message(
        &privmsg("#test", action_text),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Alice should NOT receive her own action.
    assert!(rx1.try_recv().is_err());

    // Bob should receive the action with \x01 intact.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "#test");
    assert_eq!(reply.params[1], "\x01ACTION dances\x01");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Charlie should also receive the action with \x01 intact.
    let reply = rx3.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "#test");
    assert_eq!(reply.params[1], "\x01ACTION dances\x01");
}
