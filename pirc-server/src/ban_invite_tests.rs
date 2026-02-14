use pirc_common::{ChannelName, Nickname};
use pirc_protocol::numeric::{
    ERR_BANNEDCHANNEL, ERR_CHANOPRIVSNEEDED, ERR_INVITEONLYCHAN, ERR_NEEDMOREPARAMS,
    ERR_NOSUCHCHANNEL, ERR_NOSUCHNICK, ERR_NOTONCHANNEL, ERR_USERONCHANNEL, RPL_BANLIST,
    RPL_ENDOFBANLIST, RPL_INVITING,
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

fn invite_msg(target: &str, channel: &str) -> Message {
    Message::new(Command::Invite, vec![target.to_owned(), channel.to_owned()])
}

fn ban_msg(params: &[&str]) -> Message {
    Message::new(Command::Ban, params.iter().map(|s| s.to_string()).collect())
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

// ============================================================
// INVITE tests
// ============================================================

// ---- INVITE: success ----

#[tokio::test]
async fn invite_success_sends_rpl_inviting_and_invite_message() {
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

    // Alice joins #test (gets +o).
    handle_message(
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}

    // Alice invites Bob.
    handle_message(
        &invite_msg("Bob", "#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    // Alice receives RPL_INVITING.
    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_INVITING));
    assert_eq!(reply.params[1], "Bob");
    assert_eq!(reply.params[2], "#test");

    // Bob receives INVITE message.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Invite);
    assert_eq!(reply.params[0], "Bob");
    // Channel should be in trailing.
    assert!(reply.params.iter().any(|p| p == "#test"));
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );
}

#[tokio::test]
async fn invite_adds_target_to_invite_list() {
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
    let (_tx2, _rx2, _state2) =
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
    while rx1.try_recv().is_ok() {}

    handle_message(
        &invite_msg("Bob", "#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    // Bob should be in the invite list.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.invite_list.contains(&Nickname::new("Bob").unwrap()));
}

#[tokio::test]
async fn invite_non_invite_only_channel_no_op_required() {
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
    let (_tx3, mut rx3, _state3) = register_user(
        "Charlie",
        "charlie",
        3,
        "127.0.0.3",
        &registry,
        &channels,
        &config,
    );

    // Alice joins (gets +o), Bob joins (Normal).
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

    // Bob (non-op) invites Charlie to a non-+i channel — should succeed.
    handle_message(
        &invite_msg("Charlie", "#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_INVITING));

    // Charlie should receive INVITE.
    let reply = rx3.recv().await.unwrap();
    assert_eq!(reply.command, Command::Invite);
}

// ---- INVITE: invited user can join +i channel ----

#[tokio::test]
async fn invited_user_can_join_invite_only_channel() {
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

    // Alice joins #secret and sets +i.
    handle_message(
        &join_msg("#secret"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}
    handle_message(
        &mode_msg(&["#secret", "+i"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}

    // Bob tries to join without invite — should fail.
    handle_message(
        &join_msg("#secret"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_INVITEONLYCHAN));

    // Alice invites Bob.
    handle_message(
        &invite_msg("Bob", "#secret"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Bob joins — should succeed now.
    handle_message(
        &join_msg("#secret"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    // Should get JOIN broadcast, not an error.
    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.command, Command::Join);
    assert_eq!(reply.params[0], "#secret");

    // Invite should be consumed.
    let chan_name = ChannelName::new("#secret").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(!channel.invite_list.contains(&Nickname::new("Bob").unwrap()));
}

// ---- INVITE: error cases ----

#[tokio::test]
async fn invite_no_params_returns_err_needmoreparams() {
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

    let msg = Message::new(Command::Invite, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn invite_one_param_returns_err_needmoreparams() {
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

    let msg = Message::new(Command::Invite, vec!["Bob".to_owned()]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn invite_target_does_not_exist_returns_err_nosuchnick() {
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
        &invite_msg("Ghost", "#test"),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));
}

#[tokio::test]
async fn invite_nonexistent_channel_returns_err_nosuchchannel() {
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
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    handle_message(
        &invite_msg("Bob", "#nonexistent"),
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
async fn invite_not_on_channel_returns_err_notonchannel() {
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
    let (_tx2, _rx2, _state2) =
        register_user("Bob", "bob", 2, "127.0.0.2", &registry, &channels, &config);

    // Alice is not on #test, tries to invite Bob.
    handle_message(
        &invite_msg("Bob", "#test"),
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
async fn invite_non_op_on_invite_only_channel_returns_err_chanoprivsneeded() {
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
    let (_tx3, _rx3, _state3) = register_user(
        "Charlie",
        "charlie",
        3,
        "127.0.0.3",
        &registry,
        &channels,
        &config,
    );

    // Alice joins (op), Bob joins (normal), set +i.
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

    handle_message(
        &mode_msg(&["#test", "+i"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}
    while rx2.try_recv().is_ok() {}

    // Bob (non-op) tries to invite Charlie to +i channel.
    handle_message(
        &invite_msg("Charlie", "#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CHANOPRIVSNEEDED));
}

#[tokio::test]
async fn invite_target_already_on_channel_returns_err_useronchannel() {
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

    // Both join.
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

    // Alice invites Bob who is already on the channel.
    handle_message(
        &invite_msg("Bob", "#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    let reply = rx1.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_USERONCHANNEL));
}

// ============================================================
// BAN tests
// ============================================================

// ---- BAN: list bans ----

#[tokio::test]
async fn ban_list_empty() {
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

    // BAN #test (no mask) — lists bans.
    handle_message(
        &ban_msg(&["#test"]),
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
async fn ban_list_with_entries() {
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

    // Add bans directly.
    {
        let chan_name = ChannelName::new("#test").unwrap();
        let channel_arc = channels.get(&chan_name).unwrap();
        let mut channel = channel_arc.write().unwrap();
        channel.ban_list.push(BanEntry {
            mask: "*!*@evil.host".to_owned(),
            who_set: "Alice".to_owned(),
            timestamp: 1700000000,
        });
        channel.ban_list.push(BanEntry {
            mask: "troll!*@*".to_owned(),
            who_set: "Alice".to_owned(),
            timestamp: 1700000001,
        });
    }

    handle_message(
        &ban_msg(&["#test"]),
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

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_BANLIST));
    assert_eq!(reply.params[2], "troll!*@*");

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFBANLIST));
}

// ---- BAN: add ban ----

#[tokio::test]
async fn ban_add_mask() {
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
        &ban_msg(&["#test", "*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    // Should receive MODE broadcast.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert_eq!(reply.params[0], "#test");
    assert_eq!(reply.params[1], "+b");
    assert_eq!(reply.params[2], "*!*@evil.host");
    assert_eq!(
        reply.prefix.as_ref().unwrap().to_string(),
        "Alice!alice@127.0.0.1"
    );

    // Verify ban was added.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert_eq!(channel.ban_list.len(), 1);
    assert_eq!(channel.ban_list[0].mask, "*!*@evil.host");
    assert_eq!(channel.ban_list[0].who_set, "Alice");
}

// ---- BAN: remove ban ----

#[tokio::test]
async fn ban_remove_mask() {
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

    // Add ban first.
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

    // Remove with -mask.
    handle_message(
        &ban_msg(&["#test", "-*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx,
        &mut state,
        &config,
    );

    // Should receive MODE broadcast for -b.
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.command, Command::Mode);
    assert_eq!(reply.params[1], "-b");
    assert_eq!(reply.params[2], "*!*@evil.host");

    // Verify ban was removed.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.ban_list.is_empty());
}

// ---- BAN: enforcement in JOIN ----

#[tokio::test]
async fn banned_user_cannot_join() {
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

    // Alice joins and sets a ban on Bob's host.
    handle_message(
        &join_msg("#test"),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}

    handle_message(
        &ban_msg(&["#test", "*!*@127.0.0.2"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );
    while rx1.try_recv().is_ok() {}

    // Bob tries to join.
    handle_message(
        &join_msg("#test"),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_BANNEDCHANNEL));

    // Bob should not be in the channel.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(!channel.members.contains_key(&Nickname::new("Bob").unwrap()));
}

// ---- BAN: ban mask wildcard matching ----

#[tokio::test]
async fn ban_wildcard_matching_star() {
    use crate::handler_channel::matches_ban_mask;

    // * matches any sequence
    assert!(matches_ban_mask("*!*@evil.host", "troll!user@evil.host"));
    assert!(matches_ban_mask("*!*@*", "anyone!user@anyhost"));
    assert!(matches_ban_mask("bad*!*@*", "badguy!user@host"));
    assert!(!matches_ban_mask("*!*@evil.host", "user!user@good.host"));
}

#[tokio::test]
async fn ban_wildcard_matching_question_mark() {
    use crate::handler_channel::glob_match;

    // ? matches single char
    assert!(glob_match("us?r", "user"));
    assert!(!glob_match("us?r", "usr"));
    assert!(!glob_match("us?r", "useer"));
}

#[tokio::test]
async fn ban_matching_case_insensitive() {
    use crate::handler_channel::matches_ban_mask;

    assert!(matches_ban_mask("*!*@EVIL.HOST", "troll!user@evil.host"));
    assert!(matches_ban_mask("*!*@evil.host", "TROLL!USER@EVIL.HOST"));
}

// ---- BAN: error cases ----

#[tokio::test]
async fn ban_no_params_returns_err_needmoreparams() {
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

    let msg = Message::new(Command::Ban, vec![]);
    handle_message(&msg, 1, &registry, &channels, &tx, &mut state, &config);

    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));
}

#[tokio::test]
async fn ban_nonexistent_channel_returns_err_nosuchchannel() {
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
        &ban_msg(&["#nonexistent", "*!*@host"]),
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
async fn ban_not_on_channel_returns_err_notonchannel() {
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

    handle_message(
        &ban_msg(&["#test", "*!*@evil.host"]),
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
async fn ban_non_operator_returns_err_chanoprivsneeded() {
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

    // Bob (non-op) tries to set a ban.
    handle_message(
        &ban_msg(&["#test", "*!*@evil.host"]),
        2,
        &registry,
        &channels,
        &tx2,
        &mut state2,
        &config,
    );

    let reply = rx2.recv().await.unwrap();
    assert_eq!(reply.numeric_code(), Some(ERR_CHANOPRIVSNEEDED));

    // Ban should not have been added.
    let chan_name = ChannelName::new("#test").unwrap();
    let channel_arc = channels.get(&chan_name).unwrap();
    let channel = channel_arc.read().unwrap();
    assert!(channel.ban_list.is_empty());
}

// ---- BAN: broadcast to channel members ----

#[tokio::test]
async fn ban_add_broadcasts_to_all_members() {
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

    // Alice adds a ban.
    handle_message(
        &ban_msg(&["#test", "*!*@evil.host"]),
        1,
        &registry,
        &channels,
        &tx1,
        &mut state1,
        &config,
    );

    // Both Alice and Bob should receive the MODE broadcast.
    let reply1 = rx1.recv().await.unwrap();
    assert_eq!(reply1.command, Command::Mode);
    assert_eq!(reply1.params[1], "+b");

    let reply2 = rx2.recv().await.unwrap();
    assert_eq!(reply2.command, Command::Mode);
    assert_eq!(reply2.params[1], "+b");
}

#[tokio::test]
async fn ban_invalid_channel_name_returns_err_nosuchchannel() {
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
        &ban_msg(&["nohash", "*!*@host"]),
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
