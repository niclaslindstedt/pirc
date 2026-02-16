use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{RPL_ENDOFNAMES, RPL_LIST, RPL_LISTEND, RPL_NAMREPLY};

use super::*;
use crate::channel::MemberStatus;

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

fn channel_name(s: &str) -> ChannelName {
    ChannelName::new(s).unwrap()
}

fn list_msg() -> Message {
    Message::new(Command::List, vec![])
}

fn list_msg_with_filter(filter: &str) -> Message {
    Message::new(Command::List, vec![filter.to_owned()])
}

fn names_msg() -> Message {
    Message::new(Command::Names, vec![])
}

fn names_msg_with_channel(channel: &str) -> Message {
    Message::new(Command::Names, vec![channel.to_owned()])
}

// ---- LIST command tests ----

#[tokio::test]
async fn list_empty_returns_only_listend() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &list_msg(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
    assert!(reply.trailing().unwrap().contains("End of /LIST"));
}

#[tokio::test]
async fn list_shows_channels_with_members_and_topics() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );

    // Join a channel.
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {} // drain JOIN replies

    // Set a topic.
    {
        let ch_arc = channels.get(&channel_name("#general")).unwrap();
        let mut ch = ch_arc.write().unwrap();
        ch.topic = Some(("Welcome to general!".to_owned(), "Alice".to_owned(), 100));
    }

    handle_message(
        &list_msg(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LIST));
    // Params: [nick, channel, member_count]
    assert_eq!(reply.params[1], "#general");
    assert_eq!(reply.params[2], "1");
    assert_eq!(reply.trailing().unwrap(), "Welcome to general!");

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
}

#[tokio::test]
async fn list_hides_secret_channels_from_non_members() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Alice joins #secret and sets it +s.
    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#secret"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}

    {
        let ch_arc = channels.get(&channel_name("#secret")).unwrap();
        let mut ch = ch_arc.write().unwrap();
        ch.modes.insert(ChannelMode::Secret);
    }

    // Bob is not in #secret.
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);

    handle_message(
        &list_msg(),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Bob should only see RPL_LISTEND, no RPL_LIST for #secret.
    let reply = rx_b.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
}

#[tokio::test]
async fn list_shows_secret_channels_to_members() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#secret"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    {
        let ch_arc = channels.get(&channel_name("#secret")).unwrap();
        let mut ch = ch_arc.write().unwrap();
        ch.modes.insert(ChannelMode::Secret);
    }

    handle_message(
        &list_msg(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LIST));
    assert_eq!(reply.params[1], "#secret");

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
}

#[tokio::test]
async fn list_with_filter_shows_only_matching_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}
    handle_message(
        &join_msg("#random"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    // LIST with filter for #general only.
    handle_message(
        &list_msg_with_filter("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LIST));
    assert_eq!(reply.params[1], "#general");

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
}

#[tokio::test]
async fn list_channel_with_no_topic_has_empty_trailing() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#notopic"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &list_msg(),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LIST));
    assert_eq!(reply.trailing().unwrap(), "");

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_LISTEND));
}

// ---- NAMES command tests ----

#[tokio::test]
async fn names_specific_channel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &names_msg_with_channel("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_NAMREPLY));
    // Alice is operator (first joiner), so prefixed with @.
    assert!(reply.trailing().unwrap().contains("@Alice"));

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_ENDOFNAMES));
}

#[tokio::test]
async fn names_nonexistent_channel_sends_endofnames() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &names_msg_with_channel("#doesnotexist"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_ENDOFNAMES));
}

#[tokio::test]
async fn names_no_args_lists_user_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#mychannel"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}

    // Create another channel that Alice is NOT in.
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);
    handle_message(
        &join_msg("#bobchannel"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}

    // NAMES with no args for Alice.
    handle_message(
        &names_msg(),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Alice should get NAMREPLY + ENDOFNAMES for #mychannel only.
    let reply = rx_a.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_NAMREPLY));
    assert!(reply.params.iter().any(|p| p == "#mychannel"));

    let reply = rx_a.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_ENDOFNAMES));

    // Should NOT have more replies (no #bobchannel).
    assert!(rx_a.try_recv().is_err());
}

