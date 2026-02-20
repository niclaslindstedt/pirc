//! Stress and load integration tests.
//!
//! All tests in this module are marked `#[ignore]` so they do not run during
//! normal CI (`cargo test`).  Run them explicitly with:
//!
//! ```bash
//! cargo test -p pirc-integration-tests --test stress_test -- --ignored
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use pirc_integration_tests::common::{
    connection_pair, join_msg, nick_msg, privmsg, quit_msg, TestClient, TestServer, JOIN_BURST_LEN,
};
use pirc_network::connection::AsyncTransport;
use pirc_network::{BackpressureController, Connection, ConnectionPool, WriteConfig};
use pirc_protocol::{Command, Message};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register a client with a unique nick/username derived from an index.
async fn register_indexed(client: &mut TestClient, idx: usize) {
    let nick = format!("user{idx}");
    client.register(&nick, &nick).await;
}

// ===========================================================================
// 1. Concurrent Connections
// ===========================================================================

/// Server handles 100 simultaneous client connections.
#[tokio::test]
#[ignore]
async fn concurrent_100_connections() {
    let server = TestServer::start().await;

    let mut clients = Vec::with_capacity(100);
    for i in 0..100 {
        let mut client = TestClient::connect(server.addr).await;
        register_indexed(&mut client, i).await;
        clients.push(client);
    }

    // Server tracks all connections.
    assert_eq!(server.users.connection_count(), 100);

    // Each client can send a PING and receive a PONG.
    for (i, client) in clients.iter_mut().enumerate() {
        let token = format!("ping-{i}");
        let ping = Message::new(Command::Ping, vec![token.clone()]);
        client.send(ping).await;
        let pong = client.recv_msg().await;
        assert_eq!(pong.command, Command::Pong);
        assert_eq!(pong.params[0], token);
    }

    // Clean teardown.
    for client in &mut clients {
        client.send(quit_msg("bye")).await;
    }
    // Allow the server to process QUIT messages.
    tokio::time::sleep(Duration::from_millis(200)).await;

    drop(clients);
    drop(server);
}

/// All clients can register and communicate — no connection leaks.
#[tokio::test]
#[ignore]
async fn concurrent_connections_no_leaks() {
    let server = TestServer::start().await;

    let mut clients = Vec::with_capacity(50);
    for i in 0..50 {
        let mut client = TestClient::connect(server.addr).await;
        register_indexed(&mut client, i).await;
        clients.push(client);
    }

    assert_eq!(server.users.connection_count(), 50);

    // Each client sends QUIT.
    for client in &mut clients {
        client.send(quit_msg("done")).await;
    }

    // Allow the server to process.
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(clients);

    // After all clients disconnect, registry should be empty.
    assert_eq!(
        server.users.connection_count(),
        0,
        "all users should be unregistered after QUIT"
    );
}

