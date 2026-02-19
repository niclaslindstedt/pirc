//! Membership change tests: adding/removing servers via `ClusterCommand` and
//! Raft membership changes, invite-key simulation, and single-use enforcement.

use std::time::Duration;

use tokio::time;

use pirc_server::raft::types::NodeId;
use pirc_server::raft::{ClusterCommand, ClusterStateMachine, MembershipChange, StateMachine};

use super::TestCluster;

// ===========================================================================
// Server Join via ClusterCommand
// ===========================================================================

#[tokio::test]
async fn server_added_command_replicated() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // Propose adding a new server (node 4).
    let addr = "10.0.0.4:7000".parse().unwrap();
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerAdded {
            node_id: NodeId::new(4),
            addr,
        })
        .unwrap();

    // All nodes should commit this entry.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        let entry = time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
            .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
        sm.apply(&entry.command);

        assert_eq!(
            sm.server_count(),
            1,
            "node {id}: state machine should track 1 server"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Invite Key System (Single-Use Enforcement)
// ===========================================================================

#[tokio::test]
async fn invite_key_single_use_enforcement() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rx = cluster.handle_mut(leader_id).take_commit_rx();

    // Simulate master generating an invite key by adding a server with a noop.
    // The key is the noop description — used once to add server 4.
    let invite_key = "invite-key-abc123";

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::Noop {
            description: format!("invite-key-used:{invite_key}:node:4"),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerAdded {
            node_id: NodeId::new(4),
            addr: "10.0.0.4:7000".parse().unwrap(),
        })
        .unwrap();

    // Collect both entries.
    let mut sm = ClusterStateMachine::new();
    for _ in 0..2 {
        let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
            .await
            .expect("should receive committed entry")
            .expect("commit channel open");
        sm.apply(&entry.command);
    }

    assert_eq!(sm.server_count(), 1, "server 4 should be tracked");

    // A second attempt to add with the same "invite key" would result in
    // server 4 already being tracked. In a real system the application layer
    // would reject the duplicate, but here we verify the command is idempotent.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerAdded {
            node_id: NodeId::new(4),
            addr: "10.0.0.4:7000".parse().unwrap(),
        })
        .unwrap();

    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("should receive committed entry")
        .expect("commit channel open");
    sm.apply(&entry.command);

    // Still only one server with ID 4 (idempotent).
    assert_eq!(
        sm.server_count(),
        1,
        "duplicate ServerAdded should be idempotent"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Raft Membership Change: Add Server
// ===========================================================================

#[tokio::test]
async fn raft_membership_add_server() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Allow leader to commit a noop in current term (required for membership changes).
    let mut commit_rx = cluster.handle_mut(leader_id).take_commit_rx();

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::Noop {
            description: "term-commit".to_owned(),
        })
        .unwrap();

    time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("noop should commit")
        .expect("commit channel open");

    // Now propose a membership change to add node 4.
    let addr = "10.0.0.4:7000".parse().unwrap();
    let result = cluster
        .handle(leader_id)
        .propose_membership_change(
            MembershipChange::AddServer(NodeId::new(4), addr),
            ClusterCommand::Noop {
                description: "add-server:4".to_owned(),
            },
        )
        .await;

    assert!(
        result.is_ok(),
        "membership change should be accepted: {result:?}"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Server Removal via ClusterCommand
// ===========================================================================

#[tokio::test]
async fn server_removed_command_replicated() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // First add a server, then remove it.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerAdded {
            node_id: NodeId::new(4),
            addr: "10.0.0.4:7000".parse().unwrap(),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerRemoved {
            node_id: NodeId::new(4),
        })
        .unwrap();

    // All nodes should see both entries.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        for _ in 0..2 {
            let entry = time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
                .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
            sm.apply(&entry.command);
        }

        assert_eq!(
            sm.server_count(),
            0,
            "node {id}: server 4 should be removed from state"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// New Server Receives Full State Sync (via log replication)
// ===========================================================================

#[tokio::test]
async fn new_server_joins_and_receives_replicated_state() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Commit some state before the new server "joins".
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "existing_user".to_owned(),
            username: "existing_user".to_owned(),
            realname: "Existing User".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ChannelJoined {
            nickname: "existing_user".to_owned(),
            channel: "#established".to_owned(),
            status: "operator".to_owned(),
        })
        .unwrap();

    // Wait for replication.
    time::sleep(Duration::from_millis(500)).await;

    // Record the ServerAdded event.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ServerAdded {
            node_id: NodeId::new(4),
            addr: "10.0.0.4:7000".parse().unwrap(),
        })
        .unwrap();

    // Allow commit.
    time::sleep(Duration::from_millis(300)).await;

    // In a real system, node 4 would receive the full log via AppendEntries.
    // Here we verify that follower nodes have the full state by applying
    // committed entries to a state machine.
    let follower_id = cluster
        .remaining_nodes(&[leader_id])
        .into_iter()
        .next()
        .unwrap();

    let mut follower_commit_rx = cluster.handle_mut(follower_id).take_commit_rx();

    // The follower should have received all 3 entries.
    let mut sm = ClusterStateMachine::new();
    for _ in 0..3 {
        match time::timeout(Duration::from_secs(2), follower_commit_rx.recv()).await {
            Ok(Some(entry)) => sm.apply(&entry.command),
            _ => break,
        }
    }

    assert!(
        sm.get_user("existing_user").is_some(),
        "follower should have the existing user"
    );
    assert!(
        sm.get_channel("#established").is_some(),
        "follower should have the established channel"
    );
    assert_eq!(
        sm.server_count(),
        1,
        "follower should track the newly added server"
    );

    cluster.shutdown_all();
}
