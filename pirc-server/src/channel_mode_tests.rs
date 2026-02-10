use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, ERR_UNKNOWNMODE,
    ERR_USERNOTINCHANNEL, RPL_BANLIST, RPL_CHANNELMODEIS, RPL_ENDOFBANLIST,
};

use super::*;
use crate::channel::{BanEntry, MemberStatus};

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

fn mode_msg(params: &[&str]) -> Message {
    Message::new(
        Command::Mode,
        params.iter().map(|s| s.to_string()).collect(),
    )
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
    );
    handle_message(
        &user_msg(username, &format!("{nick} Test")),
        connection_id,
        registry,
        channels,
        &tx,
        &mut state,
        config,
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- MODE query ----

#[tokio::test]
async fn mode_query_returns_channel_modes() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // Alice joins #test (gets +o).
    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set some modes directly.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
        channel.modes.insert(ChannelMode::Moderated);
        channel.modes.insert(ChannelMode::TopicProtected);
    }

    // Query modes.
    handle_message(
        &mode_msg(&["#test"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_CHANNELMODEIS));
    assert_eq!(reply.params[1], "#test");
    // Mode string should contain +imt (sorted).
    let mode_str = &reply.params[2];
    assert!(mode_str.starts_with('+'));
    assert!(mode_str.contains('i'));
    assert!(mode_str.contains('m'));
    assert!(mode_str.contains('t'));
}

#[tokio::test]
async fn mode_query_with_key_and_limit() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set key and limit.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.key = Some("secret".to_owned());
        channel.user_limit = Some(50);
    }

    handle_message(
        &mode_msg(&["#test"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_CHANNELMODEIS));
    let mode_str = &reply.params[2];
    assert!(mode_str.contains('k'));
    assert!(mode_str.contains('l'));
    assert!(mode_str.contains("secret"));
    assert!(mode_str.contains("50"));
}

#[tokio::test]
async fn mode_query_empty_modes() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_CHANNELMODEIS));
    assert_eq!(reply.params[2], "+");
}

// ---- Ban list query ----

#[tokio::test]
async fn mode_ban_list_query_empty() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+b"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFBANLIST));
}

#[tokio::test]
async fn mode_ban_list_query_with_entries() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Add a ban entry directly.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.ban_list.push(BanEntry {
            mask: "*!*@evil.host".to_owned(),
            who_set: "Alice".to_owned(),
            timestamp: 1700000000,
        });
    }

    handle_message(
        &mode_msg(&["#test", "+b"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_BANLIST));
    assert_eq!(reply.params[2], "*!*@evil.host");
    assert_eq!(reply.params[3], "Alice");

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFBANLIST));
}

// ---- Mode set: flag modes ----

#[tokio::test]
async fn mode_set_invite_only() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+i"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    // Verify mode was set.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.modes.contains(&ChannelMode::InviteOnly));

    // Should receive broadcast.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert_eq!(reply.params[0], "#test");
    assert_eq!(reply.params[1], "+i");
}

#[tokio::test]
async fn mode_unset_invite_only() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set first, then unset.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
    }

    handle_message(
        &mode_msg(&["#test", "-i"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(!channel.modes.contains(&ChannelMode::InviteOnly));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert_eq!(reply.params[1], "-i");
}

#[tokio::test]
async fn mode_set_multiple_flags() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+imn"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.modes.contains(&ChannelMode::InviteOnly));
    assert!(channel.modes.contains(&ChannelMode::Moderated));
    assert!(channel.modes.contains(&ChannelMode::NoExternalMessages));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('i'));
    assert!(reply.params[1].contains('m'));
    assert!(reply.params[1].contains('n'));
}

// ---- Mode set: key and limit ----

#[tokio::test]
async fn mode_set_key() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+k", "mypassword"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.key.as_deref(), Some("mypassword"));
    // Key is stored only in the dedicated field, not in the modes HashSet.
    assert!(
        !channel.modes.iter().any(|m| m.mode_char() == 'k'),
        "KeyRequired should not be in modes HashSet"
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('k'));
    assert_eq!(reply.params[2], "mypassword");
}