#[tokio::test]
async fn names_shows_prefixes_for_operators_and_voiced() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);
    let (tx_c, mut rx_c, mut state_c) = register_user(
        "Carol",
        "carol",
        3,
        "localhost",
        &registry,
        &channels,
        &config,
    );

    // Alice joins first (operator), Bob and Carol join after.
    handle_message(
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#test"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {} // drain Bob's JOIN from Alice
    handle_message(
        &join_msg("#test"),
        3,
        &registry,
        &channels,
        &tx_c,
        &mut state_c,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_c.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {} // drain Carol's JOIN from Alice

    // Set Bob as voiced.
    {
        let ch_arc = channels.get(&channel_name("#test")).unwrap();
        let mut ch = ch_arc.write().unwrap();
        ch.members
            .insert(Nickname::new("Bob").unwrap(), MemberStatus::Voiced);
    }

    // Ask for NAMES.
    handle_message(
        &names_msg_with_channel("#test"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    let reply = rx_a.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_NAMREPLY));
    let names = reply.trailing().unwrap();
    assert!(
        names.contains("@Alice"),
        "Alice should have @ prefix, got: {names}"
    );
    assert!(
        names.contains("+Bob"),
        "Bob should have + prefix, got: {names}"
    );
    assert!(
        names.contains("Carol"),
        "Carol should be in the list, got: {names}"
    );
    // Carol should NOT have a prefix.
    assert!(
        !names.contains("@Carol"),
        "Carol should NOT have @ prefix, got: {names}"
    );
    assert!(
        !names.contains("+Carol"),
        "Carol should NOT have + prefix, got: {names}"
    );
}

// ---- QUIT channel cleanup tests ----

#[tokio::test]
async fn quit_removes_user_from_all_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}
    handle_message(
        &join_msg("#random"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    // QUIT.
    let quit = Message::new(Command::Quit, vec!["Goodbye".to_owned()]);
    let result = handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    assert!(matches!(result, HandleResult::Quit));

    // Both channels should be gone (they were empty after Alice left).
    assert!(channels.get(&channel_name("#general")).is_none());
    assert!(channels.get(&channel_name("#random")).is_none());
}

#[tokio::test]
async fn quit_broadcasts_to_channel_members() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);

    // Both join #general.
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {} // drain Bob's JOIN from Alice

    // Alice quits.
    let quit = Message::new(Command::Quit, vec!["Leaving".to_owned()]);
    handle_message(&quit, 1, &registry, &channels, &tx_a, &mut state_a, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    // Bob should receive the QUIT message.
    let reply = rx_b.try_recv().unwrap();
    assert_eq!(reply.command, Command::Quit);
    assert_eq!(reply.trailing().unwrap(), "Leaving");
    // Verify prefix is Alice's.
    let prefix = reply.prefix.as_ref().unwrap().to_string();
    assert!(prefix.contains("Alice"));
}

#[tokio::test]
async fn quit_broadcasts_once_per_user_across_shared_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);

    // Both join #general and #random.
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {}

    handle_message(
        &join_msg("#random"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#random"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {}

    // Alice quits.
    let quit = Message::new(Command::Quit, vec!["Bye".to_owned()]);
    handle_message(&quit, 1, &registry, &channels, &tx_a, &mut state_a, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    // Bob should receive exactly ONE QUIT message (deduplicated across channels).
    let reply = rx_b.try_recv().unwrap();
    assert_eq!(reply.command, Command::Quit);

    // No more QUIT messages.
    assert!(rx_b.try_recv().is_err());
}

#[tokio::test]
async fn quit_cleans_up_empty_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);

    // Alice joins #alone (only member), and both join #shared.
    handle_message(
        &join_msg("#alone"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#shared"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#shared"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {}

    // Alice quits.
    let quit = Message::new(Command::Quit, vec!["Gone".to_owned()]);
    handle_message(&quit, 1, &registry, &channels, &tx_a, &mut state_a, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    // #alone should be cleaned up (empty).
    assert!(channels.get(&channel_name("#alone")).is_none());
    // #shared should still exist (Bob is still in it).
    assert!(channels.get(&channel_name("#shared")).is_some());

    // Verify Bob is still in #shared.
    let ch_arc = channels.get(&channel_name("#shared")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert_eq!(ch.member_count(), 1);
    assert!(ch.members.contains_key(&Nickname::new("Bob").unwrap()));
}

#[tokio::test]
async fn names_comma_separated_channels() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    handle_message(
        &join_msg("#chan1"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}
    handle_message(
        &join_msg("#chan2"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx.try_recv().is_ok() {}

    // NAMES #chan1,#chan2
    handle_message(
        &names_msg_with_channel("#chan1,#chan2"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Should get NAMREPLY + ENDOFNAMES for each channel.
    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_NAMREPLY));

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_ENDOFNAMES));

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_NAMREPLY));

    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.command, Command::Numeric(RPL_ENDOFNAMES));
}

#[tokio::test]
async fn list_multiple_channels_shows_correct_member_counts() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let (tx_a, mut rx_a, mut state_a) = register_user(
        "Alice",
        "alice",
        1,
        "localhost",
        &registry,
        &channels,
        &config,
    );
    let (tx_b, mut rx_b, mut state_b) =
        register_user("Bob", "bob", 2, "localhost", &registry, &channels, &config);

    // Alice joins #general, Bob joins #general and #random.
    handle_message(
        &join_msg("#general"),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}
    while rx_a.try_recv().is_ok() {}
    handle_message(
        &join_msg("#random"),
        2,
        &registry,
        &channels,
        &tx_b,
        &mut state_b,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx_b.try_recv().is_ok() {}

    handle_message(
        &list_msg(),
        1,
        &registry,
        &channels,
        &tx_a,
        &mut state_a,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );

    // Collect RPL_LIST replies.
    let mut list_replies = vec![];
    loop {
        let reply = rx_a.try_recv().unwrap();
        if reply.command == Command::Numeric(RPL_LISTEND) {
            break;
        }
        assert_eq!(reply.command, Command::Numeric(RPL_LIST));
        list_replies.push(reply);
    }

    assert_eq!(list_replies.len(), 2);

    // Sort by channel name for deterministic assertions.
    list_replies.sort_by(|a, b| a.params[1].cmp(&b.params[1]));

    assert_eq!(list_replies[0].params[1], "#general");
    assert_eq!(list_replies[0].params[2], "2"); // Alice + Bob
    assert_eq!(list_replies[1].params[1], "#random");
    assert_eq!(list_replies[1].params[2], "1"); // Bob only
}
