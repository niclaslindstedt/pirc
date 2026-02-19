//! Cluster formation tests: startup, leader agreement, solo node behavior,
//! heartbeats, proposal routing, election timeout determinism, and state
//! subscriptions.

use std::time::Duration;

use tokio::time;

use pirc_server::raft::{
    LogIndex, NullStateMachine, RaftBuilder, RaftConfig, RaftState,
};
use pirc_server::raft::types::{NodeId, Term};

use super::{MemStorage, TestCluster, test_config};

// ===========================================================================
// Cluster Startup
// ===========================================================================

#[tokio::test]
async fn three_node_cluster_elects_leader() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("a leader should be elected within 2 seconds");

    assert!(
        [1, 2, 3].contains(&leader_id),
        "leader should be one of the cluster nodes"
    );

    // The leader should report itself as leader.
    assert_eq!(cluster.handle(leader_id).state(), RaftState::Leader);

    cluster.shutdown_all();
}

#[tokio::test]
async fn five_node_cluster_elects_leader_within_two_seconds() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("5-node cluster should elect a leader within 2 seconds (REQ-053)");

    assert!(
        [1, 2, 3, 4, 5].contains(&leader_id),
        "leader should be one of the cluster nodes"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn deterministic_election_succession_lower_id_wins() {
    // Lower node IDs get shorter election timeouts, so they should win.
    // Run multiple trials to verify consistency.
    let mut leader_ids = Vec::new();
    for _ in 0..3 {
        let cluster = TestCluster::start(&[1, 2, 3]).await;

        let leader_id = cluster
            .wait_for_leader(Duration::from_secs(2))
            .await
            .expect("leader should be elected");

        leader_ids.push(leader_id);
        cluster.shutdown_all();
        // Small delay between trials.
        time::sleep(Duration::from_millis(50)).await;
    }

    // Node 1 should win all elections due to shortest timeout.
    for &id in &leader_ids {
        assert_eq!(
            id, 1,
            "node 1 should win election due to deterministic succession (lower ID = shorter timeout)"
        );
    }
}

#[tokio::test]
async fn all_nodes_agree_on_leader_after_election() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader_agreement(Duration::from_secs(3))
        .await
        .expect("all nodes should agree on a leader");

    // All nodes should report the same leader.
    for id in [1, 2, 3] {
        let reported_leader = cluster.handle(id).current_leader();
        assert_eq!(
            reported_leader,
            Some(NodeId::new(leader_id)),
            "node {id} should agree on leader {leader_id}"
        );
    }

    cluster.shutdown_all();
}

