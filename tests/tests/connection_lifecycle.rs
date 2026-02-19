//! Connection lifecycle and backpressure integration tests.
//!
//! Validates pirc-network's connection management: lifecycle events,
//! shutdown coordination, reconnection with backoff, connection pooling,
//! and backpressure handling — all using real TCP connections.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_common::ServerId;
use pirc_network::connection::AsyncTransport;
use pirc_network::{
    BackpressureController, BoundedChannel, Connection, ConnectionPool, Connector, Listener,
    ReadLimiter, ReconnectPolicy, ReconnectingConnector, ShutdownSignal, WriteConfig,
};
use pirc_protocol::{Command, Message};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Bind a listener on a random loopback port.
async fn loopback_listener() -> Listener {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    Listener::bind(addr).await.unwrap()
}

/// Create a connected pair of [`Connection`] endpoints via TCP loopback.
async fn connection_pair() -> (Connection, Connection) {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();
    let (client_result, server_result) =
        tokio::join!(TcpStream::connect(addr), listener.accept());
    let client = Connection::new(client_result.unwrap()).unwrap();
    let server = server_result.unwrap().0;
    (client, server)
}

fn ping_msg(token: &str) -> Message {
    Message::new(Command::Ping, vec![token.to_owned()])
}

fn pong_msg(token: &str) -> Message {
    Message::new(Command::Pong, vec![token.to_owned()])
}

fn quit_msg(reason: &str) -> Message {
    Message::new(Command::Quit, vec![reason.to_owned()])
}

// ---------------------------------------------------------------------------
// 1. Connection lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connect_and_both_sides_have_valid_info() {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();

    let client_stream = TcpStream::connect(addr).await.unwrap();
    let client_local = client_stream.local_addr().unwrap();
    let client = Connection::new(client_stream).unwrap();

    let (server, peer_addr) = listener.accept().await.unwrap();

    // Client sees server's listening port as peer
    assert_eq!(client.info().peer_addr.port(), addr.port());
    assert!(client.info().peer_addr.ip().is_loopback());

    // Server sees the client's ephemeral port
    assert_eq!(peer_addr, client_local);
    assert!(server.info().peer_addr.ip().is_loopback());

    // Both have unique connection IDs
    assert_ne!(client.info().id, server.info().id);

    // Both start with zero byte counters
    assert_eq!(client.info().bytes_sent, 0);
    assert_eq!(server.info().bytes_received, 0);
}

#[tokio::test]
async fn client_graceful_disconnect_server_detects_eof() {
    let (mut client, mut server) = connection_pair().await;

    // Send a message, then shut down gracefully
    let msg = quit_msg("goodbye");
    client.send(msg.clone()).await.unwrap();
    client.shutdown().await.unwrap();

    // Server receives the message
    let received = server.recv().await.unwrap().unwrap();
    assert_eq!(received, msg);

    // Server detects EOF
    let eof = server.recv().await.unwrap();
    assert!(eof.is_none());
}

#[tokio::test]
async fn server_drops_connection_client_detects_disconnect() {
    let (mut client, server) = connection_pair().await;

    // Drop the server side abruptly
    drop(server);

    // Client should detect the disconnect (EOF or error)
    let result = client.recv().await;
    match result {
        Ok(None) => {} // EOF
        Err(_) => {}   // I/O error — both acceptable
        Ok(Some(_)) => panic!("expected EOF or error after server drop"),
    }
}

#[tokio::test]
async fn multiple_clients_connect_simultaneously_unique_ids() {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();

    let n = 10;
    let mut clients = Vec::new();
    let mut servers = Vec::new();

    // Connect N clients
    for _ in 0..n {
        let stream = TcpStream::connect(addr).await.unwrap();
        let client = Connection::new(stream).unwrap();
        let (server, _) = listener.accept().await.unwrap();
        clients.push(client);
        servers.push(server);
    }

    // All connection IDs should be unique
    let mut ids: Vec<u64> = clients
        .iter()
        .chain(servers.iter())
        .map(|c| c.info().id)
        .collect();
    let len_before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), len_before, "all connection IDs must be unique");

    // Verify independent communication on each pair
    for (i, (client, server)) in clients.iter_mut().zip(servers.iter_mut()).enumerate() {
        let msg = ping_msg(&format!("client-{i}"));
        client.send(msg.clone()).await.unwrap();
        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);
    }
}

