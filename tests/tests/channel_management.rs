//! Channel management integration tests.
//!
//! Validates server channel operations through the full handler pipeline:
//! join/part flow, topic management, channel modes, kick/ban, and multi-user
//! scenarios — all using real TCP connections via the shared test harness.

use pirc_integration_tests::common::{
    assert_command, assert_numeric, assert_param_contains, invite_msg, join_msg,
    join_msg_with_key, kick_msg, kick_msg_with_reason, mode_msg, mode_msg_with_params, notice_msg,
    part_msg, privmsg, topic_msg, topic_query, TestClient, TestServer,
};
use pirc_protocol::numeric::{
    ERR_BADCHANNELKEY, ERR_CANNOTSENDTOCHAN, ERR_CHANNELISFULL, ERR_CHANOPRIVSNEEDED,
    ERR_INVITEONLYCHAN, RPL_CHANNELMODEIS, RPL_ENDOFNAMES, RPL_INVITING, RPL_NAMREPLY,
    RPL_NOTOPIC, RPL_TOPIC, RPL_TOPICWHOTIME,
};
use pirc_protocol::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register a client and join it to a channel, draining the join burst
/// (JOIN echo, topic reply, RPL_NAMREPLY, RPL_ENDOFNAMES).
async fn register_and_join(
    server: &TestServer,
    nick: &str,
    channel: &str,
) -> TestClient {
    let mut client = TestClient::connect(server.addr).await;
    client.register(nick, &nick.to_ascii_lowercase()).await;
    client.send(join_msg(channel)).await;
    // JOIN echo + RPL_NOTOPIC/RPL_TOPIC + RPL_NAMREPLY + RPL_ENDOFNAMES = 4
    client.drain(4).await;
    client
}

// ===========================================================================
// 1. Channel Join/Part Flow
// ===========================================================================

#[tokio::test]
async fn join_returns_echo_topic_names() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(join_msg("#lobby")).await;

    // 1. JOIN echo
    let join = alice.recv_msg().await;
    assert_command(&join, Command::Join);
    assert_param_contains(&join, 0, "#lobby");

    // 2. RPL_NOTOPIC (new channel has no topic)
    let notopic = alice.recv_msg().await;
    assert_numeric(&notopic, RPL_NOTOPIC);

    // 3. RPL_NAMREPLY — should contain @Alice (operator)
    let names = alice.recv_msg().await;
    assert_numeric(&names, RPL_NAMREPLY);
    assert_param_contains(&names, 3, "@Alice");

    // 4. RPL_ENDOFNAMES
    let endnames = alice.recv_msg().await;
    assert_numeric(&endnames, RPL_ENDOFNAMES);
}

#[tokio::test]
async fn second_client_join_notifies_first() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#lobby").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#lobby")).await;

    // Bob gets his own join burst (4 messages)
    bob.drain(4).await;

    // Alice receives Bob's JOIN notification
    let bob_join = alice.recv_msg().await;
    assert_command(&bob_join, Command::Join);
    assert_param_contains(&bob_join, 0, "#lobby");
    // The prefix should indicate Bob
    let prefix_str = format!("{}", bob_join.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("Bob"),
        "expected prefix to contain Bob, got {prefix_str}"
    );
}

#[tokio::test]
async fn part_notifies_other_members() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#lobby").await;
    let mut bob = register_and_join(&server, "Bob", "#lobby").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    bob.send(part_msg("#lobby")).await;

    // Bob receives his own PART
    let bob_part = bob.recv_msg().await;
    assert_command(&bob_part, Command::Part);

    // Alice receives Bob's PART notification
    let alice_notif = alice.recv_msg().await;
    assert_command(&alice_notif, Command::Part);
}

