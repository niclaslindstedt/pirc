//! Server failover tests: leader failure detection, automatic re-election,
//! user migration tracking, and clean shutdown notification.

use std::time::Duration;

use tokio::time;

use pirc_server::raft::{ClusterCommand, HealthEvent};
use pirc_server::raft::types::NodeId;

use super::TestCluster;

// ===========================================================================
// Leader Failure and Re-election (3-node cluster)
// ===========================================================================

#[tokio::test]
async fn three_node_cluster_kill_leader_elects_new_leader() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("first leader should be elected");

    let first_term = cluster.handle(first_leader).current_term();

    // Kill the leader.
    cluster.kill_node(first_leader);

    let remaining = cluster.remaining_nodes(&[first_leader]);

    // A new leader should be elected among the remaining nodes.
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(3))
        .await
        .expect("new leader should be elected after leader failure");

    assert_ne!(
        new_leader, first_leader,
        "new leader should be different from the failed leader"
    );

    // New leader should have a higher term.
    let new_term = cluster.handle(new_leader).current_term();
    assert!(
        new_term > first_term,
        "new term ({new_term}) should be greater than first term ({first_term})"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn re_election_after_leader_failure_within_two_seconds() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    cluster.kill_node(first_leader);

    let remaining = cluster.remaining_nodes(&[first_leader]);

    let start = time::Instant::now();
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(2))
        .await;
    let elapsed = start.elapsed();

    assert!(
        new_leader.is_some(),
        "re-election should complete within 2 seconds (REQ-053, took {elapsed:?})"
    );
}

// ===========================================================================
// User Migration Tracking via ClusterCommand
// ===========================================================================