#[tokio::test]
async fn bidirectional_message_exchange() {
    let (mut client, mut server) = connection_pair().await;

    // Client -> Server
    let ping = ping_msg("check");
    client.send(ping.clone()).await.unwrap();
    let received = server.recv().await.unwrap().unwrap();
    assert_eq!(received, ping);

    // Server -> Client
    let pong = pong_msg("check");
    server.send(pong.clone()).await.unwrap();
    let received = client.recv().await.unwrap().unwrap();
    assert_eq!(received, pong);
}

#[tokio::test]
async fn bytes_counters_track_traffic() {
    let (mut client, mut server) = connection_pair().await;

    let msg = ping_msg("hello");
    let wire_len = (msg.to_string().len() + 2) as u64; // +2 for \r\n

    client.send(msg).await.unwrap();
    assert_eq!(client.info().bytes_sent, wire_len);

    let _ = server.recv().await.unwrap().unwrap();
    assert_eq!(server.info().bytes_received, wire_len);
}

// ---------------------------------------------------------------------------
// 2. Shutdown coordination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_controller_stops_accept_loop() {
    let listener = Arc::new(loopback_listener().await);
    let (controller, mut signal) = ShutdownSignal::new();

    let listener_clone = listener.clone();
    let handle = tokio::spawn(async move {
        let mut accepted = 0u32;
        loop {
            match listener_clone.accept_with_shutdown(&mut signal).await {
                Ok(Some(_)) => accepted += 1,
                Ok(None) | Err(_) => break,
            }
        }
        accepted
    });

    // Give the task time to start waiting
    tokio::time::sleep(Duration::from_millis(10)).await;

    controller.shutdown();
    let accepted = handle.await.unwrap();
    assert_eq!(accepted, 0);
}

#[tokio::test]
async fn shutdown_signal_propagates_to_recv_with_shutdown() {
    let (mut client, mut server) = connection_pair().await;
    let (controller, mut signal) = ShutdownSignal::new();

    // Send a message, then signal shutdown
    let msg = ping_msg("before-shutdown");
    client.send(msg.clone()).await.unwrap();

    // First recv should succeed (message already in buffer)
    let received = server.recv_with_shutdown(&mut signal).await.unwrap();
    assert_eq!(received, Some(msg));

    // Now signal shutdown while server waits
    controller.shutdown();
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        server.recv_with_shutdown(&mut signal),
    )
    .await
    .expect("should not timeout")
    .unwrap();
    assert!(result.is_none(), "should get None on shutdown");

    // Client should see EOF after server's shutdown-triggered close
    let eof = client.recv().await.unwrap();
    assert!(eof.is_none());
}

#[tokio::test]
async fn multiple_shutdown_signal_clones_all_receive() {
    let (controller, signal) = ShutdownSignal::new();

    let mut handles = Vec::new();
    for _ in 0..5 {
        let mut s = signal.clone();
        handles.push(tokio::spawn(async move {
            s.recv().await;
            assert!(s.is_shutdown());
        }));
    }
    drop(signal);

    tokio::time::sleep(Duration::from_millis(10)).await;
    controller.shutdown();

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn shutdown_flushes_pending_writes() {
    let (mut client, mut server) = connection_pair().await;

    // Server sends a message, then shutdown is triggered
    let msg = quit_msg("shutting down");
    server.send(msg.clone()).await.unwrap();
    server.shutdown().await.unwrap();

    // Client should still receive the flushed message
    let received = client.recv().await.unwrap().unwrap();
    assert_eq!(received, msg);

    // Then client sees EOF
    let eof = client.recv().await.unwrap();
    assert!(eof.is_none());
}

// ---------------------------------------------------------------------------
// 3. Reconnection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reconnecting_connector_succeeds_first_try() {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();

    let rc = ReconnectingConnector::new(Connector::new(), ReconnectPolicy::default());
    let conn = rc.connect_with_retry(addr).await.unwrap();
    assert_eq!(conn.info().peer_addr.port(), addr.port());
}