/// Clean connection teardown for all clients concurrently.
#[tokio::test]
#[ignore]
async fn concurrent_teardown_is_clean() {
    let server = TestServer::start().await;

    let mut clients = Vec::with_capacity(30);
    for i in 0..30 {
        let mut client = TestClient::connect(server.addr).await;
        register_indexed(&mut client, i).await;
        clients.push(client);
    }

    // Shut down all clients concurrently.
    let mut handles = Vec::new();
    for mut client in clients {
        handles.push(tokio::spawn(async move {
            client.shutdown().await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(300)).await;
    // Server should not panic and should have cleaned up all connections.
    assert_eq!(
        server.users.connection_count(),
        0,
        "all connections should be cleaned up after teardown"
    );
}

// ===========================================================================
// 2. Message Throughput
// ===========================================================================

/// Single client sends 10,000 messages in rapid succession.
#[tokio::test]
#[ignore]
async fn single_client_10k_messages() {
    let server = TestServer::start().await;

    let mut sender = TestClient::connect(server.addr).await;
    sender.register("Sender", "sender").await;

    let mut receiver = TestClient::connect(server.addr).await;
    receiver.register("Receiver", "receiver").await;

    let count = 10_000;
    let start = Instant::now();

    for i in 0..count {
        sender
            .send(privmsg("Receiver", &format!("msg-{i}")))
            .await;
    }

    // Receive all messages.
    for i in 0..count {
        let msg = receiver.recv_msg().await;
        assert_eq!(msg.command, Command::Privmsg);
        assert_eq!(msg.params[1], format!("msg-{i}"));
    }

    let elapsed = start.elapsed();
    // Sanity: should complete in a reasonable time (< 30s for 10k messages).
    assert!(
        elapsed < Duration::from_secs(30),
        "10k messages took too long: {elapsed:?}"
    );
}

/// 50 clients each send 100 messages simultaneously.
#[tokio::test]
#[ignore]
async fn fifty_clients_concurrent_messaging() {
    let server = TestServer::start().await;

    let mut receiver = TestClient::connect(server.addr).await;
    receiver.register("Target", "target").await;

    let client_count = 50;
    let msgs_per_client = 100;

    let mut handles = Vec::new();
    for i in 0..client_count {
        let addr = server.addr;
        handles.push(tokio::spawn(async move {
            let mut client = TestClient::connect(addr).await;
            let nick = format!("sender{i}");
            client.register(&nick, &nick).await;

            for j in 0..msgs_per_client {
                client
                    .send(privmsg("Target", &format!("c{i}-m{j}")))
                    .await;
            }

            client.send(quit_msg("done")).await;
        }));
    }

    // Wait for all senders to finish.
    for h in handles {
        h.await.unwrap();
    }

    // Receive all messages sent to Target.
    let expected_total = client_count * msgs_per_client;
    let mut received = 0;
    let deadline = Instant::now() + Duration::from_secs(30);

    while received < expected_total {
        if Instant::now() > deadline {
            panic!(
                "timeout: only received {received}/{expected_total} messages"
            );
        }
        if let Some(msg) = receiver.try_recv_msg().await {
            if msg.command == Command::Privmsg {
                received += 1;
            }
        }
    }

    assert_eq!(received, expected_total);
}

/// Channel with many members: message from one reaches all others.
#[tokio::test]
#[ignore]
async fn channel_broadcast_to_many_members() {
    let server = TestServer::start().await;
    let member_count = 30;
    let channel = "#stress";

    let mut clients = Vec::with_capacity(member_count);
    for i in 0..member_count {
        let mut client = TestClient::connect(server.addr).await;
        register_indexed(&mut client, i).await;

        // Join channel.
        client.send(join_msg(channel)).await;
        // Drain own join burst.
        client.drain(JOIN_BURST_LEN).await;

        // Drain JOIN notifications from previously-joined members.
        // Each new joiner triggers a JOIN notification to all existing members.
        // The joining client doesn't receive those, but existing clients do.
        // We'll drain those after all join.
        clients.push(client);
    }

    // Drain JOIN notifications that accumulated on earlier clients.
    // Client i received (member_count - i - 1) JOIN notifications from later joiners.
    for i in 0..member_count {
        let notifications = member_count - i - 1;
        for _ in 0..notifications {
            clients[i].recv_msg().await;
        }
    }

    // Client 0 sends a message to the channel.
    clients[0]
        .send(privmsg(channel, "hello everyone"))
        .await;

    // All other clients should receive it.
    for client in clients.iter_mut().skip(1) {
        let msg = client.recv_msg().await;
        assert_eq!(msg.command, Command::Privmsg);
        assert!(msg.params[1].contains("hello everyone"));
    }
}

/// Verify message ordering under load on a single connection.
#[tokio::test]
#[ignore]
async fn message_ordering_under_load() {
    let server = TestServer::start().await;

    let mut sender = TestClient::connect(server.addr).await;
    sender.register("OrderSend", "ordersend").await;

    let mut receiver = TestClient::connect(server.addr).await;
    receiver.register("OrderRecv", "orderrecv").await;

    let count = 1_000;
    for i in 0..count {
        sender
            .send(privmsg("OrderRecv", &format!("seq-{i}")))
            .await;
    }

    for i in 0..count {
        let msg = receiver.recv_msg().await;
        assert_eq!(
            msg.params[1],
            format!("seq-{i}"),
            "message ordering violated at index {i}"
        );
    }
}

// ===========================================================================
// 3. Resource Limits
// ===========================================================================

/// Connection pool does not leak under rapid connect/disconnect cycles.
#[tokio::test]
#[ignore]
async fn connection_pool_no_leak_on_churn() {
    let pool = ConnectionPool::new(10);

    for round in 0..50 {
        let (conn, _peer) = connection_pair().await;
        let sid = pirc_common::ServerId::new((round % 5) + 1);

        pool.add(sid, conn).await.unwrap();
    }

    // Pool should have at most 5 entries (one per server id).
    assert!(
        pool.len().await <= 5,
        "pool leaked: {} entries",
        pool.len().await
    );

    pool.shutdown_all().await.unwrap();
    assert!(pool.is_empty().await);
}

/// Backpressure activates when buffer exceeds high-water mark.
#[tokio::test]
#[ignore]
async fn backpressure_activates_under_load() {
    let cfg = WriteConfig {
        high_water_mark: 100,
        low_water_mark: 50,
    };
    let mut ctrl = BackpressureController::new(cfg);

    // Simulate rapid buffering.
    for _ in 0..20 {
        ctrl.record_buffered(10);
    }

    // Should be backpressured (200 bytes buffered > 100 high-water).
    assert!(
        !ctrl.is_write_ready(),
        "backpressure should be active after exceeding high-water mark"
    );

    // Flush most of it.
    ctrl.record_flushed(160);

    // 40 remaining — below low-water mark of 50.
    assert!(
        ctrl.is_write_ready(),
        "backpressure should release below low-water mark"
    );
}

/// Server handles clients that connect but never send (idle connections).
#[tokio::test]
#[ignore]
async fn idle_connections_do_not_block_server() {
    let server = TestServer::start().await;

    // Open 20 idle connections (no registration).
    let mut idle_conns = Vec::new();
    for _ in 0..20 {
        let stream = TcpStream::connect(server.addr).await.unwrap();
        idle_conns.push(stream);
    }

    // An active client should still be able to connect, register, and communicate.
    let mut active = TestClient::connect(server.addr).await;
    active.register("ActiveUser", "activeuser").await;

    let ping = Message::new(Command::Ping, vec!["alive".to_owned()]);
    active.send(ping).await;
    let pong = active.recv_msg().await;
    assert_eq!(pong.command, Command::Pong);
    assert_eq!(pong.params[0], "alive");

    active.shutdown().await;
    drop(idle_conns);
}

/// Rapid connect/disconnect cycles don't crash the server.
#[tokio::test]
#[ignore]
async fn rapid_connect_disconnect_cycles() {
    let server = TestServer::start().await;

    for i in 0..100 {
        let mut client = TestClient::connect(server.addr).await;
        let nick = format!("churn{i}");
        client.register(&nick, &nick).await;
        client.send(quit_msg("churn")).await;
        // Don't wait for response — just move on.
    }

    // Give the server time to process.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Server should still be functional.
    let mut check = TestClient::connect(server.addr).await;
    check.register("PostChurn", "postchurn").await;
    let ping = Message::new(Command::Ping, vec!["still-alive".to_owned()]);
    check.send(ping).await;
    let pong = check.recv_msg().await;
    assert_eq!(pong.command, Command::Pong);
    assert_eq!(pong.params[0], "still-alive");
}

// ===========================================================================
// 4. Protocol Stress
// ===========================================================================

/// Send maximum-length messages (510 bytes of content + CRLF = 512) at rate.
#[tokio::test]
#[ignore]
async fn max_length_messages() {
    let server = TestServer::start().await;

    let mut sender = TestClient::connect(server.addr).await;
    sender.register("MaxSend", "maxsend").await;

    let mut receiver = TestClient::connect(server.addr).await;
    receiver.register("MaxRecv", "maxrecv").await;

    // IRC max message is 512 bytes including CRLF. The text portion is
    // whatever fits after "PRIVMSG MaxRecv :".
    let long_text = "A".repeat(400);

    for _ in 0..100 {
        sender.send(privmsg("MaxRecv", &long_text)).await;
    }

    for _ in 0..100 {
        let msg = receiver.recv_msg().await;
        assert_eq!(msg.command, Command::Privmsg);
        assert!(msg.params[1].len() >= 400);
    }
}

/// Send malformed messages mixed with valid ones.
#[tokio::test]
#[ignore]
async fn malformed_messages_mixed_with_valid() {
    let server = TestServer::start().await;

    let mut client = TestClient::connect(server.addr).await;
    client.register("MalUser", "maluser").await;

    let mut other = TestClient::connect(server.addr).await;
    other.register("Other", "other").await;

    // Send a valid message.
    client.send(privmsg("Other", "valid1")).await;
    let msg = other.recv_msg().await;
    assert_eq!(msg.params[1], "valid1");

    // Send a numeric message (unusual for a client to send — server should handle gracefully).
    let odd = Message::new(Command::Numeric(999), vec!["param".to_owned()]);
    client.send(odd).await;

    // Send another valid message.
    client.send(privmsg("Other", "valid2")).await;

    // The server may send an error reply for the unknown command, but should
    // still deliver the valid message. Drain any error replies.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found_valid2 = false;
    while Instant::now() < deadline {
        match other.try_recv_msg().await {
            Some(m) if m.command == Command::Privmsg && m.params[1] == "valid2" => {
                found_valid2 = true;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }
    assert!(found_valid2, "server should still deliver valid messages after malformed ones");
}

/// Rapidly connect and disconnect (connection churn) without crashing.
#[tokio::test]
#[ignore]
async fn connection_churn_stress() {
    let server = TestServer::start().await;

    let mut handles = Vec::new();
    for i in 0..200 {
        let addr = server.addr;
        handles.push(tokio::spawn(async move {
            let stream = TcpStream::connect(addr).await;
            if let Ok(stream) = stream {
                let mut conn = Connection::new(stream).unwrap();
                let nick_m = nick_msg(&format!("churnbot{i}"));
                let _ = conn.send(nick_m).await;
                // Immediately drop — server must handle abrupt disconnects.
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Give the server time to clean up.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Server should still be functional.
    let mut check = TestClient::connect(server.addr).await;
    check.register("AfterChurn", "afterchurn").await;
    let ping = Message::new(Command::Ping, vec!["ok".to_owned()]);
    check.send(ping).await;
    let pong = check.recv_msg().await;
    assert_eq!(pong.command, Command::Pong);
    assert_eq!(pong.params[0], "ok");
}

/// Multiple channels under concurrent load.
#[tokio::test]
#[ignore]
async fn multi_channel_concurrent_load() {
    let server = TestServer::start().await;
    let channel_count = 5;
    let clients_per_channel = 10;

    let mut all_clients = Vec::new();

    // Create clients and have them join different channels.
    for ch in 0..channel_count {
        let channel_name = format!("#stress{ch}");
        for c in 0..clients_per_channel {
            let idx = ch * clients_per_channel + c;
            let mut client = TestClient::connect(server.addr).await;
            register_indexed(&mut client, idx).await;
            client.send(join_msg(&channel_name)).await;
            client.drain(JOIN_BURST_LEN).await;
            all_clients.push((idx, channel_name.clone(), client));
        }
    }

    // Drain JOIN notifications for existing members in each channel.
    for i in 0..all_clients.len() {
        let pos_in_ch = i % clients_per_channel;
        let notifications = clients_per_channel - pos_in_ch - 1;
        for _ in 0..notifications {
            all_clients[i].2.recv_msg().await;
        }
    }

    // Each first client in a channel sends a message.
    for ch in 0..channel_count {
        let channel_name = format!("#stress{ch}");
        let idx = ch * clients_per_channel;
        all_clients[idx]
            .2
            .send(privmsg(&channel_name, &format!("hello-from-ch{ch}")))
            .await;
    }

    // Each non-first client in a channel should receive the message.
    for ch in 0..channel_count {
        for c in 1..clients_per_channel {
            let idx = ch * clients_per_channel + c;
            let msg = all_clients[idx].2.recv_msg().await;
            assert_eq!(msg.command, Command::Privmsg);
            assert!(msg.params[1].contains(&format!("hello-from-ch{ch}")));
        }
    }
}

/// Network-level connection survives a burst of many messages.
#[tokio::test]
#[ignore]
async fn raw_connection_burst() {
    let (mut client, mut server_side) = connection_pair().await;

    let count = 5_000;
    for i in 0..count {
        let msg = Message::new(Command::Ping, vec![format!("burst-{i}")]);
        client.send(msg).await.unwrap();
    }

    for i in 0..count {
        let received = server_side.recv().await.unwrap().unwrap();
        assert_eq!(received.params[0], format!("burst-{i}"));
    }
}

/// Concurrent senders on a connection pool.
#[tokio::test]
#[ignore]
async fn connection_pool_concurrent_sends() {
    let pool = Arc::new(ConnectionPool::new(5));

    let mut peers = Vec::new();
    for i in 1..=5 {
        let (conn, peer) = connection_pair().await;
        pool.add(pirc_common::ServerId::new(i), conn)
            .await
            .unwrap();
        peers.push(peer);
    }

    // Broadcast 100 messages through the pool.
    for i in 0..100 {
        let msg = Message::new(Command::Ping, vec![format!("pool-{i}")]);
        let results = pool.broadcast(&msg).await;
        for (_, result) in &results {
            assert!(result.is_ok());
        }
    }

    // Each peer should have received 100 messages.
    for peer in &mut peers {
        for i in 0..100 {
            let received = peer.recv().await.unwrap().unwrap();
            assert_eq!(received.params[0], format!("pool-{i}"));
        }
    }

    pool.shutdown_all().await.unwrap();
}
