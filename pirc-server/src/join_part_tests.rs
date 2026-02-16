use pirc_common::{ChannelMode, ChannelName};
use pirc_protocol::numeric::{
    ERR_BADCHANNELKEY, ERR_BANNEDCHANNEL, ERR_CHANNELISFULL, ERR_INVITEONLYCHAN, ERR_NOSUCHCHANNEL,
    ERR_NOTONCHANNEL, RPL_ENDOFNAMES, RPL_NAMREPLY, RPL_NOTOPIC, RPL_TOPIC, RPL_TOPICWHOTIME,
};

use super::*;
use crate::channel::{BanEntry, MemberStatus};
use crate::handler_channel::{glob_match, matches_ban_mask};

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

fn join_msg_with_key(channels: &str, keys: &str) -> Message {
    Message::new(Command::Join, vec![channels.to_owned(), keys.to_owned()])
}

fn part_msg(channels: &str) -> Message {
    Message::new(Command::Part, vec![channels.to_owned()])
}

fn part_msg_with_reason(channels: &str, reason: &str) -> Message {
    Message::new(Command::Part, vec![channels.to_owned(), reason.to_owned()])
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
    // Drain welcome burst (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD)
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- JOIN: basic functionality ----

#[tokio::test]
async fn join_creates_channel_and_grants_operator() {
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

    // Should receive JOIN echo.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // RPL_NOTOPIC (no topic set)
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_NOTOPIC));

    // RPL_NAMREPLY
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_NAMREPLY));
    assert_eq!(reply.params[1], "="); // public channel
    assert_eq!(reply.params[2], "#general");
    // First user should be @Alice (operator)
    assert!(reply.trailing().unwrap().contains("@Alice"));

    // RPL_ENDOFNAMES
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFNAMES));

    // Verify channel state.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.member_count(), 1);
    let nick = Nickname::new("Alice").unwrap();
    assert_eq!(channel.members.get(&nick), Some(&MemberStatus::Operator));
}

#[tokio::test]
async fn join_second_user_gets_normal_status() {
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

    // Alice joins first (gets +o).
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
    while rx1.try_recv().is_ok() {} // drain Alice's JOIN replies

    // Bob joins second (gets Normal).
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

    // Alice should receive Bob's JOIN broadcast.
    let alice_recv = rx1.recv().await.unwrap();
    assert_eq!(alice_recv.command, Command::Join);
    assert_eq!(alice_recv.params[0], "#general");
    assert_eq!(
        alice_recv.prefix.as_ref().unwrap().to_string(),
        "Bob!bob@127.0.0.2"
    );

    // Bob receives: JOIN echo, topic, NAMES
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_NOTOPIC));

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_NAMREPLY));
    let names = reply.trailing().unwrap();
    assert!(names.contains("@Alice"));
    assert!(names.contains("Bob"));
    // Bob should NOT have a prefix (normal user)
    assert!(!names.contains("@Bob"));

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFNAMES));

    // Verify channel state.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.member_count(), 2);
    let bob_nick = Nickname::new("Bob").unwrap();
    assert_eq!(channel.members.get(&bob_nick), Some(&MemberStatus::Normal));
}

#[tokio::test]
async fn join_sends_topic_when_set() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Pre-create the channel with a topic.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.topic = Some((
            "Welcome to general!".to_owned(),
            "Op".to_owned(),
            1700000000,
        ));
        // Add a dummy member so the channel isn't "new" for the joining user.
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

    // JOIN echo
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);

    // RPL_TOPIC
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_TOPIC));
    assert_eq!(reply.trailing().unwrap(), "Welcome to general!");

    // RPL_TOPICWHOTIME
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_TOPICWHOTIME));
    assert_eq!(reply.params[2], "Op");
    assert_eq!(reply.params[3], "1700000000");
}