#[tokio::test]
async fn reconnecting_connector_retries_then_succeeds() {
    // Get an address, drop the listener, then restart it after a delay
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let restart_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        Listener::bind(addr).await.unwrap()
    });

    let policy = ReconnectPolicy {
        max_retries: Some(10),
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(200),
        backoff_factor: 1.5,
    };

    let rc = ReconnectingConnector::new(
        Connector::with_timeout(Duration::from_millis(200)),
        policy,
    );
    let conn = rc.connect_with_retry(addr).await.unwrap();
    assert_eq!(conn.info().peer_addr.port(), addr.port());

    let _listener = restart_handle.await.unwrap();
}

#[tokio::test]
async fn reconnecting_connector_exhausts_retries() {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let policy = ReconnectPolicy {
        max_retries: Some(2),
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(50),
        backoff_factor: 2.0,
    };

    let rc = ReconnectingConnector::new(
        Connector::with_timeout(Duration::from_millis(100)),
        policy,
    );
    let result = rc.connect_with_retry(addr).await;
    assert!(result.is_err(), "should fail after exhausting retries");
}

#[tokio::test]
async fn reconnecting_connector_exponential_backoff_timing() {
    // Verify that retries take progressively longer
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let policy = ReconnectPolicy {
        max_retries: Some(3),
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_secs(5),
        backoff_factor: 2.0,
    };

    let rc = ReconnectingConnector::new(
        Connector::with_timeout(Duration::from_millis(50)),
        policy,
    );

    let start = tokio::time::Instant::now();
    let _ = rc.connect_with_retry(addr).await;
    let elapsed = start.elapsed();

    // With 3 retries and initial_delay=50ms, backoff_factor=2.0:
    // attempt 0 fails → wait 50ms
    // attempt 1 fails → wait 100ms
    // attempt 2 fails → wait 200ms
    // attempt 3 fails → give up
    // Total wait ~350ms (+ connection attempt times)
    // Should be well above 300ms but below 5s
    assert!(
        elapsed > Duration::from_millis(300),
        "elapsed {:?} should be > 300ms due to backoff",
        elapsed
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "elapsed {:?} should be < 5s",
        elapsed
    );
}