#[tokio::test]
async fn first_joiner_becomes_operator() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;
    alice.send(join_msg("#newchan")).await;

    // Skip JOIN echo
    alice.recv_msg().await;
    // Skip RPL_NOTOPIC
    alice.recv_msg().await;

    // RPL_NAMREPLY should show @Alice (operator prefix)
    let names = alice.recv_msg().await;
    assert_numeric(&names, RPL_NAMREPLY);
    assert_param_contains(&names, 3, "@Alice");
}

#[tokio::test]
async fn join_with_key() {
    let server = TestServer::start().await;

    // Alice creates channel and sets key
    let mut alice = register_and_join(&server, "Alice", "#secret").await;
    alice.send(mode_msg_with_params("#secret", "+k", &["mykey"])).await;
    // Drain mode confirmation
    alice.recv_msg().await;

    // Bob tries to join without key — should fail
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#secret")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_BADCHANNELKEY);

    // Bob joins with correct key — should succeed
    bob.send(join_msg_with_key("#secret", "mykey")).await;
    let join = bob.recv_msg().await;
    assert_command(&join, Command::Join);
    assert_param_contains(&join, 0, "#secret");
}

// ===========================================================================
// 2. Topic Management
// ===========================================================================

#[tokio::test]
async fn set_and_query_topic() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#dev").await;

    // Set topic
    alice.send(topic_msg("#dev", "Welcome to #dev!")).await;
    // Alice receives the broadcast TOPIC message
    let topic_notif = alice.recv_msg().await;
    assert_command(&topic_notif, Command::Topic);
    assert_param_contains(&topic_notif, 1, "Welcome to #dev!");

    // Query topic
    alice.send(topic_query("#dev")).await;
    let rpl_topic = alice.recv_msg().await;
    assert_numeric(&rpl_topic, RPL_TOPIC);
    assert_param_contains(&rpl_topic, 2, "Welcome to #dev!");

    // RPL_TOPICWHOTIME follows
    let who_time = alice.recv_msg().await;
    assert_numeric(&who_time, RPL_TOPICWHOTIME);
}

#[tokio::test]
async fn topic_broadcast_to_other_members() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#dev").await;
    let mut bob = register_and_join(&server, "Bob", "#dev").await;

    // Drain Bob's JOIN notification on Alice's side
    alice.recv_msg().await;

    alice.send(topic_msg("#dev", "New topic")).await;
    // Alice sees the TOPIC broadcast
    alice.recv_msg().await;

    // Bob should also see the TOPIC broadcast
    let bob_topic = bob.recv_msg().await;
    assert_command(&bob_topic, Command::Topic);
    assert_param_contains(&bob_topic, 1, "New topic");
}

#[tokio::test]
async fn topic_protection_mode() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#dev").await;
    let mut bob = register_and_join(&server, "Bob", "#dev").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    // Alice sets +t (topic protected)
    alice.send(mode_msg_with_params("#dev", "+t", &[])).await;
    // Alice gets mode broadcast
    alice.recv_msg().await;
    // Bob gets mode broadcast
    bob.recv_msg().await;

    // Bob (non-op) tries to set topic — should fail
    bob.send(topic_msg("#dev", "Bob's topic")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_CHANOPRIVSNEEDED);

    // Alice (op) can still set topic
    alice.send(topic_msg("#dev", "Alice's topic")).await;
    let topic_notif = alice.recv_msg().await;
    assert_command(&topic_notif, Command::Topic);
    assert_param_contains(&topic_notif, 1, "Alice's topic");
}

// ===========================================================================
// 3. Channel Modes
// ===========================================================================

#[tokio::test]
async fn invite_only_mode_enforcement() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#private").await;

    // Set +i
    alice.send(mode_msg_with_params("#private", "+i", &[])).await;
    alice.recv_msg().await; // mode broadcast

    // Bob tries to join — should be rejected
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#private")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_INVITEONLYCHAN);

    // Alice invites Bob
    alice.send(invite_msg("Bob", "#private")).await;
    // Alice gets RPL_INVITING
    let inviting = alice.recv_msg().await;
    assert_numeric(&inviting, RPL_INVITING);

    // Bob receives the INVITE message
    let invite_recv = bob.recv_msg().await;
    assert_command(&invite_recv, Command::Invite);

    // Now Bob can join
    bob.send(join_msg("#private")).await;
    let join = bob.recv_msg().await;
    assert_command(&join, Command::Join);
    assert_param_contains(&join, 0, "#private");
}

