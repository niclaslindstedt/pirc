use std::net::SocketAddr;

use crate::raft::membership::{MembershipChange, MembershipError};
use crate::raft::rpc::{AppendEntriesResponse, RaftMessage, RequestVoteResponse};
use crate::raft::test_utils::MemStorage;
use crate::raft::types::{LogIndex, NodeId, RaftConfig, RaftState};

use super::RaftNode;

fn nid(id: u64) -> NodeId {
    NodeId::new(id)
}

fn addr(last_octet: u8) -> SocketAddr {
    format!("10.0.0.{last_octet}:7000").parse().unwrap()
}

fn test_config(id: u64, peers: Vec<NodeId>) -> RaftConfig {
    RaftConfig {
        node_id: nid(id),
        peers,
        ..RaftConfig::default()
    }
}

async fn make_leader(
    id: u64,
    peers: Vec<NodeId>,
) -> (
    RaftNode<String, MemStorage>,
    tokio::sync::mpsc::UnboundedReceiver<(NodeId, RaftMessage<String>)>,
) {
    let config = test_config(id, peers.clone());
    let storage = MemStorage::new();
    let (mut node, rx) = RaftNode::new(config, storage).await.unwrap();

    // Start election
    node.start_election().await.unwrap();

    // Grant votes from all peers to become leader
    for &peer in &peers {
        let resp = RequestVoteResponse {
            term: node.current_term(),
            vote_granted: true,
        };
        node.handle_request_vote_response(peer, resp).await.unwrap();
    }

    assert_eq!(node.state(), RaftState::Leader);

    // Append a no-op entry and commit it to satisfy the "committed in current term" requirement.
    let idx = node.client_request("noop".to_owned()).unwrap();

    // Simulate all peers acknowledging the entry.
    for &peer in &peers {
        let resp = AppendEntriesResponse {
            term: node.current_term(),
            success: true,
            match_index: idx,
        };
        node.handle_append_entries_response(peer, resp).await.unwrap();
    }

    assert!(
        node.volatile_state().commit_index >= idx,
        "noop entry should be committed"
    );
    assert!(node.has_committed_in_current_term());

    (node, rx)
}

// ---- propose_membership_change ----

#[tokio::test]
async fn propose_add_server_as_leader() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let result = node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "config_change".to_owned(),
    );
    assert!(result.is_ok());

    let index = result.unwrap();
    assert!(index.as_u64() > 0);

    // Node 4 should now be in the membership.
    assert!(node.membership().is_member(nid(4)));
    assert_eq!(node.cluster_size(), 4);
    assert_eq!(node.quorum_size(), 3);

    // Should have pending change.
    assert!(node.membership().has_pending_change());
}

#[tokio::test]
async fn propose_remove_server_as_leader() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let result = node.propose_membership_change(
        MembershipChange::RemoveServer(nid(3)),
        "config_change".to_owned(),
    );
    assert!(result.is_ok());

    // Node 3 should no longer be in the membership.
    assert!(!node.membership().is_member(nid(3)));
    assert_eq!(node.cluster_size(), 2);
    assert_eq!(node.quorum_size(), 2);
}

#[tokio::test]
async fn propose_fails_when_not_leader() {
    let config = test_config(1, vec![nid(2), nid(3)]);
    let storage = MemStorage::new();
    let (mut node, _rx) = RaftNode::new(config, storage).await.unwrap();

    assert_eq!(node.state(), RaftState::Follower);

    let result = node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "config_change".to_owned(),
    );
    assert!(matches!(result, Err(MembershipError::NotLeader)));
}

#[tokio::test]
async fn propose_fails_when_change_pending() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    // First change succeeds.
    node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "change1".to_owned(),
    )
    .unwrap();

    // Second change fails.
    let result = node.propose_membership_change(
        MembershipChange::AddServer(nid(5), addr(5)),
        "change2".to_owned(),
    );
    assert!(matches!(result, Err(MembershipError::ChangePending(_))));
}