#[tokio::test]
async fn connector_timeout_enforced() {
    // Use a non-routable address (TEST-NET-1, RFC 5737) to trigger timeout
    let addr: SocketAddr = "192.0.2.1:6667".parse().unwrap();
    let connector = Connector::with_timeout(Duration::from_millis(100));

    let start = tokio::time::Instant::now();
    let result = connector.connect(addr).await;
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(
        elapsed < Duration::from_secs(2),
        "should timeout quickly, took {:?}",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// 4. Connection pool
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pool_tracks_multiple_connections() {
    let pool = ConnectionPool::new(10);

    let (c1, _p1) = connection_pair().await;
    let (c2, _p2) = connection_pair().await;
    let (c3, _p3) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();
    pool.add(ServerId::new(3), c3).await.unwrap();

    assert_eq!(pool.len().await, 3);
    assert!(pool.contains(ServerId::new(1)).await);
    assert!(pool.contains(ServerId::new(2)).await);
    assert!(pool.contains(ServerId::new(3)).await);
    assert!(!pool.contains(ServerId::new(99)).await);

    let mut servers = pool.connected_servers().await;
    servers.sort();
    assert_eq!(
        servers,
        vec![ServerId::new(1), ServerId::new(2), ServerId::new(3)]
    );
}

#[tokio::test]
async fn pool_remove_and_get() {
    let pool = ConnectionPool::new(10);
    let (conn, _peer) = connection_pair().await;
    let conn_id = conn.info().id;
    let sid = ServerId::new(1);

    pool.add(sid, conn).await.unwrap();

    // Get returns a valid reference
    let got = pool.get(sid).await.unwrap();
    assert_eq!(got.info().id, conn_id);
    drop(got);

    // Remove returns the connection
    let removed = pool.remove(sid).await;
    assert!(removed.is_some());
    assert!(pool.is_empty().await);

    // Get after remove returns None
    assert!(pool.get(sid).await.is_none());
}

#[tokio::test]
async fn pool_broadcast_delivers_to_all() {
    let pool = ConnectionPool::new(10);

    let (c1, mut p1) = connection_pair().await;
    let (c2, mut p2) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();

    let msg = ping_msg("broadcast-test");
    let results = pool.broadcast(&msg).await;

    assert_eq!(results.len(), 2);
    for (_, result) in &results {
        assert!(result.is_ok());
    }

    // Both peers receive the message
    let r1 = p1.recv().await.unwrap().unwrap();
    let r2 = p2.recv().await.unwrap().unwrap();
    assert_eq!(r1, msg);
    assert_eq!(r2, msg);
}

#[tokio::test]
async fn pool_capacity_enforcement() {
    let pool = ConnectionPool::new(2);

    let (c1, _p1) = connection_pair().await;
    let (c2, _p2) = connection_pair().await;
    let (c3, _p3) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();

    // Third add should fail
    let result = pool.add(ServerId::new(3), c3).await;
    assert!(result.is_err());
    assert_eq!(pool.len().await, 2);
}

#[tokio::test]
async fn pool_handles_dropped_peer_gracefully() {
    let pool = ConnectionPool::new(10);

    let (c1, peer1) = connection_pair().await;
    let (c2, _p2) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();

    // Drop one peer — its connection is now broken
    drop(peer1);

    let msg = ping_msg("test");
    let results = pool.broadcast(&msg).await;

    // Both connections should get results (one may fail)
    assert_eq!(results.len(), 2);
    let ids: Vec<ServerId> = results.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&ServerId::new(1)));
    assert!(ids.contains(&ServerId::new(2)));
}

#[tokio::test]
async fn pool_shutdown_all_closes_connections() {
    let pool = ConnectionPool::new(10);

    let (c1, mut p1) = connection_pair().await;
    let (c2, mut p2) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();

    pool.shutdown_all().await.unwrap();

    assert!(pool.is_empty().await);

    // Peers see EOF
    assert!(p1.recv().await.unwrap().is_none());
    assert!(p2.recv().await.unwrap().is_none());
}

#[tokio::test]
async fn pool_shutdown_on_signal() {
    let pool = Arc::new(ConnectionPool::new(10));

    let (c1, mut p1) = connection_pair().await;
    let (c2, mut p2) = connection_pair().await;

    pool.add(ServerId::new(1), c1).await.unwrap();
    pool.add(ServerId::new(2), c2).await.unwrap();

    let (controller, signal) = ShutdownSignal::new();

    let pool_clone = pool.clone();
    let handle = tokio::spawn(async move { pool_clone.shutdown_on_signal(signal).await });

    tokio::time::sleep(Duration::from_millis(10)).await;
    controller.shutdown();

    handle.await.unwrap().unwrap();

    assert!(pool.is_empty().await);
    assert!(p1.recv().await.unwrap().is_none());
    assert!(p2.recv().await.unwrap().is_none());
}

#[tokio::test]
async fn pool_replace_existing_connection() {
    let pool = ConnectionPool::new(1);

    let (c1, _p1) = connection_pair().await;
    let c1_id = c1.info().id;
    pool.add(ServerId::new(1), c1).await.unwrap();

    // Replace with a new connection — should succeed despite max capacity of 1
    let (c2, _p2) = connection_pair().await;
    let c2_id = c2.info().id;
    pool.add(ServerId::new(1), c2).await.unwrap();

    assert_eq!(pool.len().await, 1);
    let got = pool.get(ServerId::new(1)).await.unwrap();
    assert_eq!(got.info().id, c2_id);
    assert_ne!(c1_id, c2_id);
}