#[tokio::test]
async fn moderated_mode_enforcement() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#moderated").await;
    let mut bob = register_and_join(&server, "Bob", "#moderated").await;

    // Drain Bob's JOIN notification on Alice
    alice.recv_msg().await;

    // Set +m
    alice.send(mode_msg_with_params("#moderated", "+m", &[])).await;
    alice.recv_msg().await; // mode broadcast to alice
    bob.recv_msg().await; // mode broadcast to bob

    // Bob (normal member) tries to send — should fail
    bob.send(privmsg("#moderated", "hello")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_CANNOTSENDTOCHAN);

    // Alice (op) voices Bob
    alice.send(mode_msg_with_params("#moderated", "+v", &["Bob"])).await;
    alice.recv_msg().await; // mode broadcast to alice
    bob.recv_msg().await; // mode broadcast to bob

    // Bob (voiced) can now send
    bob.send(privmsg("#moderated", "hello voiced")).await;
    let msg = alice.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello voiced");
}

#[tokio::test]
async fn user_limit_enforcement() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#limited").await;

    // Set limit to 2
    alice.send(mode_msg_with_params("#limited", "+l", &["2"])).await;
    alice.recv_msg().await; // mode broadcast

    // Bob joins (2/2)
    let mut bob = register_and_join(&server, "Bob", "#limited").await;
    // Drain Bob's join notification on Alice
    alice.recv_msg().await;

    // Carol tries to join (3/2) — should fail
    let mut carol = TestClient::connect(server.addr).await;
    carol.register("Carol", "carol").await;
    carol.send(join_msg("#limited")).await;
    let err = carol.recv_msg().await;
    assert_numeric(&err, ERR_CHANNELISFULL);

    // Bob parts, freeing a slot
    bob.send(part_msg("#limited")).await;
    bob.recv_msg().await; // Bob's PART echo
    alice.recv_msg().await; // Alice sees Bob's PART

    // Carol can now join (2/2)
    carol.send(join_msg("#limited")).await;
    let join = carol.recv_msg().await;
    assert_command(&join, Command::Join);
    assert_param_contains(&join, 0, "#limited");
}

#[tokio::test]
async fn op_deop_users() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#ops").await;
    let mut bob = register_and_join(&server, "Bob", "#ops").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    // Alice gives Bob +o
    alice.send(mode_msg_with_params("#ops", "+o", &["Bob"])).await;
    alice.recv_msg().await; // mode broadcast
    let bob_mode = bob.recv_msg().await;
    assert_command(&bob_mode, Command::Mode);
    assert_param_contains(&bob_mode, 1, "+o");
    assert_param_contains(&bob_mode, 2, "Bob");

    // Verify Bob is now an operator by having Bob set a mode
    bob.send(mode_msg_with_params("#ops", "+t", &[])).await;
    let bob_set = bob.recv_msg().await;
    assert_command(&bob_set, Command::Mode);
    assert_param_contains(&bob_set, 1, "+t");

    // Alice removes Bob's +o
    // Drain Alice's +t notification first
    alice.recv_msg().await;
    alice.send(mode_msg_with_params("#ops", "-o", &["Bob"])).await;
    alice.recv_msg().await; // mode broadcast
    bob.recv_msg().await; // bob sees deop

    // Bob (now normal) tries to set mode — should fail
    bob.send(mode_msg_with_params("#ops", "+m", &[])).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_CHANOPRIVSNEEDED);
}

