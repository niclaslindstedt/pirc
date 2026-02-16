use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, RPL_NOTOPIC, RPL_TOPIC,
    RPL_TOPICWHOTIME,
};

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

fn topic_msg(channel: &str) -> Message {
    Message::new(Command::Topic, vec![channel.to_owned()])
}

fn topic_msg_set(channel: &str, topic: &str) -> Message {
    Message::new(Command::Topic, vec![channel.to_owned(), topic.to_owned()])
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
    &Arc::new(GroupRegistry::new()),
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
    &Arc::new(GroupRegistry::new()),
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- TOPIC: query ----

#[tokio::test]
async fn topic_query_no_topic_returns_rpl_notopic() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice joins #general (creates it).
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
    &Arc::new(GroupRegistry::new()),
    );
    while rx.try_recv().is_ok() {}

    // Query topic.
    handle_message(
        &topic_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_NOTOPIC));
    assert_eq!(reply.trailing().unwrap(), "No topic is set");
}

#[tokio::test]
async fn topic_query_with_topic_returns_rpl_topic_and_whotime() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice joins #general.
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
    &Arc::new(GroupRegistry::new()),
    );
    while rx.try_recv().is_ok() {}

    // Set topic directly on the channel.
    {
        let chan_name = ChannelName::new("#general").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.topic = Some(("Welcome!".to_owned(), "Alice".to_owned(), 1700000000));
    }

    // Query topic.
    handle_message(
        &topic_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_TOPIC));
    assert_eq!(reply.trailing().unwrap(), "Welcome!");

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_TOPICWHOTIME));
    assert_eq!(reply.params[2], "Alice");
    assert_eq!(reply.params[3], "1700000000");
}

// ---- TOPIC: set ----

#[tokio::test]
async fn topic_set_broadcasts_to_channel() {
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

    // Both join #general.
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
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Alice sets the topic.
    handle_message(
        &topic_msg_set("#general", "New topic!"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    // Alice receives TOPIC broadcast.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Topic);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "New topic!");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Bob also receives TOPIC broadcast.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Topic);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "New topic!");

    // Verify channel state.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let (text, who, _ts) = channel.topic.as_ref().unwrap();
    assert_eq!(text, "New topic!");
    assert_eq!(who, "Alice");
}

#[tokio::test]
async fn topic_set_by_normal_user_without_topic_protected() {
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

    // Alice joins (gets +o), Bob joins (gets Normal).
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
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Bob (normal user) sets topic. No +t mode, so it should succeed.
    handle_message(
        &topic_msg_set("#general", "Bob's topic"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    // Bob receives TOPIC broadcast.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Topic);
    assert_eq!(reply.params[1], "Bob's topic");

    // Verify channel state.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let (text, who, _) = channel.topic.as_ref().unwrap();
    assert_eq!(text, "Bob's topic");
    assert_eq!(who, "Bob");
}

// ---- TOPIC: clear ----

#[tokio::test]
async fn topic_clear_with_empty_trailing() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice joins.
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
    &Arc::new(GroupRegistry::new()),
    );
    while rx.try_recv().is_ok() {}

    // Set a topic first.
    handle_message(
        &topic_msg_set("#general", "Some topic"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    while rx.try_recv().is_ok() {}

    // Verify topic is set.
    {
        let chan_name = ChannelName::new("#general").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let channel = channel_arc.read().unwrap();
        assert!(channel.topic.is_some());
    }

    // Clear topic with empty string.
    handle_message(
        &topic_msg_set("#general", ""),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    // Should receive TOPIC broadcast with empty trailing.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Topic);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "");

    // Verify topic is cleared.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.topic.is_none());
}

// ---- TOPIC: +t mode enforcement ----

#[tokio::test]
async fn topic_protected_mode_denies_normal_user() {
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

    // Alice joins (gets +o), Bob joins (gets Normal).
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
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +t mode on the channel.
    {
        let chan_name = ChannelName::new("#general").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::TopicProtected);
    }

    // Bob (normal user) tries to set topic. Should be denied.
    handle_message(
        &topic_msg_set("#general", "Bob's topic"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CHANOPRIVSNEEDED));

    // Topic should remain unset.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.topic.is_none());
}

#[tokio::test]
async fn topic_protected_mode_allows_operator() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice joins (gets +o).
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
    &Arc::new(GroupRegistry::new()),
    );
    while rx.try_recv().is_ok() {}

    // Set +t mode.
    {
        let chan_name = ChannelName::new("#general").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::TopicProtected);
    }

    // Alice (operator) sets topic. Should succeed.
    handle_message(
        &topic_msg_set("#general", "Op's topic"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Topic);
    assert_eq!(reply.params[1], "Op's topic");

    // Verify channel state.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let (text, _, _) = channel.topic.as_ref().unwrap();
    assert_eq!(text, "Op's topic");
}

// ---- TOPIC: error cases ----

#[tokio::test]
async fn topic_no_params_returns_err_needmoreparams() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    let msg = Message::new(Command::Topic, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn topic_nonexistent_channel_returns_err_nosuchchannel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &topic_msg("#nonexistent"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHCHANNEL));
}

#[tokio::test]
async fn topic_invalid_channel_name_returns_err_nosuchchannel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    handle_message(
        &topic_msg("nochanprefix"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHCHANNEL));
}

#[tokio::test]
async fn topic_not_on_channel_returns_err_notonchannel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Create a channel with another user.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get_or_create(chan_name);
    {
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
    }

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice tries to query topic on channel she's not in.
    handle_message(
        &topic_msg("#general"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOTONCHANNEL));
}

#[tokio::test]
async fn topic_set_not_on_channel_returns_err_notonchannel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Create a channel with another user.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get_or_create(chan_name);
    {
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
    }

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice tries to set topic on channel she's not in.
    handle_message(
        &topic_msg_set("#general", "Sneaky topic"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOTONCHANNEL));
}

#[tokio::test]
async fn topic_protected_voiced_user_denied() {
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

    // Alice joins (operator), Bob joins (normal).
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
    &Arc::new(GroupRegistry::new()),
    );
    handle_message(
        &join_msg("#general"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +t mode and give Bob +v (voiced, not operator).
    {
        let chan_name = ChannelName::new("#general").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::TopicProtected);
        let bob_nick = Nickname::new("Bob").unwrap();
        channel.members.insert(bob_nick, MemberStatus::Voiced);
    }

    // Bob (voiced) tries to set topic with +t. Should be denied.
    handle_message(
        &topic_msg_set("#general", "Voiced topic"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CHANOPRIVSNEEDED));
}