// ---------------------------------------------------------------------------
// 5. Backpressure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bounded_channel_blocks_sender_when_full() {
    let (tx, mut rx) = BoundedChannel::channel::<i32>(1);

    tx.send(1).await.unwrap();

    // Spawn a sender that will block
    let tx_clone = tx.clone();
    let handle = tokio::spawn(async move {
        tx_clone.send(2).await.unwrap();
    });

    // Give the spawned task time to block
    tokio::task::yield_now().await;

    // Drain one to unblock
    assert_eq!(rx.recv().await, Some(1));

    // The blocked send should complete
    handle.await.unwrap();
    assert_eq!(rx.recv().await, Some(2));
}

#[tokio::test]
async fn bounded_channel_try_send_fails_when_full() {
    let (tx, _rx) = BoundedChannel::channel::<i32>(2);

    tx.try_send(1).unwrap();
    tx.try_send(2).unwrap();

    // Third should fail
    let result = tx.try_send(3);
    assert!(result.is_err(), "try_send should fail when channel is full");
}

#[tokio::test]
async fn bounded_channel_fifo_ordering() {
    let (tx, mut rx) = BoundedChannel::channel::<i32>(16);

    for i in 0..10 {
        tx.send(i).await.unwrap();
    }

    for i in 0..10 {
        assert_eq!(rx.recv().await, Some(i));
    }
}

#[tokio::test]
async fn bounded_channel_recv_returns_none_when_all_senders_dropped() {
    let (tx, mut rx) = BoundedChannel::channel::<i32>(8);
    tx.send(42).await.unwrap();
    drop(tx);

    // Should receive the buffered value first
    assert_eq!(rx.recv().await, Some(42));
    // Then None
    assert_eq!(rx.recv().await, None);
}