#[tokio::test]
async fn propose_fails_without_current_term_commit() {
    let config = test_config(1, vec![nid(2), nid(3)]);
    let storage = MemStorage::new();
    let (mut node, _rx) = RaftNode::new(config, storage).await.unwrap();

    // Start election and become leader.
    node.start_election().await.unwrap();
    for &peer in &[nid(2), nid(3)] {
        let resp = RequestVoteResponse {
            term: node.current_term(),
            vote_granted: true,
        };
        node.handle_request_vote_response(peer, resp).await.unwrap();
    }
    assert_eq!(node.state(), RaftState::Leader);

    // No entry committed in the current term yet.
    assert!(!node.has_committed_in_current_term());

    let result = node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "config_change".to_owned(),
    );
    assert!(matches!(
        result,
        Err(MembershipError::NoCurrentTermCommit)
    ));
}

#[tokio::test]
async fn propose_fails_adding_existing_member() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let result = node.propose_membership_change(
        MembershipChange::AddServer(nid(2), addr(2)),
        "config_change".to_owned(),
    );
    assert!(matches!(result, Err(MembershipError::AlreadyMember(_))));
}

#[tokio::test]
async fn propose_fails_removing_non_member() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let result = node.propose_membership_change(
        MembershipChange::RemoveServer(nid(99)),
        "config_change".to_owned(),
    );
    assert!(matches!(result, Err(MembershipError::NotMember(_))));
}

#[tokio::test]
async fn propose_fails_removing_last_member() {
    // Solo node.
    let config = test_config(1, vec![]);
    let storage = MemStorage::new();
    let (mut node, _rx) = RaftNode::new(config, storage).await.unwrap();

    // Become leader (solo election).
    node.start_election().await.unwrap();
    assert_eq!(node.state(), RaftState::Leader);

    // Commit a noop in current term (solo = immediately committed).
    node.client_request("noop".to_owned());
    node.advance_commit_index();
    assert!(node.has_committed_in_current_term());

    let result = node.propose_membership_change(
        MembershipChange::RemoveServer(nid(1)),
        "config_change".to_owned(),
    );
    assert!(matches!(
        result,
        Err(MembershipError::CannotRemoveLastMember)
    ));
}

// ---- Membership change commit ----

#[tokio::test]
async fn membership_change_committed_after_replication() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let change_index = node
        .propose_membership_change(
            MembershipChange::AddServer(nid(4), addr(4)),
            "add_node4".to_owned(),
        )
        .unwrap();

    assert!(node.membership().has_pending_change());

    // Simulate majority acknowledging the entry.
    // With 4 nodes, quorum is 3. Leader counts as 1, need 2 more.
    for &peer in &[nid(2), nid(3)] {
        let resp = AppendEntriesResponse {
            term: node.current_term(),
            success: true,
            match_index: change_index,
        };
        node.handle_append_entries_response(peer, resp)
            .await
            .unwrap();
    }

    // Change should be committed now.
    assert!(!node.membership().has_pending_change());
    assert_eq!(node.membership().generation(), 1);
    assert!(node.membership().is_member(nid(4)));
}

// ---- Leader self-removal ----

#[tokio::test]
async fn leader_steps_down_after_self_removal_committed() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let change_index = node
        .propose_membership_change(
            MembershipChange::RemoveServer(nid(1)),
            "remove_self".to_owned(),
        )
        .unwrap();

    // Node 1 is no longer in membership, cluster is {2, 3}, quorum = 2.
    assert!(!node.membership().is_member(nid(1)));
    assert_eq!(node.cluster_size(), 2);

    // Simulate majority acknowledging.
    for &peer in &[nid(2), nid(3)] {
        let resp = AppendEntriesResponse {
            term: node.current_term(),
            success: true,
            match_index: change_index,
        };
        node.handle_append_entries_response(peer, resp)
            .await
            .unwrap();
    }

    // Leader should have stepped down.
    assert_eq!(node.state(), RaftState::Follower);
    assert!(node.leader_state().is_none());
}

// ---- Rollback on term change ----

#[tokio::test]
async fn pending_change_rolled_back_on_term_update() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "add_node4".to_owned(),
    )
    .unwrap();

    assert!(node.membership().is_member(nid(4)));
    assert!(node.membership().has_pending_change());

    // Receive a higher term, causing step-down.
    let higher_term = node.current_term() + 1;
    node.handle_term_update(higher_term).await.unwrap();

    // Membership change should be rolled back.
    assert!(!node.membership().is_member(nid(4)));
    assert!(!node.membership().has_pending_change());
    assert_eq!(node.cluster_size(), 3);
}