#[tokio::test]
async fn join_no_params_returns_err() {
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

    let msg = Message::new(Command::Join, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn join_invalid_channel_name_returns_err() {
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

    // Channel names must start with #
    handle_message(
        &join_msg("nochanprefix"),
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
async fn join_duplicate_is_silently_ignored() {
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
    while rx.try_recv().is_ok() {} // drain first JOIN replies

    // Join again - should be silently ignored.
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

    // No messages should be sent for duplicate join.
    assert!(rx.try_recv().is_err());

    // Channel should still have exactly 1 member.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.member_count(), 1);
}

#[tokio::test]
async fn join_multiple_channels() {
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
        &join_msg("#chan1,#chan2"),
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

    // First channel: JOIN + NOTOPIC + NAMREPLY + ENDOFNAMES
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);
    assert_eq!(reply.params[0], "#chan1");

    let _ = rx.recv().await.unwrap(); // RPL_NOTOPIC
    let _ = rx.recv().await.unwrap(); // RPL_NAMREPLY
    let _ = rx.recv().await.unwrap(); // RPL_ENDOFNAMES

    // Second channel: JOIN + NOTOPIC + NAMREPLY + ENDOFNAMES
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);
    assert_eq!(reply.params[0], "#chan2");

    let _ = rx.recv().await.unwrap(); // RPL_NOTOPIC
    let _ = rx.recv().await.unwrap(); // RPL_NAMREPLY
    let _ = rx.recv().await.unwrap(); // RPL_ENDOFNAMES

    // Both channels should exist.
    assert_eq!(channels.channel_count(), 2);
}

// ---- JOIN: mode restrictions ----

#[tokio::test]
async fn join_invite_only_without_invite_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Create an invite-only channel with a member.
    let chan_name = ChannelName::new("#secret").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
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
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_INVITEONLYCHAN));
}

#[tokio::test]
async fn join_invite_only_with_invite_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Create an invite-only channel and invite Alice.
    let chan_name = ChannelName::new("#secret").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
        channel.invite_list.insert(Nickname::new("Alice").unwrap());
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
    &Arc::new(GroupRegistry::new()),
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);

    // Alice should have been removed from invite list.
    let channel = channel_arc.read().unwrap();
    assert!(!channel
        .invite_list
        .contains(&Nickname::new("Alice").unwrap()));
}

#[tokio::test]
async fn join_key_required_wrong_key_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#locked").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.key = Some("secret123".to_owned());
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

    handle_message(
        &join_msg_with_key("#locked", "wrongkey"),
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
    assert_eq!(reply.numeric_code(), Some(ERR_BADCHANNELKEY));
}

#[tokio::test]
async fn join_key_required_correct_key_succeeds() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#locked").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.key = Some("secret123".to_owned());
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

    handle_message(
        &join_msg_with_key("#locked", "secret123"),
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
    assert_eq!(reply.command, Command::Join);
}

#[tokio::test]
async fn join_key_required_no_key_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#locked").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.key = Some("secret123".to_owned());
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

    // No key provided
    handle_message(
        &join_msg("#locked"),
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
    assert_eq!(reply.numeric_code(), Some(ERR_BADCHANNELKEY));
}

#[tokio::test]
async fn join_user_limit_reached_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#limited").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel.user_limit = Some(1);
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

    handle_message(
        &join_msg("#limited"),
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
    assert_eq!(reply.numeric_code(), Some(ERR_CHANNELISFULL));
}

#[tokio::test]
async fn join_banned_user_returns_err() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#moderated").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
        channel.ban_list.push(BanEntry {
            mask: "*!*@127.0.0.1".to_owned(),
            who_set: "Op".to_owned(),
            timestamp: 100,
        });
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

    handle_message(
        &join_msg("#moderated"),
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
    assert_eq!(reply.numeric_code(), Some(ERR_BANNEDCHANNEL));
}