#[tokio::test]
async fn voice_devoice_users() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#voice").await;
    let mut bob = register_and_join(&server, "Bob", "#voice").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    // Alice voices Bob
    alice.send(mode_msg_with_params("#voice", "+v", &["Bob"])).await;
    alice.recv_msg().await;
    let bob_mode = bob.recv_msg().await;
    assert_command(&bob_mode, Command::Mode);
    assert_param_contains(&bob_mode, 1, "+v");
    assert_param_contains(&bob_mode, 2, "Bob");

    // Alice devoices Bob
    alice.send(mode_msg_with_params("#voice", "-v", &["Bob"])).await;
    alice.recv_msg().await;
    let bob_demode = bob.recv_msg().await;
    assert_command(&bob_demode, Command::Mode);
    assert_param_contains(&bob_demode, 1, "-v");
}

#[tokio::test]
async fn channel_mode_query() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#modes").await;

    // Set several modes
    alice
        .send(mode_msg_with_params("#modes", "+imt", &[]))
        .await;
    alice.recv_msg().await; // mode broadcast

    // Query channel modes
    alice.send(mode_msg("#modes", "")).await;
    // Since mode_msg sends MODE #modes "" — the server needs just MODE #channel
    // Let's use a raw message for the query
    alice
        .send(pirc_protocol::Message::new(
            Command::Mode,
            vec!["#modes".to_owned()],
        ))
        .await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_CHANNELMODEIS);
    // The mode string should contain i, m, t
    let mode_str = &reply.params[2];
    assert!(mode_str.contains('i'), "expected +i in {mode_str}");
    assert!(mode_str.contains('m'), "expected +m in {mode_str}");
    assert!(mode_str.contains('t'), "expected +t in {mode_str}");
}

// ===========================================================================
// 4. Kick and Ban
// ===========================================================================

#[tokio::test]
async fn operator_kicks_user() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#kicktest").await;
    let mut bob = register_and_join(&server, "Bob", "#kicktest").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    alice
        .send(kick_msg_with_reason("#kicktest", "Bob", "behave"))
        .await;

    // Both Alice and Bob should see the KICK
    let alice_kick = alice.recv_msg().await;
    assert_command(&alice_kick, Command::Kick);
    assert_param_contains(&alice_kick, 1, "Bob");
    assert_param_contains(&alice_kick, 2, "behave");

    let bob_kick = bob.recv_msg().await;
    assert_command(&bob_kick, Command::Kick);
    assert_param_contains(&bob_kick, 1, "Bob");
}

#[tokio::test]
async fn non_operator_cannot_kick() {
    let server = TestServer::start().await;
    let mut _alice = register_and_join(&server, "Alice", "#kicktest2").await;
    let mut bob = register_and_join(&server, "Bob", "#kicktest2").await;

    bob.send(kick_msg("#kicktest2", "Alice")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_CHANOPRIVSNEEDED);
}

#[tokio::test]
async fn ban_prevents_join() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#bans").await;

    // Ban Bob's user mask
    alice
        .send(mode_msg_with_params("#bans", "+b", &["Bob!*@*"]))
        .await;
    alice.recv_msg().await; // mode broadcast

    // Bob tries to join — should be banned
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#bans")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, pirc_protocol::numeric::ERR_BANNEDCHANNEL);
}

#[tokio::test]
async fn ban_removal_allows_join() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#bans2").await;

    // Ban Bob
    alice
        .send(mode_msg_with_params("#bans2", "+b", &["Bob!*@*"]))
        .await;
    alice.recv_msg().await;

    // Verify Bob is banned
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#bans2")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, pirc_protocol::numeric::ERR_BANNEDCHANNEL);

    // Remove ban
    alice
        .send(mode_msg_with_params("#bans2", "-b", &["Bob!*@*"]))
        .await;
    alice.recv_msg().await;

    // Bob can now join
    bob.send(join_msg("#bans2")).await;
    let join = bob.recv_msg().await;
    assert_command(&join, Command::Join);
    assert_param_contains(&join, 0, "#bans2");
}

