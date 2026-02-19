//! Client-server connection and command flow integration tests.
//!
//! Validates the full end-to-end client-to-server interaction: connection
//! establishment, registration (welcome burst), command processing round-trips
//! (JOIN, PRIVMSG, NICK, QUIT, LIST, WHOIS), PING/PONG keepalive, and error
//! handling — all using real TCP connections via the shared test harness.

use pirc_integration_tests::common::{
    assert_command, assert_numeric, assert_param_contains, join_msg, nick_msg, ping_msg, pong_msg,
    privmsg, quit_msg, user_msg, whois_msg, TestClient, TestServer,
};
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_NICKNAMEINUSE, ERR_NOMOTD, ERR_NONICKNAMEGIVEN, ERR_NOSUCHNICK,
    RPL_CREATED, RPL_ENDOFNAMES, RPL_ENDOFWHOIS, RPL_LIST, RPL_LISTEND, RPL_MOTD, RPL_MOTDSTART,
    RPL_ENDOFMOTD, RPL_NAMREPLY, RPL_NOTOPIC, RPL_WELCOME, RPL_WHOISSERVER, RPL_WHOISUSER,
    RPL_WHOISIDLE, RPL_YOURHOST,
};
use pirc_protocol::Command;
use pirc_server::config::{MotdConfig, ServerConfig};

// ===========================================================================
// 1. Connection & Registration Flow
// ===========================================================================

#[tokio::test]
async fn registration_welcome_burst_full_inspection() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    client.send(nick_msg("WelcomeUser")).await;
    client.send(user_msg("welcomeuser", "Welcome Test")).await;

    // RPL_WELCOME (001)
    let welcome = client.recv_msg().await;
    assert_numeric(&welcome, RPL_WELCOME);
    assert_param_contains(&welcome, 0, "WelcomeUser");

    // RPL_YOURHOST (002)
    let yourhost = client.recv_msg().await;
    assert_numeric(&yourhost, RPL_YOURHOST);

    // RPL_CREATED (003)
    let created = client.recv_msg().await;
    assert_numeric(&created, RPL_CREATED);

    // ERR_NOMOTD (422) — default config has no MOTD
    let nomotd = client.recv_msg().await;
    assert_numeric(&nomotd, ERR_NOMOTD);
}

#[tokio::test]
async fn registration_with_motd() {
    let mut config = ServerConfig::default();
    config.motd = MotdConfig {
        path: None,
        text: Some("Welcome to the PIRC test server!".to_owned()),
    };
    let server = TestServer::with_config(config).await;
    let mut client = TestClient::connect(server.addr).await;

    client.send(nick_msg("MotdUser")).await;
    client.send(user_msg("motduser", "MOTD Test")).await;

    // RPL_WELCOME, RPL_YOURHOST, RPL_CREATED
    let welcome = client.recv_msg().await;
    assert_numeric(&welcome, RPL_WELCOME);
    client.recv_msg().await; // RPL_YOURHOST
    client.recv_msg().await; // RPL_CREATED

    // MOTD: RPL_MOTDSTART (375), RPL_MOTD (372), RPL_ENDOFMOTD (376)
    let motd_start = client.recv_msg().await;
    assert_numeric(&motd_start, RPL_MOTDSTART);

    let motd_line = client.recv_msg().await;
    assert_numeric(&motd_line, RPL_MOTD);
    assert_param_contains(&motd_line, 1, "Welcome to the PIRC test server!");

    let motd_end = client.recv_msg().await;
    assert_numeric(&motd_end, RPL_ENDOFMOTD);
}

#[tokio::test]
async fn nick_only_does_not_complete_registration() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send only NICK without USER
    client.send(nick_msg("OnlyNick")).await;

    // Server should not have registered us yet — no welcome burst
    let msg = client.try_recv_short().await;
    assert!(msg.is_none(), "should not get welcome burst with only NICK");

    // Server registry should have 0 registered users
    assert_eq!(server.users.connection_count(), 0);
}

#[tokio::test]
async fn user_only_does_not_complete_registration() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send only USER without NICK
    client.send(user_msg("onlyuser", "Only User")).await;

    // Server should not register us
    let msg = client.try_recv_short().await;
    assert!(msg.is_none(), "should not get welcome burst with only USER");

    assert_eq!(server.users.connection_count(), 0);
}

#[tokio::test]
async fn duplicate_registration_returns_error() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("DupUser", "dupuser").await;

    // Try to register again
    client.send(user_msg("dupuser2", "Duplicate")).await;
    let err = client.recv_msg().await;
    assert_numeric(&err, ERR_ALREADYREGISTERED);
}

