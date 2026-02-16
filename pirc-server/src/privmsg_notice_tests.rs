use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{ERR_CANNOTSENDTOCHAN, ERR_NEEDMOREPARAMS, ERR_NOSUCHNICK, RPL_AWAY};

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

fn privmsg(target: &str, text: &str) -> Message {
    Message::new(Command::Privmsg, vec![target.to_owned(), text.to_owned()])
}

fn notice(target: &str, text: &str) -> Message {
    Message::new(Command::Notice, vec![target.to_owned(), text.to_owned()])
}

fn away_msg(text: &str) -> Message {
    Message::new(Command::Away, vec![text.to_owned()])
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

// ---- Channel PRIVMSG ----

#[tokio::test]
async fn privmsg_channel_delivered_to_members_except_sender() {
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

    // All three join #general.
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
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    handle_message(
        &join_msg("#general"),
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

    // Alice sends PRIVMSG to #general.
    handle_message(
        &privmsg("#general", "Hello everyone!"),
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

    // Alice should NOT receive her own message.
    assert!(rx1.try_recv().is_err());

    // Bob should receive the message.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "Hello everyone!");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Charlie should receive the message.
    let reply = rx3.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "Hello everyone!");
}

// ---- Channel NOTICE ----

#[tokio::test]
async fn notice_channel_delivered_to_members_except_sender() {
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
        &tx2,
        &mut state2,
        &config,
        None,
        &Arc::new(PreKeyBundleStore::new()),
        &Arc::new(OfflineMessageStore::default()),
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Alice sends NOTICE to #general.
    handle_message(
        &notice("#general", "Server notice"),
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

    // Alice should NOT receive her own notice.
    assert!(rx1.try_recv().is_err());

    // Bob should receive the notice.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert_eq!(reply.params[0], "#general");
    assert_eq!(reply.params[1], "Server notice");
}

// ---- Moderated mode (+m) ----

#[tokio::test]
async fn privmsg_moderated_channel_normal_user_blocked() {
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
        &join_msg("#moderated"),
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
        &join_msg("#moderated"),
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
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +m on channel.
    {
        let chan_name = ChannelName::new("#moderated").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::Moderated);
    }

    // Bob (normal) tries to send a message — should be blocked.
    handle_message(
        &privmsg("#moderated", "I want to speak"),
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

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CANNOTSENDTOCHAN));

    // Alice should NOT have received the message.
    assert!(rx1.try_recv().is_err());
}

#[tokio::test]
async fn privmsg_moderated_channel_voiced_user_allowed() {
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
        &join_msg("#moderated"),
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
        &join_msg("#moderated"),
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
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +m on channel, give Bob +v.
    {
        let chan_name = ChannelName::new("#moderated").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::Moderated);
        channel
            .members
            .insert(Nickname::new("Bob").unwrap(), MemberStatus::Voiced);
    }

    // Bob (voiced) sends a message — should succeed.
    handle_message(
        &privmsg("#moderated", "I can speak"),
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

    // Alice should receive Bob's message.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[1], "I can speak");
}

#[tokio::test]
async fn privmsg_moderated_channel_operator_allowed() {
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
        &join_msg("#moderated"),
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
        &join_msg("#moderated"),
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
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +m on channel. Alice is already operator (first joiner).
    {
        let chan_name = ChannelName::new("#moderated").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::Moderated);
    }

    // Alice (operator) sends a message — should succeed.
    handle_message(
        &privmsg("#moderated", "Op speaking"),
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

    // Bob should receive Alice's message.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[1], "Op speaking");
}

// ---- No external messages mode (+n) ----

#[tokio::test]
async fn privmsg_no_external_messages_blocks_non_member() {
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

    // Alice joins, Bob does NOT join.
    handle_message(
        &join_msg("#private"),
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
    while rx1.try_recv().is_ok() {}

    // Set +n on channel.
    {
        let chan_name = ChannelName::new("#private").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::NoExternalMessages);
    }

    // Bob (not a member) tries to send — should be blocked.
    handle_message(
        &privmsg("#private", "I'm outside"),
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

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CANNOTSENDTOCHAN));

    // Alice should NOT have received the message.
    assert!(rx1.try_recv().is_err());
}

#[tokio::test]
async fn privmsg_no_external_messages_allows_member() {
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
        &join_msg("#private"),
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
        &join_msg("#private"),
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
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +n on channel.
    {
        let chan_name = ChannelName::new("#private").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::NoExternalMessages);
    }

    // Bob (member) sends — should succeed.
    handle_message(
        &privmsg("#private", "I'm inside"),
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

    // Alice should receive the message.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[1], "I'm inside");
}