#[tokio::test]
async fn join_ban_with_wildcard_nick_matches() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    let chan_name = ChannelName::new("#banned").unwrap();
    let channel_arc = channels.get_or_create(chan_name.clone());
    {
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
        channel.ban_list.push(BanEntry {
            mask: "Alice!*@*".to_owned(),
            who_set: "Op".to_owned(),
            timestamp: 100,
        });
    }

    let (tx, mut rx, mut state) = register_user(
        "Alice", "alice", 1, "10.0.0.5", &registry, &channels, &config,
    );

    handle_message(
        &join_msg("#banned"),
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
    assert_eq!(reply.numeric_code(), Some(ERR_BANNEDCHANNEL));
}

// ---- PART: basic functionality ----

#[tokio::test]
async fn part_removes_user_and_broadcasts() {
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

    // Alice parts.
    handle_message(
        &part_msg("#general"),
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

    // Alice receives PART message.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Part);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Bob also receives Alice's PART.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Part);
    assert_eq!(reply.params[0], "#general");

    // Channel should have only Bob.
    let chan_name = ChannelName::new("#general").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.member_count(), 1);
    let alice_nick = Nickname::new("Alice").unwrap();
    assert!(!channel.members.contains_key(&alice_nick));
}

#[tokio::test]
async fn part_with_reason() {
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

    handle_message(
        &part_msg_with_reason("#general", "Goodbye!"),
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
    assert_eq!(reply.command, Command::Part);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "Goodbye!");
}

#[tokio::test]
async fn part_last_user_removes_channel() {
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
        &join_msg("#temp"),
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
    assert_eq!(channels.channel_count(), 1);

    handle_message(
        &part_msg("#temp"),
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

    // Channel should be removed.
    assert_eq!(channels.channel_count(), 0);
    let chan_name = ChannelName::new("#temp").unwrap();
    assert!(channels.get(&chan_name).is_none());
}

#[tokio::test]
async fn part_not_on_channel_returns_err() {
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

    handle_message(
        &part_msg("#general"),
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
async fn part_nonexistent_channel_returns_err() {
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
        &part_msg("#nonexistent"),
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
async fn part_invalid_channel_name_returns_err() {
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
        &part_msg("nochanprefix"),
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
async fn part_no_params_returns_err() {
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

    let msg = Message::new(Command::Part, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn part_multiple_channels() {
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

    // Join two channels.
    handle_message(
        &join_msg("#chan1,#chan2"),
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
    assert_eq!(channels.channel_count(), 2);

    // Part both.
    handle_message(
        &part_msg("#chan1,#chan2"),
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

    // PART #chan1
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Part);
    assert_eq!(reply.params[0], "#chan1");

    // PART #chan2
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Part);
    assert_eq!(reply.params[0], "#chan2");

    // Both channels should be cleaned up.
    assert_eq!(channels.channel_count(), 0);
}

// ---- QUIT removes user from channels ----

#[tokio::test]
async fn quit_removes_user_from_channels() {
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

    // Join two channels.
    handle_message(
        &join_msg("#chan1,#chan2"),
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
    assert_eq!(channels.channel_count(), 2);

    // Quit.
    let quit = Message::new(Command::Quit, vec!["Bye".to_owned()]);
    handle_message(&quit, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()), &Arc::new(GroupRegistry::new()));

    // Both channels should be cleaned up (Alice was the only member).
    assert_eq!(channels.channel_count(), 0);
}

// ---- Glob matching ----

#[test]
fn glob_match_exact() {
    assert!(glob_match("hello", "hello"));
    assert!(!glob_match("hello", "world"));
}

#[test]
fn glob_match_star() {
    assert!(glob_match("*", "anything"));
    assert!(glob_match("he*", "hello"));
    assert!(glob_match("*lo", "hello"));
    assert!(glob_match("h*o", "hello"));
    assert!(glob_match("*!*@*", "nick!user@host"));
}

#[test]
fn glob_match_question() {
    assert!(glob_match("h?llo", "hello"));
    assert!(!glob_match("h?llo", "hllo"));
}

#[test]
fn ban_mask_case_insensitive() {
    assert!(matches_ban_mask("ALICE!*@*", "alice!user@host"));
    assert!(matches_ban_mask("*!*@HOST.COM", "nick!user@host.com"));
}
