use pirc_common::{ChannelMode, ChannelName};
use pirc_protocol::numeric::{RPL_BANLIST, RPL_CHANNELMODEIS, RPL_ENDOFBANLIST};

use super::*;
use crate::channel::BanEntry;

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
    let (tx, mut rx, mut state) = register_user(
        "Alice",
        "alice",
        1,
        "127.0.0.1",
        &registry,
        &channels,
        &config,
    );

    // Alice joins #test (gets +o).
    handle_message(
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );
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
        None,
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
    );
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
        None,
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
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
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
    );
    while rx.try_recv().is_ok() {}

    handle_message(
        &mode_msg(&["#test", "+b"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
        None,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFBANLIST));
}

#[tokio::test]
async fn mode_ban_list_query_with_entries() {
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
    );
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
        None,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_BANLIST));
    assert_eq!(reply.params[2], "*!*@evil.host");
    assert_eq!(reply.params[3], "Alice");

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFBANLIST));
}