#[tokio::test]
async fn election_increments_term() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // All nodes should have term >= 1 after an election.
    for id in [1, 2, 3] {
        assert!(
            cluster.handle(id).current_term() >= Term::new(1),
            "node {id} should have term >= 1 after election"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Solo Node Behavior
// ===========================================================================

#[tokio::test]
async fn solo_node_auto_elects_leader() {
    let (mut driver, handle, shutdown, _inbound_tx, _outbound_rx) =
        RaftBuilder::<String, _, _>::new()
            .config(test_config(1, vec![]))
            .storage(MemStorage::new())
            .state_machine(NullStateMachine)
            .build()
            .await
            .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    time::sleep(Duration::from_millis(300)).await;

    assert_eq!(handle.state(), RaftState::Leader);
    assert_eq!(handle.current_leader(), Some(NodeId::new(1)));
    assert!(handle.current_term() >= Term::new(1));

    shutdown.shutdown();
}

#[tokio::test]
async fn solo_node_commits_immediately() {
    let (mut driver, mut handle, shutdown, _inbound_tx, _outbound_rx) =
        RaftBuilder::<String, _, _>::new()
            .config(test_config(1, vec![]))
            .storage(MemStorage::new())
            .state_machine(NullStateMachine)
            .build()
            .await
            .unwrap();

    let mut commit_rx = handle.take_commit_rx();

    tokio::spawn(async move {
        driver.run().await;
    });

    time::sleep(Duration::from_millis(300)).await;
    assert!(handle.is_leader());

    handle.propose("solo-cmd".to_owned()).unwrap();

    let entry = time::timeout(Duration::from_secs(1), commit_rx.recv())
        .await
        .expect("solo node should commit immediately")
        .expect("commit channel should not be closed");

    assert_eq!(entry.command, "solo-cmd");
    assert_eq!(entry.index, LogIndex::new(1));

    shutdown.shutdown();
}

// ===========================================================================
// Term Consistency
// ===========================================================================

#[tokio::test]
async fn followers_adopt_leader_term() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Allow time for heartbeats to propagate term.
    time::sleep(Duration::from_millis(300)).await;

    let leader_term = cluster.handle(leader_id).current_term();
    for id in [1, 2, 3] {
        assert_eq!(
            cluster.handle(id).current_term(),
            leader_term,
            "node {id} should have the same term as leader"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Heartbeat and Election Timeout
// ===========================================================================

#[tokio::test]
async fn heartbeats_prevent_re_election() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // With heartbeat_interval = 50ms and election_timeout_min = 150ms,
    // the leader sends heartbeats fast enough to prevent re-elections.
    time::sleep(Duration::from_secs(1)).await;

    // Leader should still be leader after 1 second.
    assert_eq!(
        cluster.handle(leader_id).state(),
        RaftState::Leader,
        "leader should remain leader while heartbeats flow"
    );

    // No other node should be leader.
    for id in [1, 2, 3] {
        if id != leader_id {
            assert_ne!(
                cluster.handle(id).state(),
                RaftState::Leader,
                "node {id} should not be leader while original leader is healthy"
            );
        }
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Proposal Rejection
// ===========================================================================

#[tokio::test]
async fn proposal_rejected_when_not_leader() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Find a follower.
    let follower_id = [1, 2, 3]
        .iter()
        .copied()
        .find(|&id| id != leader_id)
        .unwrap();

    let result = cluster.handle(follower_id).propose("test".to_owned());
    assert!(
        result.is_err(),
        "proposals to followers should be rejected"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn leader_accepts_proposals() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let result = cluster
        .handle(leader_id)
        .propose("accepted".to_owned());
    assert!(
        result.is_ok(),
        "proposals to leader should be accepted"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Election Timeout Determinism
// ===========================================================================

#[tokio::test]
async fn election_timeout_is_deterministic_for_same_config() {
    use pirc_server::raft::compute_election_timeout;

    let config1 = RaftConfig {
        election_timeout_min: Duration::from_millis(100),
        election_timeout_max: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(50),
        node_id: NodeId::new(1),
        peers: vec![NodeId::new(2), NodeId::new(3)],
        ..RaftConfig::default()
    };

    let config2 = RaftConfig {
        election_timeout_min: Duration::from_millis(100),
        election_timeout_max: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(50),
        node_id: NodeId::new(2),
        peers: vec![NodeId::new(1), NodeId::new(3)],
        ..RaftConfig::default()
    };

    let timeout1 = compute_election_timeout(&config1);
    let timeout2 = compute_election_timeout(&config2);

    // Node 1 should have a shorter timeout than node 2 (lower ID = higher priority).
    assert!(
        timeout1 < timeout2,
        "node 1 (timeout {timeout1:?}) should have shorter timeout than node 2 ({timeout2:?})"
    );

    // Timeouts should be consistent across calls.
    assert_eq!(
        timeout1,
        compute_election_timeout(&config1),
        "election timeout should be deterministic"
    );
}

// ===========================================================================
// State Subscriptions
// ===========================================================================

#[tokio::test]
async fn state_subscription_reflects_transitions() {
    let (mut driver, handle, shutdown, _inbound_tx, _outbound_rx) =
        RaftBuilder::<String, _, _>::new()
            .config(test_config(1, vec![]))
            .storage(MemStorage::new())
            .state_machine(NullStateMachine)
            .build()
            .await
            .unwrap();

    let mut state_rx = handle.subscribe_state();

    // Initially follower.
    let (state, _, _) = *state_rx.borrow();
    assert_eq!(state, RaftState::Follower);

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for state change (solo node becomes leader).
    let result = time::timeout(Duration::from_secs(1), state_rx.wait_for(|&(s, _, _)| s == RaftState::Leader))
        .await;

    assert!(
        result.is_ok(),
        "state subscription should reflect Leader transition"
    );

    shutdown.shutdown();
}
