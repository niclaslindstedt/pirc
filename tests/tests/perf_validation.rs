//! End-to-end NFR performance validation tests.
//!
//! All tests in this module are marked `#[ignore]` so they do not run during
//! normal CI (`cargo test`).  Run them explicitly with:
//!
//! ```bash
//! make perf-test
//! ```
//!
//! Each test validates a specific non-functional requirement (NFR) by
//! exercising real system components, measuring wall-clock time, and asserting
//! against the contractual threshold.

use std::time::{Duration, Instant};

use pirc_integration_tests::cluster_harness::RaftTestCluster;
use pirc_integration_tests::common::{privmsg, TestClient, TestServer};
use pirc_p2p::ice::{CandidateType, IceCandidate};
use pirc_p2p::session::{P2pSession, SessionState};
use pirc_server::raft::RaftState;

// ===========================================================================
// Helper: spawn a mock STUN server (copied from p2p_connectivity tests)
// ===========================================================================

async fn spawn_mock_stun_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use pirc_p2p::stun::{StunAttribute, StunMessage};
    use tokio::net::UdpSocket;

    let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server_sock.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            let result = server_sock.recv_from(&mut buf).await;
            match result {
                Ok((len, src)) => {
                    if let Ok(request) = StunMessage::from_bytes(&buf[..len]) {
                        let response = StunMessage {
                            msg_type: 0x0101, // Binding Response
                            transaction_id: request.transaction_id,
                            attributes: vec![StunAttribute::XorMappedAddress(src)],
                        };
                        let _ = server_sock.send_to(&response.to_bytes(), src).await;
                    }
                }
                Err(_) => break,
            }
        }
    });

    (server_addr, handle)
}

fn default_gatherer_config() -> pirc_p2p::ice::GathererConfig {
    pirc_p2p::ice::GathererConfig {
        stun_server: None,
        turn_server: None,
        turn_username: None,
        turn_password: None,
    }
}

// ===========================================================================
// REQ-049: Client startup time < 500ms
// ===========================================================================

/// Validates that a test server starts and accepts a client connection within
/// 500ms.
///
/// This measures server bind + accept + client connect + registration, which
/// exercises the real startup path. The client binary includes a TUI which
/// cannot be tested headlessly, so we validate the server-side startup and
/// client connection establishment instead.
#[tokio::test]
#[ignore]
async fn req049_client_startup_under_500ms() {
    // Warm up: ensure the runtime is initialized before measuring.
    let _warmup = TestServer::start().await;
    drop(_warmup);

    const ITERATIONS: usize = 5;
    let mut times = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let start = Instant::now();

        // Start server (bind, listen).
        let server = TestServer::start().await;

        // Connect and register a client (full startup path).
        let mut client = TestClient::connect(server.addr).await;
        client.register("PerfUser", "perfuser").await;

        let elapsed = start.elapsed();
        times.push(elapsed);

        client.shutdown().await;
        drop(server);
    }

    let avg = times.iter().sum::<Duration>() / ITERATIONS as u32;
    let max = times.iter().max().unwrap();

    eprintln!("REQ-049 Client startup times:");
    for (i, t) in times.iter().enumerate() {
        eprintln!("  iteration {}: {:?}", i + 1, t);
    }
    eprintln!("  average: {avg:?}");
    eprintln!("  max: {max:?}");

    assert!(
        *max < Duration::from_millis(500),
        "REQ-049 FAILED: client startup max time {max:?} exceeds 500ms threshold"
    );
}

// ===========================================================================
// REQ-050: Message delivery latency < 100ms in-cluster
// ===========================================================================