#[tokio::test]
async fn mode_unset_key() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set key first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.key = Some("oldkey".to_owned());
    }

    handle_message(
        &mode_msg(&["#test", "-k"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.key.is_none());

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('k'));
}

#[tokio::test]
async fn mode_set_limit() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+l", "25"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.user_limit, Some(25));
    // Limit is stored only in the dedicated field, not in the modes HashSet.
    assert!(
        !channel.modes.iter().any(|m| m.mode_char() == 'l'),
        "UserLimit should not be in modes HashSet"
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('l'));
    assert_eq!(reply.params[2], "25");
}

#[tokio::test]
async fn mode_unset_limit() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set limit first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.user_limit = Some(50);
    }

    handle_message(
        &mode_msg(&["#test", "-l"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.user_limit.is_none());

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('l'));
}

// ---- Mode set: +o and +v ----

#[tokio::test]
async fn mode_set_operator() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx1, &mut state1, &config);
    handle_message(&join_msg("#test"), 2, &registry, &channels, &tx2, &mut state2, &config);
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Alice grants +o to Bob.
    handle_message(
        &mode_msg(&["#test", "+o", "Bob"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let bob_nick = Nickname::new("Bob").unwrap();
    assert_eq!(channel.members.get(&bob_nick), Some(&MemberStatus::Operator));

    // Both should receive broadcast.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('o'));
    assert_eq!(reply.params[2], "Bob");

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
}

#[tokio::test]
async fn mode_unset_operator() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx1, &mut state1, &config);
    handle_message(&join_msg("#test"), 2, &registry, &channels, &tx2, &mut state2, &config);
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Give Bob +o first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.members.insert(Nickname::new("Bob").unwrap(), MemberStatus::Operator);
    }

    // Alice removes -o from Bob.
    handle_message(
        &mode_msg(&["#test", "-o", "Bob"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let bob_nick = Nickname::new("Bob").unwrap();
    assert_eq!(channel.members.get(&bob_nick), Some(&MemberStatus::Normal));

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
}

#[tokio::test]
async fn mode_set_voice() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx1, &mut state1, &config);
    handle_message(&join_msg("#test"), 2, &registry, &channels, &tx2, &mut state2, &config);
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Alice grants +v to Bob.
    handle_message(
        &mode_msg(&["#test", "+v", "Bob"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let bob_nick = Nickname::new("Bob").unwrap();
    assert_eq!(channel.members.get(&bob_nick), Some(&MemberStatus::Voiced));

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('v'));
    assert_eq!(reply.params[2], "Bob");
}

// ---- Mode set: ban (+b / -b) ----

#[tokio::test]
async fn mode_add_ban() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+b", "*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.ban_list.len(), 1);
    assert_eq!(channel.ban_list[0].mask, "*!*@evil.host");
    assert_eq!(channel.ban_list[0].who_set, "Alice");

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert!(reply.params[1].contains('b'));
    assert_eq!(reply.params[2], "*!*@evil.host");
}

#[tokio::test]
async fn mode_remove_ban() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Add a ban first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.ban_list.push(BanEntry {
            mask: "*!*@evil.host".to_owned(),
            who_set: "Alice".to_owned(),
            timestamp: 1000,
        });
    }

    handle_message(
        &mode_msg(&["#test", "-b", "*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.ban_list.is_empty());

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
}

// ---- Error cases ----

#[tokio::test]
async fn mode_nonexistent_channel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(
        &mode_msg(&["#nonexistent"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHCHANNEL));
}

#[tokio::test]
async fn mode_not_on_channel() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();

    // Create a channel with another user.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get_or_create(chan_name);
    {
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Op").unwrap(), MemberStatus::Operator);
    }

    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    // Alice tries to set modes on a channel she's not in.
    handle_message(
        &mode_msg(&["#test", "+i"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOTONCHANNEL));
}

#[tokio::test]
async fn mode_non_operator_cannot_set() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx1, &mut state1, &config);
    handle_message(&join_msg("#test"), 2, &registry, &channels, &tx2, &mut state2, &config);
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Bob (non-op) tries to set +i.
    handle_message(
        &mode_msg(&["#test", "+i"]),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CHANOPRIVSNEEDED));

    // Mode should not have been set.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(!channel.modes.contains(&ChannelMode::InviteOnly));
}