#[tokio::test]
async fn kicked_and_banned_cannot_rejoin() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#kickban").await;
    let mut bob = register_and_join(&server, "Bob", "#kickban").await;

    // Drain Alice's notification of Bob's join
    alice.recv_msg().await;

    // Ban Bob, then kick
    alice
        .send(mode_msg_with_params("#kickban", "+b", &["Bob!*@*"]))
        .await;
    alice.recv_msg().await; // ban mode broadcast
    bob.recv_msg().await; // bob sees ban broadcast

    alice.send(kick_msg("#kickban", "Bob")).await;
    alice.recv_msg().await; // kick broadcast
    bob.recv_msg().await; // bob sees kick

    // Bob tries to rejoin — should be banned
    bob.send(join_msg("#kickban")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, pirc_protocol::numeric::ERR_BANNEDCHANNEL);
}

// ===========================================================================
// 5. Multi-User Scenarios
// ===========================================================================

#[tokio::test]
async fn channel_privmsg_reaches_all_others() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#multi").await;
    let mut bob = register_and_join(&server, "Bob", "#multi").await;
    let mut carol = register_and_join(&server, "Carol", "#multi").await;

    // Drain join notifications
    // Alice sees Bob join, then Carol join
    alice.recv_msg().await;
    alice.recv_msg().await;
    // Bob sees Carol join
    bob.recv_msg().await;

    // Alice sends a PRIVMSG to #multi
    alice.send(privmsg("#multi", "hello everyone")).await;

    // Bob receives it
    let bob_msg = bob.recv_msg().await;
    assert_command(&bob_msg, Command::Privmsg);
    assert_param_contains(&bob_msg, 1, "hello everyone");

    // Carol receives it
    let carol_msg = carol.recv_msg().await;
    assert_command(&carol_msg, Command::Privmsg);
    assert_param_contains(&carol_msg, 1, "hello everyone");

    // Alice does NOT receive her own message back
    let no_msg = alice.try_recv_short().await;
    assert!(no_msg.is_none(), "sender should not receive own PRIVMSG");
}

#[tokio::test]
async fn channel_notice_reaches_all_members() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#notice").await;
    let mut bob = register_and_join(&server, "Bob", "#notice").await;

    // Drain join notifications
    alice.recv_msg().await;

    alice.send(notice_msg("#notice", "important notice")).await;

    let bob_notice = bob.recv_msg().await;
    assert_command(&bob_notice, Command::Notice);
    assert_param_contains(&bob_notice, 1, "important notice");
}

#[tokio::test]
async fn mixed_modes_ops_voiced_regular() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#mixed").await;
    let mut bob = register_and_join(&server, "Bob", "#mixed").await;
    let mut carol = register_and_join(&server, "Carol", "#mixed").await;

    // Drain join notifications
    alice.recv_msg().await; // Bob joined
    alice.recv_msg().await; // Carol joined
    bob.recv_msg().await; // Carol joined

    // Alice is operator (first joiner), voice Bob
    alice
        .send(mode_msg_with_params("#mixed", "+v", &["Bob"]))
        .await;
    alice.recv_msg().await; // mode broadcast
    bob.recv_msg().await; // mode broadcast
    carol.recv_msg().await; // mode broadcast

    // Set moderated
    alice
        .send(mode_msg_with_params("#mixed", "+m", &[]))
        .await;
    alice.recv_msg().await;
    bob.recv_msg().await;
    carol.recv_msg().await;

    // Alice (op) can speak
    alice.send(privmsg("#mixed", "from op")).await;
    let bob_msg = bob.recv_msg().await;
    assert_command(&bob_msg, Command::Privmsg);
    assert_param_contains(&bob_msg, 1, "from op");
    let carol_msg = carol.recv_msg().await;
    assert_command(&carol_msg, Command::Privmsg);
    assert_param_contains(&carol_msg, 1, "from op");

    // Bob (voiced) can speak
    bob.send(privmsg("#mixed", "from voiced")).await;
    let alice_msg = alice.recv_msg().await;
    assert_command(&alice_msg, Command::Privmsg);
    assert_param_contains(&alice_msg, 1, "from voiced");
    let carol_msg2 = carol.recv_msg().await;
    assert_command(&carol_msg2, Command::Privmsg);
    assert_param_contains(&carol_msg2, 1, "from voiced");

    // Carol (normal) cannot speak in moderated channel
    carol.send(privmsg("#mixed", "from normal")).await;
    let err = carol.recv_msg().await;
    assert_numeric(&err, ERR_CANNOTSENDTOCHAN);
}