// ---- Quorum calculations with membership changes ----

#[tokio::test]
async fn quorum_uses_new_membership_for_add() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    // 3-node cluster: quorum = 2
    assert_eq!(node.quorum_size(), 2);

    node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "add_node4".to_owned(),
    )
    .unwrap();

    // 4-node cluster: quorum = 3
    assert_eq!(node.quorum_size(), 3);
}

#[tokio::test]
async fn quorum_uses_new_membership_for_remove() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    // 3-node cluster: quorum = 2
    assert_eq!(node.quorum_size(), 2);

    node.propose_membership_change(
        MembershipChange::RemoveServer(nid(3)),
        "remove_node3".to_owned(),
    )
    .unwrap();

    // 2-node cluster: quorum = 2
    assert_eq!(node.quorum_size(), 2);
}

// ---- Leader state updated for new members ----

#[tokio::test]
async fn leader_state_includes_new_member() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    node.propose_membership_change(
        MembershipChange::AddServer(nid(4), addr(4)),
        "add_node4".to_owned(),
    )
    .unwrap();

    let leader = node.leader_state().unwrap();
    assert!(leader.next_index.contains_key(&nid(4)));
    assert!(leader.match_index.contains_key(&nid(4)));
    assert_eq!(leader.match_index[&nid(4)], LogIndex::new(0));
}

// ---- Sequential membership changes ----

#[tokio::test]
async fn sequential_membership_changes() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    // Add node 4.
    let idx1 = node
        .propose_membership_change(
            MembershipChange::AddServer(nid(4), addr(4)),
            "add_node4".to_owned(),
        )
        .unwrap();

    // Commit via majority (quorum=3 in 4-node cluster: need leader + 2 peers).
    for &peer in &[nid(2), nid(3)] {
        let resp = AppendEntriesResponse {
            term: node.current_term(),
            success: true,
            match_index: idx1,
        };
        node.handle_append_entries_response(peer, resp)
            .await
            .unwrap();
    }
    assert!(!node.membership().has_pending_change());
    assert_eq!(node.membership().generation(), 1);

    // Now remove node 3.
    let idx2 = node
        .propose_membership_change(
            MembershipChange::RemoveServer(nid(3)),
            "remove_node3".to_owned(),
        )
        .unwrap();

    // Commit via majority (quorum=2 in 3-node cluster {1,2,4}: need leader + 1).
    let resp = AppendEntriesResponse {
        term: node.current_term(),
        success: true,
        match_index: idx2,
    };
    node.handle_append_entries_response(nid(2), resp)
        .await
        .unwrap();

    assert!(!node.membership().has_pending_change());
    assert_eq!(node.membership().generation(), 2);
    assert!(!node.membership().is_member(nid(3)));
    assert!(node.membership().is_member(nid(4)));
    assert_eq!(node.cluster_size(), 3);
}

// ---- Removed member cleaned from leader state ----

#[tokio::test]
async fn removed_member_cleaned_from_leader_state() {
    let (mut node, _rx) = make_leader(1, vec![nid(2), nid(3)]).await;

    let idx = node
        .propose_membership_change(
            MembershipChange::RemoveServer(nid(3)),
            "remove_node3".to_owned(),
        )
        .unwrap();

    // Commit.
    let resp = AppendEntriesResponse {
        term: node.current_term(),
        success: true,
        match_index: idx,
    };
    node.handle_append_entries_response(nid(2), resp)
        .await
        .unwrap();

    let leader = node.leader_state().unwrap();
    assert!(!leader.next_index.contains_key(&nid(3)));
    assert!(!leader.match_index.contains_key(&nid(3)));
}

// ---- has_committed_in_current_term ----

#[tokio::test]
async fn has_committed_in_current_term_initially_false() {
    let config = test_config(1, vec![nid(2), nid(3)]);
    let storage = MemStorage::new();
    let (node, _rx) = RaftNode::<String, MemStorage>::new(config, storage)
        .await
        .unwrap();

    assert!(!node.has_committed_in_current_term());
}
