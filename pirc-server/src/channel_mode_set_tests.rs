use pirc_common::{ChannelMode, ChannelName, Nickname};

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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
    );
    assert!(state.registered, "registration should have completed");
    // Drain welcome burst.
    while rx.try_recv().is_ok() {}
    (tx, rx, state)
}

// ---- Mode set: flag modes ----

#[tokio::test]
async fn mode_set_invite_only() {
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+i"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+imn"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+k", "mypassword"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+l", "25"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    let bob_nick = Nickname::new("Bob").unwrap();
    assert_eq!(
        channel.members.get(&bob_nick),
        Some(&MemberStatus::Operator)
    );

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
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Give Bob +o first.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel
            .members
            .insert(Nickname::new("Bob").unwrap(), MemberStatus::Operator);
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+b", "*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
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
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );
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
        None,
        &Arc::new(PreKeyBundleStore::new()),
    );

    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.ban_list.is_empty());

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
}
