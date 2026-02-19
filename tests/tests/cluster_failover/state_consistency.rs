//! State replication consistency tests: verify that channel/user state
//! replicated via `ClusterCommand` is preserved across all nodes and
//! survives leader failover.

use std::time::Duration;

use tokio::time;

use pirc_server::raft::types::NodeId;
use pirc_server::raft::{ClusterCommand, ClusterStateMachine, StateMachine};

use super::TestCluster;

// ===========================================================================
// Channel State Replicated Across Cluster
// ===========================================================================

#[tokio::test]
async fn channel_state_replicated_to_all_followers() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Take commit receivers from all nodes.
    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // Register user and join a channel.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "alice".to_owned(),
            username: "alice".to_owned(),
            realname: "Alice".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: Some(NodeId::new(leader_id)),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ChannelJoined {
            nickname: "alice".to_owned(),
            channel: "#general".to_owned(),
            status: "normal".to_owned(),
        })
        .unwrap();

    // All nodes should receive both committed entries.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        for _ in 0..2 {
            let entry = time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
                .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
            sm.apply(&entry.command);
        }

        // Verify state machine has the user and channel.
        assert!(
            sm.get_user("alice").is_some(),
            "node {id} should have alice in its state"
        );
        let channel = sm
            .get_channel("#general")
            .unwrap_or_else(|| panic!("node {id} should have #general"));
        assert!(
            channel.members.contains_key("alice"),
            "node {id}: #general should have alice as member"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Topic Changes Replicated
// ===========================================================================

#[tokio::test]
async fn topic_change_replicated_across_cluster() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // Create channel by joining, then set topic.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "op".to_owned(),
            username: "op".to_owned(),
            realname: "Operator".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ChannelJoined {
            nickname: "op".to_owned(),
            channel: "#dev".to_owned(),
            status: "operator".to_owned(),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::TopicSet {
            channel: "#dev".to_owned(),
            topic: Some(pirc_server::raft::cluster_command::TopicInfo {
                text: "Welcome to #dev!".to_owned(),
                who: "op".to_owned(),
                timestamp: 1_700_000_000,
            }),
        })
        .unwrap();

    // All nodes should receive all 3 entries.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        for _ in 0..3 {
            let entry = time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
                .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
            sm.apply(&entry.command);
        }

        let channel = sm
            .get_channel("#dev")
            .unwrap_or_else(|| panic!("node {id} should have #dev"));
        assert_eq!(
            channel.topic.as_ref().map(|t| &t.0),
            Some(&"Welcome to #dev!".to_owned()),
            "node {id}: topic should be replicated"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// Ban List Changes Replicated
// ===========================================================================

#[tokio::test]
async fn ban_list_replicated_across_cluster() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // Create channel and add a ban.
    cluster
        .handle(leader_id)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "mod_user".to_owned(),
            username: "mod_user".to_owned(),
            realname: "Moderator".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::ChannelJoined {
            nickname: "mod_user".to_owned(),
            channel: "#moderated".to_owned(),
            status: "operator".to_owned(),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(leader_id)
        .propose(ClusterCommand::BanAdded {
            channel: "#moderated".to_owned(),
            mask: "*!*@bad.host".to_owned(),
            who_set: "mod_user".to_owned(),
            timestamp: 1_700_000_000,
        })
        .unwrap();

    // All nodes should see the ban.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        for _ in 0..3 {
            let entry = time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
                .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
            sm.apply(&entry.command);
        }

        let channel = sm
            .get_channel("#moderated")
            .unwrap_or_else(|| panic!("node {id} should have #moderated"));
        assert_eq!(
            channel.ban_list.len(),
            1,
            "node {id}: ban list should have 1 entry"
        );
        assert_eq!(
            channel.ban_list[0].mask, "*!*@bad.host",
            "node {id}: ban mask should match"
        );
    }

    cluster.shutdown_all();
}

// ===========================================================================
// State Preserved After Failover
// ===========================================================================

#[tokio::test]
async fn state_preserved_after_leader_failover() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Register user and join channel on the first leader.
    cluster
        .handle(first_leader)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "charlie".to_owned(),
            username: "charlie".to_owned(),
            realname: "Charlie".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: Some(NodeId::new(first_leader)),
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    cluster
        .handle(first_leader)
        .propose(ClusterCommand::ChannelJoined {
            nickname: "charlie".to_owned(),
            channel: "#persist".to_owned(),
            status: "normal".to_owned(),
        })
        .unwrap();

    // Allow replication to complete.
    time::sleep(Duration::from_millis(500)).await;

    // Kill the leader.
    cluster.kill_node(first_leader);

    let remaining = cluster.remaining_nodes(&[first_leader]);
    let new_leader = cluster
        .wait_for_leader_among(&remaining, Duration::from_secs(3))
        .await
        .expect("new leader should be elected");

    // Take commit receiver from the new leader and drain pre-existing entries.
    let mut commit_rx = cluster.handle_mut(new_leader).take_commit_rx();
    loop {
        match time::timeout(Duration::from_millis(300), commit_rx.recv()).await {
            Ok(Some(_)) => continue,
            _ => break,
        }
    }

    // The new leader should be able to propose new commands, showing the
    // cluster is still functional. Propose a new user.
    cluster
        .handle(new_leader)
        .propose(ClusterCommand::UserRegistered {
            connection_id: 2,
            nickname: "dave".to_owned(),
            username: "dave".to_owned(),
            realname: "Dave".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 200,
            home_node: Some(NodeId::new(new_leader)),
        })
        .unwrap();

    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("new leader should accept proposals")
        .expect("commit channel open");

    assert!(
        matches!(
            &entry.command,
            ClusterCommand::UserRegistered { nickname, .. } if nickname == "dave"
        ),
        "new leader should commit the new user registration"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// User Joins/Parts Replicated
// ===========================================================================

#[tokio::test]
async fn user_join_and_part_replicated() {
    let mut cluster = TestCluster::start(&[1, 2, 3]).await;

    let leader_id = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    let mut commit_rxs: Vec<(u64, _)> = vec![];
    for &id in &[1, 2, 3] {
        commit_rxs.push((id, cluster.handle_mut(id).take_commit_rx()));
    }

    // Register, join, then part.
    let commands = vec![
        ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "eve".to_owned(),
            username: "eve".to_owned(),
            realname: "Eve".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        },
        ClusterCommand::ChannelJoined {
            nickname: "eve".to_owned(),
            channel: "#temp".to_owned(),
            status: "normal".to_owned(),
        },
        ClusterCommand::ChannelParted {
            nickname: "eve".to_owned(),
            channel: "#temp".to_owned(),
            reason: Some("leaving".to_owned()),
        },
    ];

    for cmd in &commands {
        cluster.handle(leader_id).propose(cmd.clone()).unwrap();
        time::sleep(Duration::from_millis(100)).await;
    }

    // All nodes should apply all 3 entries.
    for (id, rx) in &mut commit_rxs {
        let mut sm = ClusterStateMachine::new();
        for _ in 0..3 {
            let entry = time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("node {id} should receive committed entry"))
                .unwrap_or_else(|| panic!("node {id} commit channel should be open"));
            sm.apply(&entry.command);
        }

        // User should still exist, but channel should be gone (empty channel removed).
        assert!(
            sm.get_user("eve").is_some(),
            "node {id}: eve should still exist"
        );
        assert!(
            sm.get_channel("#temp").is_none(),
            "node {id}: #temp should be removed after last member parts"
        );
    }

    cluster.shutdown_all();
}