#[tokio::test]
async fn reconnection_after_disconnect() {
    let server = TestServer::start().await;

    // First connection
    let mut client1 = TestClient::connect(server.addr).await;
    client1.register("ReconUser", "reconuser").await;
    assert_eq!(server.users.connection_count(), 1);

    // Disconnect
    client1.send(quit_msg("leaving")).await;
    // Give server time to process the quit
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(server.users.connection_count(), 0);

    // Reconnect with the same nick
    let mut client2 = TestClient::connect(server.addr).await;
    client2.register("ReconUser", "reconuser").await;
    assert_eq!(server.users.connection_count(), 1);
}

#[tokio::test]
async fn registration_order_user_then_nick() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send USER before NICK (reversed order)
    client.send(user_msg("revorder", "Reversed Order")).await;
    client.send(nick_msg("RevOrder")).await;

    // Should still get welcome burst
    let welcome = client.recv_msg().await;
    assert_numeric(&welcome, RPL_WELCOME);
    assert_param_contains(&welcome, 0, "RevOrder");

    // Drain rest of welcome burst
    client.recv_msg().await; // RPL_YOURHOST
    client.recv_msg().await; // RPL_CREATED
    client.recv_msg().await; // ERR_NOMOTD

    assert_eq!(server.users.connection_count(), 1);
}

#[tokio::test]
async fn nick_without_params_returns_error() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send NICK with no parameters
    client
        .send(pirc_protocol::Message::new(Command::Nick, vec![]))
        .await;
    let err = client.recv_msg().await;
    assert_numeric(&err, ERR_NONICKNAMEGIVEN);
}

// ===========================================================================
// 2. Command Processing E2E: JOIN
// ===========================================================================

#[tokio::test]
async fn join_channel_full_roundtrip() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("JoinUser", "joinuser").await;

    client.send(join_msg("#test-join")).await;

    // JOIN echo
    let join_echo = client.recv_msg().await;
    assert_command(&join_echo, Command::Join);
    assert_param_contains(&join_echo, 0, "#test-join");

    // RPL_NOTOPIC (331)
    let notopic = client.recv_msg().await;
    assert_numeric(&notopic, RPL_NOTOPIC);

    // RPL_NAMREPLY (353) — first joiner is operator
    let names = client.recv_msg().await;
    assert_numeric(&names, RPL_NAMREPLY);
    assert_param_contains(&names, 3, "@JoinUser");

    // RPL_ENDOFNAMES (366)
    let endnames = client.recv_msg().await;
    assert_numeric(&endnames, RPL_ENDOFNAMES);

    // Verify channel exists on server
    assert_eq!(server.channels.channel_count(), 1);
}

#[tokio::test]
async fn join_second_client_receives_notification() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("JAlice", "jalice").await;
    alice.send(join_msg("#flow-join")).await;
    alice.drain(4).await; // JOIN echo + topic + names + endnames

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("JBob", "jbob").await;
    bob.send(join_msg("#flow-join")).await;
    bob.drain(4).await;

    // Alice should receive Bob's JOIN notification
    let bob_join = alice.recv_msg().await;
    assert_command(&bob_join, Command::Join);
    let prefix_str = format!("{}", bob_join.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("JBob"),
        "expected Bob in prefix, got {prefix_str}"
    );
}

// ===========================================================================
// 3. Command Processing E2E: PRIVMSG
// ===========================================================================

#[tokio::test]
async fn privmsg_user_to_user_roundtrip() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("MsgAlice", "msgalice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("MsgBob", "msgbob").await;

    alice.send(privmsg("MsgBob", "Hello from Alice!")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 0, "MsgBob");
    assert_param_contains(&msg, 1, "Hello from Alice!");

    // Verify sender prefix
    let prefix_str = format!("{}", msg.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("MsgAlice"),
        "expected sender nick in prefix, got {prefix_str}"
    );
}

#[tokio::test]
async fn privmsg_to_channel_roundtrip() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("CAlice", "calice").await;
    alice.send(join_msg("#flow-msg")).await;
    alice.drain(4).await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("CBob", "cbob").await;
    bob.send(join_msg("#flow-msg")).await;
    bob.drain(4).await;
    alice.recv_msg().await; // Drain Bob's JOIN notification

    alice.send(privmsg("#flow-msg", "channel message")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 0, "#flow-msg");
    assert_param_contains(&msg, 1, "channel message");

    // Sender should NOT receive echo
    let no_echo = alice.try_recv_short().await;
    assert!(no_echo.is_none(), "sender should not get echo");
}

