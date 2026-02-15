use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::{Command, Message};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::degraded_mode::{DegradedModeState, SharedDegradedState};
use crate::raft::types::{NodeId, RaftState, Term};
use crate::registry::UserRegistry;
use crate::user::UserSession;

/// Create a test user session registered in the given registry.
/// Returns the receiver for messages sent to this user.
fn register_test_user(
    registry: &UserRegistry,
    nick: &str,
    conn_id: u64,
) -> mpsc::UnboundedReceiver<Message> {
    let (tx, rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let session = UserSession {
        connection_id: conn_id,
        nickname: Nickname::new(nick).unwrap(),
        username: format!("user{conn_id}"),
        realname: format!("Real {nick}"),
        hostname: "127.0.0.1".to_owned(),
        modes: HashSet::new(),
        away_message: None,
        connected_at: now,
        signon_time: 0,
        last_active: now,
        registered: true,
        sender: tx,
    };
    registry.register(session).unwrap();
    rx
}

// ---- DegradedModeState unit tests ----

#[test]
fn initially_not_degraded() {
    let state = DegradedModeState::new();
    assert!(!state.is_degraded());
}

#[test]
fn set_and_clear_degraded() {
    let state = DegradedModeState::new();
    state.set_degraded(true);
    assert!(state.is_degraded());
    state.set_degraded(false);
    assert!(!state.is_degraded());
}

// ---- Monitor tests ----

#[tokio::test]
async fn monitor_detects_quorum_loss() {
    let (state_tx, state_rx) = watch::channel((
        RaftState::Leader,
        Term::new(1),
        Some(NodeId::new(1)),
    ));
    let registry = Arc::new(UserRegistry::new());
    let mut user_rx = register_test_user(&registry, "Alice", 1);
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let _handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        false, // multi-node cluster
    );

    // Allow the monitor to start and process the initial state.
    tokio::task::yield_now().await;

    assert!(!degraded_state.is_degraded());

    // Simulate leader loss (quorum lost).
    state_tx
        .send((RaftState::Follower, Term::new(2), None))
        .unwrap();

    // Give the monitor time to process.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(degraded_state.is_degraded());

    // Check that the user received a degraded notice.
    let msg = user_rx.try_recv().expect("should receive degraded notice");
    assert_eq!(msg.command, Command::Notice);
    let text = msg.params.last().unwrap();
    assert!(text.contains("degraded mode"), "got: {text}");
}

#[tokio::test]
async fn monitor_detects_quorum_restore() {
    let (state_tx, state_rx) = watch::channel((
        RaftState::Leader,
        Term::new(1),
        Some(NodeId::new(1)),
    ));
    let registry = Arc::new(UserRegistry::new());
    let mut user_rx = register_test_user(&registry, "Bob", 2);
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let _handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        false,
    );

    tokio::task::yield_now().await;

    // Transition to degraded.
    state_tx
        .send((RaftState::Follower, Term::new(2), None))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(degraded_state.is_degraded());

    // Drain the degraded notice.
    let _ = user_rx.try_recv();

    // Restore quorum.
    state_tx
        .send((RaftState::Follower, Term::new(3), Some(NodeId::new(2))))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(!degraded_state.is_degraded());

    // Check that the user received a restore notice.
    let msg = user_rx.try_recv().expect("should receive restore notice");
    assert_eq!(msg.command, Command::Notice);
    let text = msg.params.last().unwrap();
    assert!(text.contains("restored"), "got: {text}");
}

#[tokio::test]
async fn single_node_never_degrades() {
    let (state_tx, state_rx) = watch::channel((
        RaftState::Leader,
        Term::new(1),
        Some(NodeId::new(1)),
    ));
    let registry = Arc::new(UserRegistry::new());
    let mut user_rx = register_test_user(&registry, "Carol", 3);
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let _handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        true, // single-node cluster
    );

    tokio::task::yield_now().await;

    // Even if the leader field becomes None transiently, single-node
    // clusters should never enter degraded mode.
    state_tx
        .send((RaftState::Follower, Term::new(2), None))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(!degraded_state.is_degraded());
    // No notice should be sent.
    assert!(user_rx.try_recv().is_err());
}

#[tokio::test]
async fn monitor_shuts_down_when_channel_closes() {
    let (state_tx, state_rx) = watch::channel((
        RaftState::Leader,
        Term::new(1),
        Some(NodeId::new(1)),
    ));
    let registry = Arc::new(UserRegistry::new());
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        false,
    );

    tokio::task::yield_now().await;

    // Drop the sender to close the channel.
    drop(state_tx);

    // The monitor should complete without hanging.
    tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("monitor should shut down within timeout")
        .expect("monitor task should not panic");
}

#[tokio::test]
async fn no_spurious_restore_on_startup_without_leader() {
    // Start in a state without a leader — the monitor should NOT send
    // a "restored" notice since it was never degraded.
    let (state_tx, state_rx) = watch::channel((
        RaftState::Follower,
        Term::new(0),
        None,
    ));
    let registry = Arc::new(UserRegistry::new());
    let mut user_rx = register_test_user(&registry, "Dave", 4);
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let _handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        false,
    );

    tokio::task::yield_now().await;

    // Now a leader is elected — but we never had a leader before, so
    // this is not a "restore" scenario. The monitor should not enter
    // degraded mode in the first place.
    state_tx
        .send((RaftState::Follower, Term::new(1), Some(NodeId::new(1))))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(!degraded_state.is_degraded());
    // No notice should have been sent since we were never in degraded mode.
    assert!(user_rx.try_recv().is_err());
}

#[tokio::test]
async fn broadcast_reaches_multiple_users() {
    let (state_tx, state_rx) = watch::channel((
        RaftState::Leader,
        Term::new(1),
        Some(NodeId::new(1)),
    ));
    let registry = Arc::new(UserRegistry::new());
    let mut rx1 = register_test_user(&registry, "User1", 10);
    let mut rx2 = register_test_user(&registry, "User2", 20);
    let mut rx3 = register_test_user(&registry, "User3", 30);
    let degraded_state: SharedDegradedState = Arc::new(DegradedModeState::new());

    let _handle = super::spawn_degraded_mode_monitor(
        state_rx,
        Arc::clone(&registry),
        Arc::clone(&degraded_state),
        false,
    );

    tokio::task::yield_now().await;

    // Simulate quorum loss.
    state_tx
        .send((RaftState::Candidate, Term::new(2), None))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // All three users should have received the notice.
    for (name, rx) in [("User1", &mut rx1), ("User2", &mut rx2), ("User3", &mut rx3)] {
        let msg = rx.try_recv().unwrap_or_else(|_| panic!("{name} should receive notice"));
        assert_eq!(msg.command, Command::Notice);
        let text = msg.params.last().unwrap();
        assert!(text.contains("degraded"), "{name} notice: {text}");
    }
}