#[tokio::test]
async fn key_mode_set_and_remove() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#keyed").await;

    // Set key
    alice
        .send(mode_msg_with_params("#keyed", "+k", &["secret"]))
        .await;
    alice.recv_msg().await; // mode broadcast

    // Verify key required
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#keyed")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_BADCHANNELKEY);

    // Remove key
    alice
        .send(mode_msg_with_params("#keyed", "-k", &[]))
        .await;
    alice.recv_msg().await;

    // Bob can now join without key
    bob.send(join_msg("#keyed")).await;
    let join = bob.recv_msg().await;
    assert_command(&join, Command::Join);
}

#[tokio::test]
async fn multiple_mode_changes_at_once() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#combo").await;

    // Set +imt at once
    alice
        .send(mode_msg_with_params("#combo", "+imt", &[]))
        .await;
    let mode_reply = alice.recv_msg().await;
    assert_command(&mode_reply, Command::Mode);
    let mode_str = &mode_reply.params[1];
    assert!(mode_str.contains('i'), "expected i in {mode_str}");
    assert!(mode_str.contains('m'), "expected m in {mode_str}");
    assert!(mode_str.contains('t'), "expected t in {mode_str}");
}

#[tokio::test]
async fn non_operator_cannot_set_modes() {
    let server = TestServer::start().await;
    let mut _alice = register_and_join(&server, "Alice", "#noperm").await;
    let mut bob = register_and_join(&server, "Bob", "#noperm").await;

    bob.send(mode_msg_with_params("#noperm", "+i", &[])).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_CHANOPRIVSNEEDED);
}

#[tokio::test]
async fn join_creates_channel_and_part_cleans_up() {
    let server = TestServer::start().await;

    assert_eq!(server.channels.channel_count(), 0);

    let mut alice = register_and_join(&server, "Alice", "#temp").await;
    assert_eq!(server.channels.channel_count(), 1);

    alice.send(part_msg("#temp")).await;
    alice.recv_msg().await; // PART echo

    // Small delay for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        server.channels.channel_count(),
        0,
        "empty channel should be removed"
    );
}

#[tokio::test]
async fn join_existing_topic_shown_to_new_user() {
    let server = TestServer::start().await;
    let mut alice = register_and_join(&server, "Alice", "#topictest").await;

    // Set a topic
    alice.send(topic_msg("#topictest", "Welcome!")).await;
    alice.recv_msg().await; // TOPIC broadcast

    // Bob joins — should see the existing topic
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;
    bob.send(join_msg("#topictest")).await;

    // JOIN echo
    let join = bob.recv_msg().await;
    assert_command(&join, Command::Join);

    // RPL_TOPIC with existing topic
    let topic = bob.recv_msg().await;
    assert_numeric(&topic, RPL_TOPIC);
    assert_param_contains(&topic, 2, "Welcome!");

    // RPL_TOPICWHOTIME
    let who_time = bob.recv_msg().await;
    assert_numeric(&who_time, RPL_TOPICWHOTIME);

    // RPL_NAMREPLY + RPL_ENDOFNAMES
    bob.recv_msg().await;
    bob.recv_msg().await;
}