/// Validates that a PRIVMSG sent through the server is delivered to the
/// recipient in under 100ms.
///
/// Measures the round-trip time: sender sends PRIVMSG, receiver gets it.
/// Uses a single server (in-cluster = single node), which is the common
/// deployment for the current architecture.
#[tokio::test]
#[ignore]
async fn req050_message_delivery_under_100ms() {
    let server = TestServer::start().await;

    let mut sender = TestClient::connect(server.addr).await;
    sender.register("Sender", "sender").await;

    let mut receiver = TestClient::connect(server.addr).await;
    receiver.register("Receiver", "receiver").await;

    // Warm up the connection.
    sender.send(privmsg("Receiver", "warmup")).await;
    receiver.recv_msg().await;

    const ITERATIONS: usize = 100;
    let mut times = Vec::with_capacity(ITERATIONS);

    for i in 0..ITERATIONS {
        let msg_text = format!("perf-{i}");
        let start = Instant::now();
        sender.send(privmsg("Receiver", &msg_text)).await;
        let received = receiver.recv_msg().await;
        let elapsed = start.elapsed();
        times.push(elapsed);

        assert_eq!(received.params[1], msg_text);
    }

    times.sort();
    let avg = times.iter().sum::<Duration>() / ITERATIONS as u32;
    let p50 = times[ITERATIONS / 2];
    let p99 = times[ITERATIONS * 99 / 100];
    let max = *times.last().unwrap();

    eprintln!("REQ-050 Message delivery latency ({ITERATIONS} messages):");
    eprintln!("  avg: {avg:?}");
    eprintln!("  p50: {p50:?}");
    eprintln!("  p99: {p99:?}");
    eprintln!("  max: {max:?}");

    assert!(
        p99 < Duration::from_millis(100),
        "REQ-050 FAILED: p99 message delivery latency {p99:?} exceeds 100ms threshold"
    );
}

// ===========================================================================
// REQ-052: P2P connection established < 5s
// ===========================================================================

/// Validates that a P2P session can be established (offer, answer,
/// connectivity checks) within 5 seconds using loopback networking.
///
/// Uses the real P2P session state machine with a mock STUN responder,
/// measuring the time from initiating the session to achieving Connected state.
#[tokio::test]
#[ignore]
async fn req052_p2p_connection_under_5s() {
    const ITERATIONS: usize = 3;
    let mut times = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let (responder_addr, _handle) = spawn_mock_stun_server().await;

        let start = Instant::now();

        // Initiator side.
        let mut session_a = P2pSession::new("B".into(), default_gatherer_config());
        session_a.initiate().await.unwrap();

        // Responder side: receives the offer (simulated by providing a known
        // reachable STUN responder as the remote candidate).
        let remote_for_b = vec![IceCandidate::new(
            CandidateType::Host,
            responder_addr,
            65535,
            "host1".into(),
            1,
        )];
        let mut session_b = P2pSession::new("A".into(), default_gatherer_config());
        session_b.respond(remote_for_b).await.unwrap();

        // Cross-wire: A receives B's candidates.
        let (responder_b_addr, _handle_b) = spawn_mock_stun_server().await;
        let remote_for_a = vec![IceCandidate::new(
            CandidateType::Host,
            responder_b_addr,
            65535,
            "host1".into(),
            1,
        )];
        session_a.set_remote_candidates(remote_for_a);

        // Run connectivity checks on both sides.
        session_a.run_checks().await.unwrap();
        session_b.run_checks().await.unwrap();

        let elapsed = start.elapsed();
        times.push(elapsed);

        assert_eq!(session_a.state(), SessionState::Connected);
        assert_eq!(session_b.state(), SessionState::Connected);
    }

    let avg = times.iter().sum::<Duration>() / ITERATIONS as u32;
    let max = *times.iter().max().unwrap();

    eprintln!("REQ-052 P2P connection establishment times:");
    for (i, t) in times.iter().enumerate() {
        eprintln!("  iteration {}: {:?}", i + 1, t);
    }
    eprintln!("  average: {avg:?}");
    eprintln!("  max: {max:?}");

    assert!(
        max < Duration::from_secs(5),
        "REQ-052 FAILED: P2P connection time {max:?} exceeds 5s threshold"
    );
}

// ===========================================================================
// REQ-053: Raft leader election < 2s
// ===========================================================================