#[tokio::test]
async fn privmsg_to_nonexistent_target() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("ErrSender", "errsender").await;

    client.send(privmsg("GhostUser", "hello?")).await;
    let err = client.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

// ===========================================================================
// 4. Command Processing E2E: NICK change
// ===========================================================================

#[tokio::test]
async fn nick_change_roundtrip() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("OldNick", "oldnick").await;

    client.send(nick_msg("NewNick")).await;
    let reply = client.recv_msg().await;
    assert_command(&reply, Command::Nick);
    assert_param_contains(&reply, 0, "NewNick");

    // Prefix should contain old nick
    let prefix_str = format!("{}", reply.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("OldNick"),
        "expected old nick in prefix, got {prefix_str}"
    );
}

#[tokio::test]
async fn nick_change_collision_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("NAlice", "nalice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("NBob", "nbob").await;

    // Bob tries to take Alice's nick
    bob.send(nick_msg("NAlice")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_NICKNAMEINUSE);
}

#[tokio::test]
async fn nick_change_messaging_uses_new_nick() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("OrigAlice", "origalice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("OrigBob", "origbob").await;

    // Alice changes nick
    alice.send(nick_msg("RenamedAlice")).await;
    alice.recv_msg().await; // NICK confirmation

    // Bob can message Alice using new nick
    bob.send(privmsg("RenamedAlice", "hello renamed")).await;
    let msg = alice.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello renamed");

    // Old nick should fail
    bob.send(privmsg("OrigAlice", "old nick")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

// ===========================================================================
// 5. Command Processing E2E: QUIT
// ===========================================================================

#[tokio::test]
async fn quit_removes_user_from_registry() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("QuitUser", "quituser").await;
    assert_eq!(server.users.connection_count(), 1);

    client.send(quit_msg("goodbye")).await;

    // Give server time to process
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(server.users.connection_count(), 0);
}

#[tokio::test]
async fn quit_notifies_channel_members() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("QAlice", "qalice").await;
    alice.send(join_msg("#quit-test")).await;
    alice.drain(4).await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("QBob", "qbob").await;
    bob.send(join_msg("#quit-test")).await;
    bob.drain(4).await;
    alice.recv_msg().await; // Drain Bob's JOIN notification

    // Bob quits
    bob.send(quit_msg("leaving now")).await;

    // Alice should receive QUIT notification
    let quit_notif = alice.recv_msg().await;
    assert_command(&quit_notif, Command::Quit);
    assert_param_contains(&quit_notif, 0, "leaving now");
}

// ===========================================================================
// 6. Command Processing E2E: LIST
// ===========================================================================

#[tokio::test]
async fn list_empty_server_returns_listend() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("ListUser", "listuser").await;

    client
        .send(pirc_protocol::Message::new(Command::List, vec![]))
        .await;

    // With no channels, we should get RPL_LISTEND immediately
    let listend = client.recv_msg().await;
    assert_numeric(&listend, RPL_LISTEND);
}

#[tokio::test]
async fn list_shows_existing_channels() {
    let server = TestServer::start().await;

    // Create two channels
    let mut creator = TestClient::connect(server.addr).await;
    creator.register("Creator", "creator").await;
    creator.send(join_msg("#list-alpha")).await;
    creator.drain(4).await;
    creator.send(join_msg("#list-beta")).await;
    creator.drain(4).await;

    let mut lister = TestClient::connect(server.addr).await;
    lister.register("Lister", "lister").await;

    lister
        .send(pirc_protocol::Message::new(Command::List, vec![]))
        .await;

    // Should receive RPL_LIST for each channel, then RPL_LISTEND
    let mut channel_names = Vec::new();
    loop {
        let msg = lister.recv_msg().await;
        if msg.numeric_code() == Some(RPL_LIST) {
            channel_names.push(msg.params[1].clone());
        } else if msg.numeric_code() == Some(RPL_LISTEND) {
            break;
        } else {
            panic!("unexpected message in LIST response: {msg:?}");
        }
    }

    channel_names.sort();
    assert_eq!(channel_names, vec!["#list-alpha", "#list-beta"]);
}

// ===========================================================================
// 7. Command Processing E2E: WHOIS
// ===========================================================================

