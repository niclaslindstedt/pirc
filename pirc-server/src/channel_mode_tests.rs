use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, ERR_UNKNOWNMODE,
    ERR_USERNOTINCHANNEL,
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

// ---- Error cases ----

#[tokio::test]
async fn mode_nonexistent_channel() {
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

    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

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
    );
    handle_message(
        &join_msg("#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );
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
    );
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
    );
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
    );
    handle_message(
        &join_msg("#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );
    handle_message(
        &join_msg("#test"),
        3,
        &registry,
        &channels,
        &tx3,
        &mut state3,
        &config,
    );
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
    );
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
    );
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
    );
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