#[tokio::test]
async fn bounded_channel_send_errors_when_receiver_dropped() {
    let (tx, rx) = BoundedChannel::channel::<i32>(8);
    drop(rx);

    let result = tx.send(42).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn bounded_channel_multiple_senders() {
    let (tx, mut rx) = BoundedChannel::channel::<i32>(16);

    let mut handles = Vec::new();
    for i in 0..5 {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            tx.send(i).await.unwrap();
        }));
    }
    drop(tx);

    for handle in handles {
        handle.await.unwrap();
    }

    let mut received = Vec::new();
    while let Some(v) = rx.recv().await {
        received.push(v);
    }

    received.sort();
    assert_eq!(received, vec![0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn backpressure_controller_engage_release_cycle() {
    let cfg = WriteConfig {
        high_water_mark: 100,
        low_water_mark: 50,
    };
    let mut ctrl = BackpressureController::new(cfg);

    assert!(ctrl.is_write_ready());

    // Buffer below high water
    assert!(!ctrl.record_buffered(80));
    assert!(ctrl.is_write_ready());

    // Cross high water mark
    assert!(ctrl.record_buffered(20));
    assert!(!ctrl.is_write_ready());

    // Flush but stay above low water
    assert!(!ctrl.record_flushed(40));
    assert!(!ctrl.is_write_ready());

    // Flush below low water
    assert!(ctrl.record_flushed(20));
    assert!(ctrl.is_write_ready());
}

#[tokio::test]
async fn backpressure_controller_hysteresis_prevents_flapping() {
    let cfg = WriteConfig {
        high_water_mark: 100,
        low_water_mark: 50,
    };
    let mut ctrl = BackpressureController::new(cfg);

    // Engage
    ctrl.record_buffered(100);
    assert!(!ctrl.is_write_ready());

    // Drop to 60 — still above low water, still backpressured
    ctrl.record_flushed(40);
    assert!(!ctrl.is_write_ready());

    // Drop to 50 — at low water, released
    ctrl.record_flushed(10);
    assert!(ctrl.is_write_ready());

    // Go back up to 80 — below high water, still ready (hysteresis)
    ctrl.record_buffered(30);
    assert!(ctrl.is_write_ready());
}

#[tokio::test]
async fn read_limiter_pauses_and_resumes() {
    let mut limiter = ReadLimiter::new(3);

    assert!(!limiter.is_read_paused());

    // Fill to limit
    assert!(!limiter.record_received());
    assert!(!limiter.record_received());
    assert!(limiter.record_received()); // third → paused
    assert!(limiter.is_read_paused());
    assert_eq!(limiter.pending(), 3);

    // Consume one → resume
    assert!(limiter.record_consumed());
    assert!(!limiter.is_read_paused());
    assert_eq!(limiter.pending(), 2);
}

#[tokio::test]
async fn bounded_channel_slow_consumer_protection() {
    // Simulate a slow consumer scenario: a producer that produces faster
    // than the consumer processes. The bounded channel should prevent
    // unbounded memory growth by blocking the producer.
    let (tx, mut rx) = BoundedChannel::channel::<u64>(4);
    let produced = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let consumed = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Fast producer
    let produced_clone = produced.clone();
    let tx_clone = tx.clone();
    let producer = tokio::spawn(async move {
        for i in 0..20 {
            tx_clone.send(i).await.unwrap();
            produced_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    drop(tx);

    // Slow consumer — adds delay between each receive
    let consumed_clone = consumed.clone();
    let consumer = tokio::spawn(async move {
        while let Some(_val) = rx.recv().await {
            consumed_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    producer.await.unwrap();
    consumer.await.unwrap();

    // All messages should be received despite the speed difference
    assert_eq!(
        produced.load(std::sync::atomic::Ordering::SeqCst),
        20
    );
    assert_eq!(
        consumed.load(std::sync::atomic::Ordering::SeqCst),
        20
    );
}

#[tokio::test]
async fn bounded_channel_capacity_reflects_creation() {
    let (tx, rx) = BoundedChannel::channel::<String>(42);
    assert_eq!(tx.capacity(), 42);
    assert_eq!(rx.capacity(), 42);
}

// ---------------------------------------------------------------------------
// 6. Connection lifecycle over Listener (accept/connect integration)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn listener_accepts_then_communicates() {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();

    let connector = Connector::new();
    let mut client = connector.connect(addr).await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();

    // Bidirectional communication works
    let msg = ping_msg("listener-test");
    client.send(msg.clone()).await.unwrap();
    let received = server.recv().await.unwrap().unwrap();
    assert_eq!(received, msg);

    let reply = pong_msg("listener-test");
    server.send(reply.clone()).await.unwrap();
    let received = client.recv().await.unwrap().unwrap();
    assert_eq!(received, reply);
}

#[tokio::test]
async fn accept_loop_processes_clients_until_shutdown() {
    let listener = Arc::new(loopback_listener().await);
    let addr = listener.local_addr().unwrap();
    let (controller, mut signal) = ShutdownSignal::new();

    let listener_clone = listener.clone();
    let handle = tokio::spawn(async move {
        let mut count = 0u32;
        loop {
            match listener_clone.accept_with_shutdown(&mut signal).await {
                Ok(Some(_)) => count += 1,
                Ok(None) | Err(_) => break,
            }
        }
        count
    });

    // Connect 3 clients
    for _ in 0..3 {
        let _ = TcpStream::connect(addr).await.unwrap();
    }

    // Give the accept loop time to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    controller.shutdown();
    let count = handle.await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn connection_survives_many_messages() {
    let (mut client, mut server) = connection_pair().await;

    let message_count = 100;
    for i in 0..message_count {
        let msg = ping_msg(&format!("msg-{i}"));
        client.send(msg).await.unwrap();
    }

    for i in 0..message_count {
        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received.params[0], format!("msg-{i}"));
    }
}