/// Validates that a Raft leader election completes within 2 seconds after
/// the leader is killed.
///
/// Uses a real 3-node Raft cluster with in-memory storage and message
/// routing, measures the time from leader kill to new leader election.
#[tokio::test]
#[ignore]
async fn req053_raft_election_under_2s() {
    const ITERATIONS: usize = 3;
    let mut times = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let cluster = RaftTestCluster::start(&[1, 2, 3]).await;

        // Wait for initial leader.
        let first_leader = cluster
            .wait_for_leader(Duration::from_secs(2))
            .await
            .expect("initial leader should be elected");

        // Kill the leader and start timing.
        let start = Instant::now();
        cluster.kill_node(first_leader);

        // Wait for a new leader among remaining nodes.
        let remaining: Vec<u64> = [1, 2, 3]
            .iter()
            .copied()
            .filter(|&id| id != first_leader)
            .collect();

        let mut new_leader = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            for &id in &remaining {
                if cluster.handle(id).state() == RaftState::Leader {
                    new_leader = Some(id);
                    break;
                }
            }
            if new_leader.is_some() || Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let elapsed = start.elapsed();
        times.push(elapsed);

        assert!(
            new_leader.is_some(),
            "new leader should be elected (elapsed: {elapsed:?})"
        );
        assert_ne!(new_leader.unwrap(), first_leader);

        cluster.shutdown_all();
    }

    let avg = times.iter().sum::<Duration>() / ITERATIONS as u32;
    let max = *times.iter().max().unwrap();

    eprintln!("REQ-053 Raft leader election times:");
    for (i, t) in times.iter().enumerate() {
        eprintln!("  iteration {}: {:?}", i + 1, t);
    }
    eprintln!("  average: {avg:?}");
    eprintln!("  max: {max:?}");

    assert!(
        max < Duration::from_secs(2),
        "REQ-053 FAILED: Raft election time {max:?} exceeds 2s threshold"
    );
}

// ===========================================================================
// REQ-054: User migration < 5s
// ===========================================================================

/// Validates that after a server shutdown, a user can reconnect to a new
/// server and resume messaging within 5 seconds.
///
/// Since pirc does not yet have automatic server failover (users reconnect
/// manually), this test measures the time to: shut down the original server,
/// start a new server, connect, register, and successfully exchange a
/// message — simulating a user migration scenario.
#[tokio::test]
#[ignore]
async fn req054_user_migration_under_5s() {
    const ITERATIONS: usize = 3;
    let mut times = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        // Set up initial server with two users exchanging a message.
        let server1 = TestServer::start().await;
        let mut alice = TestClient::connect(server1.addr).await;
        alice.register("Alice", "alice").await;
        let mut bob = TestClient::connect(server1.addr).await;
        bob.register("Bob", "bob").await;

        // Verify initial communication works.
        alice.send(privmsg("Bob", "hello")).await;
        bob.recv_msg().await;

        // Simulate server failure and start timing the migration.
        let start = Instant::now();
        drop(alice);
        drop(bob);
        drop(server1);

        // Start a new server (simulates failover to a replacement).
        let server2 = TestServer::start().await;

        // Users reconnect and re-register.
        let mut alice2 = TestClient::connect(server2.addr).await;
        alice2.register("Alice", "alice").await;
        let mut bob2 = TestClient::connect(server2.addr).await;
        bob2.register("Bob", "bob").await;

        // Verify communication on the new server.
        alice2.send(privmsg("Bob", "migrated")).await;
        let msg = bob2.recv_msg().await;
        let elapsed = start.elapsed();

        assert_eq!(msg.params[1], "migrated");
        times.push(elapsed);

        alice2.shutdown().await;
        bob2.shutdown().await;
        drop(server2);
    }

    let avg = times.iter().sum::<Duration>() / ITERATIONS as u32;
    let max = *times.iter().max().unwrap();

    eprintln!("REQ-054 User migration times:");
    for (i, t) in times.iter().enumerate() {
        eprintln!("  iteration {}: {:?}", i + 1, t);
    }
    eprintln!("  average: {avg:?}");
    eprintln!("  max: {max:?}");

    assert!(
        max < Duration::from_secs(5),
        "REQ-054 FAILED: user migration time {max:?} exceeds 5s threshold"
    );
}