#[tokio::test]
async fn whois_full_roundtrip() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("WAlice", "walice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("WBob", "wbob").await;

    alice.send(whois_msg("WBob")).await;

    // RPL_WHOISUSER (311)
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);
    assert_param_contains(&user_reply, 1, "WBob");
    assert_param_contains(&user_reply, 2, "wbob");

    // RPL_WHOISSERVER (312)
    let server_reply = alice.recv_msg().await;
    assert_numeric(&server_reply, RPL_WHOISSERVER);

    // RPL_WHOISIDLE (317)
    let idle_reply = alice.recv_msg().await;
    assert_numeric(&idle_reply, RPL_WHOISIDLE);

    // RPL_ENDOFWHOIS (318)
    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

#[tokio::test]
async fn whois_nonexistent_target() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("WhoisErr", "whoiserr").await;

    client.send(whois_msg("NoSuchUser")).await;

    let err = client.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);

    let end = client.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

// ===========================================================================
// 8. PING/PONG Keepalive
// ===========================================================================

#[tokio::test]
async fn ping_pong_keepalive_flow() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("PingUser", "pinguser").await;

    // Client sends PING, server responds with PONG
    client.send(ping_msg("keepalive-token")).await;
    let pong = client.recv_msg().await;
    assert_command(&pong, Command::Pong);
    assert_param_contains(&pong, 0, "keepalive-token");
}

#[tokio::test]
async fn ping_pong_multiple_tokens() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("PingMulti", "pingmulti").await;

    for i in 0..5 {
        let token = format!("token-{i}");
        client.send(ping_msg(&token)).await;
        let pong = client.recv_msg().await;
        assert_command(&pong, Command::Pong);
        assert_param_contains(&pong, 0, &token);
    }
}

#[tokio::test]
async fn ping_pong_pre_registration() {
    // PING should work even before registration is complete
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    client.send(ping_msg("pre-reg-token")).await;
    let pong = client.recv_msg().await;
    assert_command(&pong, Command::Pong);
    assert_param_contains(&pong, 0, "pre-reg-token");
}

#[tokio::test]
async fn pong_from_client_is_silently_accepted() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("PongUser", "ponguser").await;

    // Client sends PONG (as if responding to server's PING)
    client.send(pong_msg("server-token")).await;

    // Server should not send any error back
    let no_reply = client.try_recv_short().await;
    assert!(
        no_reply.is_none(),
        "PONG from client should be silently accepted"
    );
}

// ===========================================================================
// 9. Error Handling
// ===========================================================================

#[tokio::test]
async fn pre_registration_commands_ignored() {
    // Commands other than NICK/USER/PING/PONG/QUIT should be ignored
    // before registration is complete.
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send a JOIN before registering
    client.send(join_msg("#pre-reg-channel")).await;

    // Should get no response (silently ignored)
    let no_reply = client.try_recv_short().await;
    assert!(
        no_reply.is_none(),
        "commands before registration should be silently ignored"
    );
}

#[tokio::test]
async fn connection_stable_after_bad_command() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("StableUser", "stableuser").await;

    // Send a PRIVMSG to non-existent user (triggers ERR_NOSUCHNICK)
    client.send(privmsg("NoSuchNick", "hello")).await;
    let err = client.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);

    // Connection should still be usable — PING/PONG still works
    client.send(ping_msg("still-alive")).await;
    let pong = client.recv_msg().await;
    assert_command(&pong, Command::Pong);
    assert_param_contains(&pong, 0, "still-alive");
}

#[tokio::test]
async fn multiple_errors_do_not_disconnect_client() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("ErrUser", "erruser").await;

    // Generate multiple errors
    for i in 0..5 {
        client
            .send(privmsg(&format!("Ghost{i}"), "hello"))
            .await;
        let err = client.recv_msg().await;
        assert_numeric(&err, ERR_NOSUCHNICK);
    }

    // Connection should still work
    client.send(ping_msg("still-connected")).await;
    let pong = client.recv_msg().await;
    assert_command(&pong, Command::Pong);
    assert_param_contains(&pong, 0, "still-connected");
}

// ===========================================================================
// 10. Full Flow Scenarios
// ===========================================================================

