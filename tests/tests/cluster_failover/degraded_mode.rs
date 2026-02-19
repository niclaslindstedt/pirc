//! Degraded mode tests: cluster behavior when quorum is lost, including
//! proposal non-commitment, read-only state access, and quorum recovery.

use std::time::Duration;

use tokio::time;

use pirc_server::raft::{ClusterCommand, RaftState};

use super::TestCluster;

// ===========================================================================
// Quorum Loss: Proposals Cannot Commit
// ===========================================================================

#[tokio::test]
async fn proposals_do_not_commit_when_followers_dead() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill both followers to lose replication quorum.
    let followers: Vec<u64> = cluster.remaining_nodes(&[leader_id]);
    for &id in &followers {
        cluster.kill_node(id);
    }

    // Wait for followers to fully stop.
    time::sleep(Duration::from_millis(200)).await;

    // Take commit receiver and drain any pre-existing entries.
    let mut commit_rx = cluster.handle_mut(leader_id).take_commit_rx();
    loop {
        match time::timeout(Duration::from_millis(200), commit_rx.recv()).await {
            Ok(Some(_)) => continue,
            _ => break,
        }
    }

    // The leader can still accept proposals (it's still in Leader state),
    // but they should NOT commit because there's no majority to replicate to.
    let result = cluster.handle(leader_id).propose(ClusterCommand::Noop {
        description: "should-not-commit".to_owned(),
    });
    assert!(result.is_ok(), "leader should accept the proposal");

    // The entry should not commit within a reasonable timeout.
    let commit_result = time::timeout(Duration::from_millis(500), commit_rx.recv()).await;
    assert!(
        commit_result.is_err(),
        "proposal should not commit without quorum to replicate to"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn five_node_cluster_loses_quorum_with_three_failures() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill the leader + 2 followers = 3 dead, 2 survive.
    // Quorum requires 3 of 5. The 2 survivors can't form quorum.
    let mut killed = vec![leader_id];
    for &id in &[1, 2, 3, 4, 5] {
        if id != leader_id && killed.len() < 3 {
            killed.push(id);
        }
    }
    for &id in &killed {
        cluster.kill_node(id);
    }

    let remaining: Vec<u64> = [1, 2, 3, 4, 5]
        .iter()
        .copied()
        .filter(|id| !killed.contains(id))
        .collect();

    // Wait for election attempts to fail (no quorum).
    time::sleep(Duration::from_secs(2)).await;

    // Neither surviving node should be leader.
    for &id in &remaining {
        assert_ne!(
            cluster.handle(id).state(),
            RaftState::Leader,
            "node {id} should not be leader without quorum"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// State Still Readable in Degraded Mode
// ===========================================================================

#[tokio::test]
async fn state_readable_when_followers_dead() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Commit some state before losing followers.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "pre_loss_user".to_owned(),
            username: "pre_loss_user".to_owned(),
            realname: "Pre Loss".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        })
        .unwrap();

    // Allow replication.
    time::sleep(Duration::from_millis(500)).await;

    // Kill followers.
    let followers = cluster.remaining_nodes(&[leader_id]);
    for &id in &followers {
        cluster.kill_node(id);
    }

    time::sleep(Duration::from_millis(200)).await;

    // The surviving leader node should still have readable state.
    let term = cluster.handle(leader_id).current_term();
    assert!(
        term >= pirc_server::raft::types::Term::new(1),
        "surviving node should still have a valid term"
    );

    // State subscription should still work (read-only operation).
    let state_rx = cluster.handle(leader_id).subscribe_state();
    let (state, _term, _leader) = *state_rx.borrow();
    // The leader may still be in Leader state (driver is running),
    // but read operations work regardless.
    assert!(
        state == RaftState::Leader
            || state == RaftState::Follower
            || state == RaftState::Candidate,
        "surviving node should have a valid state"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Quorum Recovery
// ===========================================================================

#[tokio::test]
async fn five_node_cluster_tolerates_minority_failure() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill 2 nodes (minority) — 3 remain, still have quorum.
    let mut killed = Vec::new();
    for &id in &[1, 2, 3, 4, 5] {
        if killed.len() < 2 {
            killed.push(id);
            cluster.kill_node(id);
        }
    }

    let remaining: Vec<u64> = [1, 2, 3, 4, 5]
        .iter()
        .copied()
        .filter(|id| !killed.contains(id))
        .collect();

    // A leader should still emerge among the 3 remaining nodes.
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(4))
        .await
        .expect("3 remaining nodes should elect a leader (quorum = 3 of 5)");

    // Verify proposals still work.
    let result = cluster.handle(new_leader).propose(ClusterCommand::Noop {
        description: "post-minority-failure".to_owned(),
    });
    assert!(
        result.is_ok(),
        "proposals should work with quorum maintained"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Sequential Failures: Cluster Degrades Gracefully
// ===========================================================================

#[tokio::test]
async fn sequential_failures_in_five_node_cluster() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill first node.
    cluster.kill_node(1);

    let remaining_4: Vec<u64> = vec![2, 3, 4, 5];
    let _leader_after_first = cluster
        .wait_for_leader_among(&remaining_4, Duration::from_secs(3))
        .await
        .expect("4 remaining nodes should elect a leader");

    // Kill second node.
    cluster.kill_node(2);

    let remaining_3: Vec<u64> = vec![3, 4, 5];
    let leader_after_second = cluster
        .wait_for_leader_among(&remaining_3, Duration::from_secs(3))
        .await
        .expect("3 remaining nodes should still elect a leader");

    // Proposals should still work.
    let result = cluster
        .handle(leader_after_second)
        .propose(ClusterCommand::Noop {
            description: "after-two-failures".to_owned(),
        });
    assert!(result.is_ok(), "cluster should still accept proposals");

    // Kill third node — now only 2 remain, no quorum (need 3 of 5).
    cluster.kill_node(3);

    // Wait for elections to fail.
    time::sleep(Duration::from_secs(2)).await;

    let remaining_2: Vec<u64> = vec![4, 5];
    for &id in &remaining_2 {
        assert_ne!(
            cluster.handle(id).state(),
            RaftState::Leader,
            "node {id} should not be leader with only 2 of 5 nodes"
        );
    }

    cluster.shutdown_all();
}