#[tokio::test]
async fn mode_unknown_mode_char() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+x"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_UNKNOWNMODE));
}

#[tokio::test]
async fn mode_user_not_in_channel_for_op() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Try to +o a user who isn't in the channel.
    handle_message(
        &mode_msg(&["#test", "+o", "Ghost"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_USERNOTINCHANNEL));
}

// ---- Broadcast ----

#[tokio::test]
async fn mode_change_broadcasts_to_all_members() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx1, mut rx1, mut state1) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);
    let (tx2, mut rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);
    let (tx3, mut rx3, mut state3) =
        register_user("Charlie", "charlie", 3, "127.0.0.3", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx1, &mut state1, &config);
    handle_message(&join_msg("#test"), 2, &registry, &channels, &tx2, &mut state2, &config);
    handle_message(&join_msg("#test"), 3, &registry, &channels, &tx3, &mut state3, &config);
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}
    while rx3.try_recv().is_ok() {}

    // Alice sets +m.
    handle_message(
        &mode_msg(&["#test", "+m"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    // All three should receive the MODE broadcast.
    let reply1 = rx1.recv().await.unwrap();
    assert_eq!(reply1.command, Command::Mode);
    assert_eq!(reply1.params[0], "#test");

    let reply2 = rx2.recv().await.unwrap();
    assert_eq!(reply2.command, Command::Mode);

    let reply3 = rx3.recv().await.unwrap();
    assert_eq!(reply3.command, Command::Mode);

    // Check sender prefix.
    assert_eq!(
        reply1.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );
}

// ---- All 9 channel modes ----

#[tokio::test]
async fn mode_set_all_flag_modes() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set all 5 flag modes at once.
    handle_message(
        &mode_msg(&["#test", "+imnst"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.modes.contains(&ChannelMode::InviteOnly));
    assert!(channel.modes.contains(&ChannelMode::Moderated));
    assert!(channel.modes.contains(&ChannelMode::NoExternalMessages));
    assert!(channel.modes.contains(&ChannelMode::Secret));
    assert!(channel.modes.contains(&ChannelMode::TopicProtected));

    while rx.try_recv().is_ok() {}
}

#[tokio::test]
async fn mode_unset_all_flag_modes() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set all modes first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
        channel.modes.insert(ChannelMode::Moderated);
        channel.modes.insert(ChannelMode::NoExternalMessages);
        channel.modes.insert(ChannelMode::Secret);
        channel.modes.insert(ChannelMode::TopicProtected);
    }

    // Unset all 5 flag modes at once.
    handle_message(
        &mode_msg(&["#test", "-imnst"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.modes.is_empty());

    while rx.try_recv().is_ok() {}
}

// ---- Mixed add/remove in one modestring ----

#[tokio::test]
async fn mode_mixed_add_remove() {
    let registry = Arc::new(UserRegistry::new());
    let channels = make_channels();
    let config = make_config();
    let (tx, mut rx, mut state) =
        register_user("Alice", "alice", 1, "127.0.0.1", &registry, &channels, &config);

    handle_message(&join_msg("#test"), 1, &registry, &channels, &tx, &mut state, &config);
    while rx.try_recv().is_ok() {}

    // Set +i first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::InviteOnly);
    }

    // +m-i: add moderated, remove invite only.
    handle_message(
        &mode_msg(&["#test", "+m-i"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.modes.contains(&ChannelMode::Moderated));
    assert!(!channel.modes.contains(&ChannelMode::InviteOnly));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
}