#[tokio::test]
async fn full_session_lifecycle() {
    let server = TestServer::start().await;

    // 1. Connect and register
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("FlowAlice", "flowalice").await;
    assert_eq!(server.users.connection_count(), 1);

    // 2. Join a channel
    alice.send(join_msg("#lifecycle")).await;
    let join_echo = alice.recv_msg().await;
    assert_command(&join_echo, Command::Join);
    alice.drain(3).await; // topic + names + endnames

    // 3. Second user connects and joins
    let mut bob = TestClient::connect(server.addr).await;
    bob.register("FlowBob", "flowbob").await;
    bob.send(join_msg("#lifecycle")).await;
    bob.drain(4).await;
    alice.recv_msg().await; // Alice sees Bob JOIN

    // 4. Exchange messages
    alice
        .send(privmsg("#lifecycle", "hello from alice"))
        .await;
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello from alice");

    bob.send(privmsg("#lifecycle", "hello from bob")).await;
    let msg = alice.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello from bob");

    // 5. Direct message between users
    bob.send(privmsg("FlowAlice", "hi alice")).await;
    let dm = alice.recv_msg().await;
    assert_command(&dm, Command::Privmsg);
    assert_param_contains(&dm, 1, "hi alice");

    // 6. PING/PONG
    alice.send(ping_msg("lifecycle-check")).await;
    let pong = alice.recv_msg().await;
    assert_command(&pong, Command::Pong);

    // 7. QUIT — Bob quits, Alice gets notified via channel membership
    bob.send(quit_msg("goodbye")).await;
    let quit_notif = alice.recv_msg().await;
    assert_command(&quit_notif, Command::Quit);
    assert_param_contains(&quit_notif, 0, "goodbye");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(server.users.connection_count(), 1);
}

#[tokio::test]
async fn concurrent_registrations() {
    let server = TestServer::start().await;

    // Connect multiple clients simultaneously
    let mut clients = Vec::new();
    for i in 0..5 {
        let mut client = TestClient::connect(server.addr).await;
        client
            .register(&format!("User{i}"), &format!("user{i}"))
            .await;
        clients.push(client);
    }

    assert_eq!(server.users.connection_count(), 5);

    // Each client can still communicate
    for (i, client) in clients.iter_mut().enumerate() {
        client.send(ping_msg(&format!("concurrent-{i}"))).await;
        let pong = client.recv_msg().await;
        assert_command(&pong, Command::Pong);
        assert_param_contains(&pong, 0, &format!("concurrent-{i}"));
    }
}

#[tokio::test]
async fn list_after_multiple_joins_and_parts() {
    let server = TestServer::start().await;

    let mut alice = TestClient::connect(server.addr).await;
    alice.register("ListAlice", "listalice").await;

    // Create channels
    alice.send(join_msg("#list-chan1")).await;
    alice.drain(4).await;
    alice.send(join_msg("#list-chan2")).await;
    alice.drain(4).await;
    alice.send(join_msg("#list-chan3")).await;
    alice.drain(4).await;

    // Part from one
    alice
        .send(pirc_integration_tests::common::part_msg("#list-chan2"))
        .await;
    alice.recv_msg().await; // PART echo

    // Give server time to clean up empty channel
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // LIST should show only the channels Alice is still in
    let mut lister = TestClient::connect(server.addr).await;
    lister.register("ListCheck", "listcheck").await;

    lister
        .send(pirc_protocol::Message::new(Command::List, vec![]))
        .await;

    let mut channels = Vec::new();
    loop {
        let msg = lister.recv_msg().await;
        if msg.numeric_code() == Some(RPL_LIST) {
            channels.push(msg.params[1].clone());
        } else if msg.numeric_code() == Some(RPL_LISTEND) {
            break;
        }
    }

    channels.sort();
    assert_eq!(channels, vec!["#list-chan1", "#list-chan3"]);
}

#[tokio::test]
async fn privmsg_after_nick_change_and_rejoin() {
    let server = TestServer::start().await;

    let mut alice = TestClient::connect(server.addr).await;
    alice.register("OrigNick", "orig").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("BobFlow", "bobflow").await;

    // Both join a channel
    alice.send(join_msg("#rename-flow")).await;
    alice.drain(4).await;
    bob.send(join_msg("#rename-flow")).await;
    bob.drain(4).await;
    alice.recv_msg().await; // Drain Bob's join notification

    // Alice changes nick
    alice.send(nick_msg("RenamedNick")).await;
    alice.recv_msg().await; // NICK confirmation

    // Alice sends a channel message — Bob should see it from RenamedNick
    alice.send(privmsg("#rename-flow", "from renamed")).await;
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "from renamed");
    let prefix_str = format!("{}", msg.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("RenamedNick"),
        "expected new nick in prefix, got {prefix_str}"
    );
}