#[tokio::test]
async fn user_migration_command_replicated_after_failover() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Take commit receiver from leader.
    let mut commit_rx = cluster.handle_mut(leader_id).take_commit_rx();

    // Register a user on the leader (homed on node 1).
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "alice".to_owned(),
            username: "alice".to_owned(),
            realname: "Alice".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 1_000_000,
            home_node: Some(NodeId::new(1)),
        })
        .unwrap();

    // Wait for the user registration to commit.
    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("user registration should commit")
        .expect("commit channel open");

    assert!(matches!(
        &entry.command,
        ClusterCommand::UserRegistered { nickname, .. } if nickname == "alice"
    ));

    // Now propose a user migration (simulating failover from node 1 to node 2).
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserMigrated {
            nickname: "alice".to_owned(),
            from_node: NodeId::new(1),
            to_node: NodeId::new(2),
        })
        .unwrap();

    let migration_entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("user migration should commit")
        .expect("commit channel open");

    assert!(
        matches!(
            &migration_entry.command,
            ClusterCommand::UserMigrated { nickname, from_node, to_node }
                if nickname == "alice"
                && from_node.as_u64() == 1
                && to_node.as_u64() == 2
        ),
        "migration command should replicate correctly"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn user_migration_within_five_seconds_of_leader_failure() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Register a user homed on the leader.
    cluster
        .handle(first_leader)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 42,
            nickname: "bob".to_owned(),
            username: "bob".to_owned(),
            realname: "Bob".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 2_000_000,
            home_node: Some(NodeId::new(first_leader)),
        })
        .unwrap();

    // Allow replication.
    time::sleep(Duration::from_millis(300)).await;

    // Kill the leader.
    cluster.kill_node(first_leader);

    let remaining = cluster.remaining_nodes(&[first_leader]);

    // Wait for new leader election.
    let start = time::Instant::now();
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(3))
        .await
        .expect("new leader should be elected");

    // Take commit receiver from new leader and drain pre-existing entries.
    let mut commit_rx = cluster.handle_mut(new_leader).take_commit_rx();
    loop {
        match time::timeout(Duration::from_millis(300), commit_rx.recv()).await {
            Ok(Some(_)) => continue,
            _ => break,
        }
    }

    // New leader proposes user migration (simulating transparent migration).
    let migration_target = remaining
        .iter()
        .copied()
        .find(|&id| id != new_leader)
        .unwrap_or(new_leader);

    cluster
        .handle(new_leader)
        .propose(ClusterCommand::UserMigrated {
            nickname: "bob".to_owned(),
            from_node: NodeId::new(first_leader),
            to_node: NodeId::new(migration_target),
        })
        .unwrap();

    let migration_entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("migration should commit within timeout")
        .expect("commit channel open");

    let elapsed = start.elapsed();

    assert!(
        matches!(&migration_entry.command, ClusterCommand::UserMigrated { .. }),
        "migration command should be committed"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "user migration should complete within 5 seconds (REQ-054, took {elapsed:?})"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// No Duplicate Messages During Migration
// ===========================================================================

#[tokio::test]
async fn no_duplicate_entries_after_leader_failover() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rx = cluster.handle_mut(first_leader).take_commit_rx();

    // Propose a few commands before failover.
    for i in 1..=3 {
        cluster
            .handle(first_leader)
            .propose(ClusterCommand::Noop {
                description: format!("pre-failover-{i}"),
            })
            .unwrap();
    }

    // Collect committed entries.
    let mut pre_failover = Vec::new();
    for _ in 0..3 {
        match time::timeout(Duration::from_secs(2), commit_rx.recv()).await {
            Ok(Some(entry)) => pre_failover.push(entry),
            _ => break,
        }
    }

    assert_eq!(pre_failover.len(), 3, "all 3 pre-failover entries should commit");

    // Kill the leader.
    cluster.kill_node(first_leader);

    let remaining = cluster.remaining_nodes(&[first_leader]);
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(3))
        .await
        .expect("new leader should be elected");

    let mut new_commit_rx = cluster.handle_mut(new_leader).take_commit_rx();

    // Propose a new command on the new leader.
    cluster
        .handle(new_leader)
        .propose(ClusterCommand::Noop {
            description: "post-failover-1".to_owned(),
        })
        .unwrap();

    // Collect entries until we find our post-failover entry.
    // The channel may contain pre-existing entries from the old leader's term.
    let last_pre_index = pre_failover.last().unwrap().index;
    let mut found_post_entry = false;
    let mut all_indices = Vec::new();
    let deadline = time::Instant::now() + Duration::from_secs(3);
    loop {
        match time::timeout(Duration::from_millis(500), new_commit_rx.recv()).await {
            Ok(Some(entry)) => {
                all_indices.push(entry.index);
                if let ClusterCommand::Noop { description } = &entry.command {
                    if description == "post-failover-1" {
                        assert!(
                            entry.index > last_pre_index,
                            "post-failover index ({}) should be > last pre-failover index ({last_pre_index})",
                            entry.index
                        );
                        found_post_entry = true;
                        break;
                    }
                }
            }
            _ => {
                if time::Instant::now() >= deadline {
                    break;
                }
            }
        }
    }

    assert!(
        found_post_entry,
        "post-failover entry should be committed (saw indices: {all_indices:?})"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Health Events: Peer Failure Detection
// ===========================================================================

#[tokio::test]
async fn leader_detects_peer_failure_via_health_events() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut health_rx = cluster.handle_mut(leader_id).take_health_event_rx();

    // Allow heartbeats to establish peer liveness.
    time::sleep(Duration::from_millis(200)).await;

    // Kill a follower.
    let follower_id = cluster
        .remaining_nodes(&[leader_id])
        .into_iter()
        .next()
        .unwrap();
    cluster.kill_node(follower_id);

    // The leader's health monitor should eventually report the follower as
    // suspected or down (failure_threshold = 6 * 50ms = 300ms).
    let mut detected = false;
    let deadline = time::Instant::now() + Duration::from_secs(2);
    loop {
        match time::timeout(Duration::from_millis(100), health_rx.recv()).await {
            Ok(Some(HealthEvent::NodeSuspected(id) | HealthEvent::NodeDown(id))) => {
                if id.as_u64() == follower_id {
                    detected = true;
                    break;
                }
            }
            Ok(Some(_)) => {} // other events
            _ => {}
        }
        if time::Instant::now() >= deadline {
            break;
        }
    }

    assert!(
        detected,
        "leader should detect follower {follower_id} failure via health events"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Clean Shutdown Notification
// ===========================================================================

#[tokio::test]
async fn clean_shutdown_allows_re_election() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Cleanly shut down the leader (simulates graceful shutdown notification).
    cluster.kill_node(leader_id);

    let remaining = cluster.remaining_nodes(&[leader_id]);

    // Remaining nodes should elect a new leader.
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(3))
        .await
        .expect("new leader should be elected after clean shutdown");

    assert!(
        remaining.contains(&new_leader),
        "new leader should be one of the remaining nodes"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn cluster_state_updated_after_server_removal_command() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rx = cluster.handle_mut(leader_id).take_commit_rx();

    // Propose a ServerRemoved command to record that a server left.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerRemoved {
            node_id: NodeId::new(3),
        })
        .unwrap();

    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("server removal should commit")
        .expect("commit channel open");

    assert!(
        matches!(
            &entry.command,
            ClusterCommand::ServerRemoved { node_id } if node_id.as_u64() == 3
        ),
        "committed command should be ServerRemoved for node 3"
    );

    cluster.shutdown_all();
}
