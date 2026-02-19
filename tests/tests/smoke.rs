//! Smoke test: start a server, connect a client, register, exchange messages.

use pirc_integration_tests::common::{
    assert_command, assert_numeric, assert_param_contains, join_msg, nick_msg, ping_msg, privmsg,
    TestClient, TestServer,
};
use pirc_protocol::numeric::{ERR_NOMOTD, RPL_CREATED, RPL_WELCOME, RPL_YOURHOST};
use pirc_protocol::Command;

#[tokio::test]
async fn server_starts_and_accepts_connection() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    // Send NICK + USER manually (don't use register() so we can inspect each reply)
    client.send(nick_msg("SmokeUser")).await;
    client
        .send(pirc_integration_tests::common::user_msg(
            "smokeuser",
            "Smoke Test User",
        ))
        .await;

    let welcome = client.recv_msg().await;
    assert_numeric(&welcome, RPL_WELCOME);
    assert_param_contains(&welcome, 0, "SmokeUser");

    let yourhost = client.recv_msg().await;
    assert_numeric(&yourhost, RPL_YOURHOST);

    let created = client.recv_msg().await;
    assert_numeric(&created, RPL_CREATED);

    let nomotd = client.recv_msg().await;
    assert_numeric(&nomotd, ERR_NOMOTD);

    client.shutdown().await;
}

#[tokio::test]
async fn register_helper_drains_welcome_burst() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;

    client.register("RegHelper", "reghelper").await;

    // After register(), the welcome burst is already consumed.
    // Verify the server tracks the user.
    assert_eq!(server.users.connection_count(), 1);

    client.shutdown().await;
}

#[tokio::test]
async fn ping_pong() {
    let server = TestServer::start().await;
    let mut client = TestClient::connect(server.addr).await;
    client.register("PingUser", "pinguser").await;

    let reply = client.send_and_recv(ping_msg("test-token")).await;
    assert_command(&reply, Command::Pong);
    assert_param_contains(&reply, 0, "test-token");

    client.shutdown().await;
    drop(server);
}

#[tokio::test]
async fn privmsg_between_two_clients() {
    let server = TestServer::start().await;

    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Alice sends a private message to Bob
    alice.send(privmsg("Bob", "hello bob")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello bob");

    alice.shutdown().await;
    bob.shutdown().await;
}

#[tokio::test]
async fn join_channel_and_send_message() {
    let server = TestServer::start().await;

    let mut alice = TestClient::connect(server.addr).await;
    alice.register("ChanAlice", "chanalice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("ChanBob", "chanbob").await;

    // Alice joins #test: gets JOIN echo, RPL_NOTOPIC, RPL_NAMREPLY, RPL_ENDOFNAMES
    alice.send(join_msg("#test")).await;
    alice.drain(4).await;

    // Bob joins #test: gets JOIN echo, RPL_NOTOPIC, RPL_NAMREPLY, RPL_ENDOFNAMES
    bob.send(join_msg("#test")).await;
    bob.drain(4).await;

    // Alice gets notified of Bob's JOIN
    let bob_join = alice.recv_msg().await;
    assert_command(&bob_join, Command::Join);

    // Alice sends a PRIVMSG to #test — Bob should receive it
    alice.send(privmsg("#test", "hello channel")).await;

    let chan_msg = bob.recv_msg().await;
    assert_command(&chan_msg, Command::Privmsg);
    assert_param_contains(&chan_msg, 1, "hello channel");

    alice.shutdown().await;
    bob.shutdown().await;
}