#[tokio::test]
async fn privmsg_without_no_external_allows_non_member() {
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
    let (tx2, _rx2, mut state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice joins, Bob does NOT join. Channel has NO +n mode.
    handle_message(
        &join_msg("#open"),
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
    while rx1.try_recv().is_ok() {}

    // Bob (not a member) sends to a channel without +n — should succeed.
    handle_message(
        &privmsg("#open", "From outside"),
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

    // Alice should receive the message.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[1], "From outside");
}

// ---- Private PRIVMSG (user-to-user) ----

#[tokio::test]
async fn privmsg_user_to_user_delivered() {
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

    // Alice sends a private message to Bob.
    handle_message(
        &privmsg("Bob", "Hello Bob!"),
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

    // Bob should receive the message.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "Hello Bob!");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Alice should NOT receive anything back (no echo).
    assert!(rx1.try_recv().is_err());
}

#[tokio::test]
async fn notice_user_to_user_delivered() {
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

    // Alice sends a notice to Bob.
    handle_message(
        &notice("Bob", "Notice to Bob"),
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

    // Bob should receive the notice.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert_eq!(reply.params[0], "Bob");
    assert_eq!(reply.params[1], "Notice to Bob");

    // Alice should NOT receive anything back.
    assert!(rx1.try_recv().is_err());
}

// ---- ERR_NOSUCHNICK ----

#[tokio::test]
async fn privmsg_nonexistent_user_returns_err_nosuchnick() {
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

    handle_message(
        &privmsg("Nobody", "Hello?"),
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

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));
}

#[tokio::test]
async fn notice_nonexistent_user_returns_err_nosuchnick() {
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

    handle_message(
        &notice("Nobody", "Notice?"),
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

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));
}

// ---- ERR_CANNOTSENDTOCHAN for nonexistent channel ----

#[tokio::test]
async fn privmsg_nonexistent_channel_returns_err_cannotsendtochan() {
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

    handle_message(
        &privmsg("#nonexistent", "Hello?"),
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

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CANNOTSENDTOCHAN));
}

// ---- ERR_NEEDMOREPARAMS ----

#[tokio::test]
async fn privmsg_no_params_returns_err_needmoreparams() {
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

    let msg = Message::new(Command::Privmsg, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn privmsg_one_param_returns_err_needmoreparams() {
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

    let msg = Message::new(Command::Privmsg, vec!["Bob".to_owned()]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config, None, &Arc::new(PreKeyBundleStore::new()), &Arc::new(OfflineMessageStore::default()));

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

// ---- AWAY replies ----

#[tokio::test]
async fn privmsg_to_away_user_sends_rpl_away() {
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

    // Bob sets away.
    handle_message(
        &away_msg("Gone fishing"),
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
    while rx2.try_recv().is_ok() {}

    // Alice sends PRIVMSG to Bob.
    handle_message(
        &privmsg("Bob", "Are you there?"),
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

    // Alice should receive RPL_AWAY.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_AWAY));
    assert!(reply.params.iter().any(|p| p == "Bob"));
    // The trailing param should be the away message.
    assert_eq!(reply.params.last().unwrap(), "Gone fishing");

    // Bob should still receive the message.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Privmsg);
    assert_eq!(reply.params[1], "Are you there?");
}

#[tokio::test]
async fn notice_to_away_user_does_not_send_rpl_away() {
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

    // Bob sets away.
    handle_message(
        &away_msg("Gone fishing"),
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
    while rx2.try_recv().is_ok() {}

    // Alice sends NOTICE to Bob.
    handle_message(
        &notice("Bob", "FYI"),
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

    // Alice should NOT receive RPL_AWAY for NOTICE.
    assert!(rx1.try_recv().is_err());

    // Bob should still receive the notice.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Notice);
    assert_eq!(reply.params[1], "FYI");
}

// ---- Combined moderated + no external messages ----

#[tokio::test]
async fn privmsg_moderated_and_no_external_both_enforced() {
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

    // Alice and Bob join.
    handle_message(
        &join_msg("#strict"),
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
        &join_msg("#strict"),
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
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Set +mn on channel.
    {
        let chan_name = ChannelName::new("#strict").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.modes.insert(ChannelMode::Moderated);
        channel.modes.insert(ChannelMode::NoExternalMessages);
    }

    // Charlie (not a member) tries to send — blocked by +n.
    handle_message(
        &privmsg("#strict", "External msg"),
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
    let reply = rx3.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CANNOTSENDTOCHAN));

    // Bob (member, normal) tries to send — blocked by +m.
    handle_message(
        &privmsg("#strict", "Normal user msg"),
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
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CANNOTSENDTOCHAN));

    // Alice (operator) sends — should succeed.
    handle_message(
        &privmsg("#strict", "Op msg"),
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
    assert_eq!(reply.params[1], "Op msg");
}
