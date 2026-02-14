use std::collections::HashSet;
use std::sync::Mutex;

use crate::raft::node::RaftNode;
use crate::raft::rpc::{
    AppendEntries, AppendEntriesResponse, RaftMessage, RequestVote, RequestVoteResponse,
};
use crate::raft::storage::{RaftStorage, StorageResult};
use crate::raft::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

/// In-memory storage backend for testing.
struct MemStorage {
    term: Mutex<Term>,
    voted_for: Mutex<Option<NodeId>>,
    log: Mutex<Vec<LogEntry<String>>>,
    snapshot: Mutex<Option<(Vec<u8>, LogIndex, Term)>>,
}

impl MemStorage {
    fn new() -> Self {
        Self {
            term: Mutex::new(Term::default()),
            voted_for: Mutex::new(None),
            log: Mutex::new(Vec::new()),
            snapshot: Mutex::new(None),
        }
    }
}

impl RaftStorage<String> for MemStorage {
    fn save_term(
        &self,
        term: Term,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        *self.term.lock().unwrap() = term;
        async { Ok(()) }
    }

    fn load_term(&self) -> impl std::future::Future<Output = StorageResult<Term>> + Send {
        let term = *self.term.lock().unwrap();
        async move { Ok(term) }
    }

    fn save_voted_for(
        &self,
        node: Option<NodeId>,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        *self.voted_for.lock().unwrap() = node;
        async { Ok(()) }
    }

    fn load_voted_for(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Option<NodeId>>> + Send {
        let voted = *self.voted_for.lock().unwrap();
        async move { Ok(voted) }
    }

    fn append_entries(
        &self,
        entries: &[LogEntry<String>],
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        self.log.lock().unwrap().extend(entries.iter().cloned());
        async { Ok(()) }
    }

    fn load_log(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Vec<LogEntry<String>>>> + Send {
        let log = self.log.lock().unwrap().clone();
        async move { Ok(log) }
    }

    fn truncate_log_from(
        &self,
        index: LogIndex,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        let i = index.as_u64() as usize;
        let mut log = self.log.lock().unwrap();
        if i > 0 && i <= log.len() {
            log.truncate(i - 1);
        }
        async { Ok(()) }
    }

    fn save_snapshot(
        &self,
        snapshot: &[u8],
        last_included_index: LogIndex,
        last_included_term: Term,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        *self.snapshot.lock().unwrap() =
            Some((snapshot.to_vec(), last_included_index, last_included_term));
        async { Ok(()) }
    }

    fn load_snapshot(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Option<(Vec<u8>, LogIndex, Term)>>>
           + Send {
        let snap = self.snapshot.lock().unwrap().clone();
        async move { Ok(snap) }
    }
}

fn test_config(id: u64, peers: Vec<u64>) -> RaftConfig {
    RaftConfig {
        node_id: NodeId::new(id),
        peers: peers.into_iter().map(NodeId::new).collect(),
        ..RaftConfig::default()
    }
}

// ---- Construction ----

#[tokio::test]
async fn new_node_starts_as_follower() {
    let config = test_config(1, vec![2, 3]);
    let (node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_term(), Term::default());
    assert_eq!(node.voted_for(), None);
    assert_eq!(node.current_leader(), None);
    assert_eq!(node.cluster_size(), 3);
    assert_eq!(node.quorum_size(), 2);
}

#[tokio::test]
async fn solo_node_cluster_size() {
    let config = test_config(1, vec![]);
    let (node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.cluster_size(), 1);
    assert_eq!(node.quorum_size(), 1);
}

#[tokio::test]
async fn five_node_quorum() {
    let config = test_config(1, vec![2, 3, 4, 5]);
    let (node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.cluster_size(), 5);
    assert_eq!(node.quorum_size(), 3);
}

// ---- Term management ----

#[tokio::test]
async fn handle_term_update_higher_term_steps_down() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Artificially set node to Candidate.
    node.state = RaftState::Candidate;
    node.current_term = Term::new(3);

    let stepped = node.handle_term_update(Term::new(5)).await.unwrap();
    assert!(stepped);
    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_term(), Term::new(5));
    assert_eq!(node.voted_for(), None);
}

#[tokio::test]
async fn handle_term_update_equal_term_no_change() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.current_term = Term::new(3);
    node.state = RaftState::Leader;

    let stepped = node.handle_term_update(Term::new(3)).await.unwrap();
    assert!(!stepped);
    assert_eq!(node.state(), RaftState::Leader);
}

#[tokio::test]
async fn handle_term_update_lower_term_no_change() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.current_term = Term::new(5);
    let stepped = node.handle_term_update(Term::new(3)).await.unwrap();
    assert!(!stepped);
    assert_eq!(node.current_term(), Term::new(5));
}

// ---- Election: solo cluster ----

#[tokio::test]
async fn solo_node_becomes_leader_immediately() {
    let config = test_config(1, vec![]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();

    assert_eq!(node.state(), RaftState::Leader);
    assert_eq!(node.current_term(), Term::new(1));
    assert_eq!(node.voted_for(), Some(NodeId::new(1)));
    assert_eq!(node.current_leader(), Some(NodeId::new(1)));
}

// ---- Election: multi-node ----

#[tokio::test]
async fn start_election_transitions_to_candidate() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, mut rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();

    assert_eq!(node.state(), RaftState::Candidate);
    assert_eq!(node.current_term(), Term::new(1));
    assert_eq!(node.voted_for(), Some(NodeId::new(1)));
    assert!(node.votes_received().contains(&NodeId::new(1)));

    // Should have sent RequestVote to both peers.
    let mut targets = HashSet::new();
    while let Ok((target, msg)) = rx.try_recv() {
        targets.insert(target);
        assert!(matches!(msg, RaftMessage::RequestVote(_)));
    }
    assert!(targets.contains(&NodeId::new(2)));
    assert!(targets.contains(&NodeId::new(3)));
}

#[tokio::test]
async fn receiving_majority_votes_becomes_leader() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, mut rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();

    // Drain the RequestVote messages.
    while rx.try_recv().is_ok() {}

    // One vote from peer 2 should be enough (self + peer2 = 2 >= quorum of 2).
    let resp = RequestVoteResponse {
        term: Term::new(1),
        vote_granted: true,
    };
    node.handle_request_vote_response(NodeId::new(2), resp)
        .await
        .unwrap();

    assert_eq!(node.state(), RaftState::Leader);
    assert_eq!(node.current_leader(), Some(NodeId::new(1)));
    assert!(node.leader_state().is_some());

    // Should have sent heartbeats to peers after becoming leader.
    let mut heartbeats = Vec::new();
    while let Ok((target, msg)) = rx.try_recv() {
        if matches!(msg, RaftMessage::AppendEntries(_)) {
            heartbeats.push(target);
        }
    }
    assert!(heartbeats.contains(&NodeId::new(2)));
    assert!(heartbeats.contains(&NodeId::new(3)));
}

#[tokio::test]
async fn rejected_votes_dont_become_leader() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();

    let resp = RequestVoteResponse {
        term: Term::new(1),
        vote_granted: false,
    };
    node.handle_request_vote_response(NodeId::new(2), resp.clone())
        .await
        .unwrap();
    node.handle_request_vote_response(NodeId::new(3), resp)
        .await
        .unwrap();

    assert_eq!(node.state(), RaftState::Candidate);
}

// ---- RequestVote handling ----

#[tokio::test]
async fn grant_vote_when_not_voted() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let req = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(resp.vote_granted);
    assert_eq!(node.voted_for(), Some(NodeId::new(2)));
}

#[tokio::test]
async fn reject_vote_when_already_voted() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Vote for node 2 first.
    let req1 = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp1 = node.handle_request_vote(req1).await.unwrap();
    assert!(resp1.vote_granted);

    // Node 3 asks for a vote in the same term — should be rejected.
    let req2 = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(3),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp2 = node.handle_request_vote(req2).await.unwrap();
    assert!(!resp2.vote_granted);
    assert_eq!(node.voted_for(), Some(NodeId::new(2)));
}

#[tokio::test]
async fn grant_vote_again_to_same_candidate() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let req = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    node.handle_request_vote(req.clone()).await.unwrap();
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(resp.vote_granted);
}

#[tokio::test]
async fn reject_vote_stale_term() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.current_term = Term::new(5);
    node.storage.save_term(Term::new(5)).await.unwrap();

    let req = RequestVote {
        term: Term::new(3),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(!resp.vote_granted);
    assert_eq!(resp.term, Term::new(5));
}

#[tokio::test]
async fn reject_vote_outdated_log() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Give our node some log entries.
    node.log.append(LogEntry {
        term: Term::new(2),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });

    // Candidate has an older log (term 1 < our term 2).
    let req = RequestVote {
        term: Term::new(3),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(1),
        last_log_term: Term::new(1),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(!resp.vote_granted);
}

#[tokio::test]
async fn grant_vote_with_more_up_to_date_log() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });

    // Candidate has a higher last_log_term.
    let req = RequestVote {
        term: Term::new(2),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(1),
        last_log_term: Term::new(2),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(resp.vote_granted);
}

#[tokio::test]
async fn grant_vote_equal_log_longer_index() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });

    // Same term, longer log.
    let req = RequestVote {
        term: Term::new(2),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(5),
        last_log_term: Term::new(1),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(resp.vote_granted);
}

#[tokio::test]
async fn reject_vote_equal_term_shorter_log() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });
    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(2),
        command: "cmd2".to_owned(),
    });

    // Same term but shorter log.
    let req = RequestVote {
        term: Term::new(2),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(1),
        last_log_term: Term::new(1),
    };
    let resp = node.handle_request_vote(req).await.unwrap();
    assert!(!resp.vote_granted);
}

// ---- AppendEntries handling ----

#[tokio::test]
async fn append_entries_rejects_stale_term() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.current_term = Term::new(5);

    let req: AppendEntries<String> = AppendEntries {
        term: Term::new(3),
        leader_id: NodeId::new(2),
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::new(0),
        entries: vec![],
        leader_commit: LogIndex::new(0),
    };
    let resp = node.handle_append_entries(req).await.unwrap();
    assert!(!resp.success);
    assert_eq!(resp.term, Term::new(5));
}

#[tokio::test]
async fn append_entries_heartbeat_resets_follower() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Make node a candidate first.
    node.start_election().await.unwrap();
    assert_eq!(node.state(), RaftState::Candidate);

    // Receive heartbeat from a leader with same term.
    let req: AppendEntries<String> = AppendEntries {
        term: Term::new(1),
        leader_id: NodeId::new(2),
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::new(0),
        entries: vec![],
        leader_commit: LogIndex::new(0),
    };
    let resp = node.handle_append_entries(req).await.unwrap();
    assert!(resp.success);
    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_leader(), Some(NodeId::new(2)));
}

#[tokio::test]
async fn append_entries_updates_commit_index() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd".to_owned(),
    });

    let req: AppendEntries<String> = AppendEntries {
        term: Term::new(1),
        leader_id: NodeId::new(2),
        prev_log_index: LogIndex::new(1),
        prev_log_term: Term::new(1),
        entries: vec![],
        leader_commit: LogIndex::new(1),
    };
    let resp = node.handle_append_entries(req).await.unwrap();
    assert!(resp.success);
    assert_eq!(node.volatile_state().commit_index, LogIndex::new(1));
}

// ---- Election timeout (deterministic succession) ----

#[tokio::test]
async fn election_timeout_lowest_id_gets_shortest() {
    let config = test_config(1, vec![2, 3]);
    let (node1, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let config2 = test_config(2, vec![1, 3]);
    let (node2, _rx) = RaftNode::<String, _>::new(config2, MemStorage::new())
        .await
        .unwrap();

    let config3 = test_config(3, vec![1, 2]);
    let (node3, _rx) = RaftNode::<String, _>::new(config3, MemStorage::new())
        .await
        .unwrap();

    let t1 = node1.election_timeout();
    let t2 = node2.election_timeout();
    let t3 = node3.election_timeout();

    // Node 1 (lowest ID) should have shortest timeout.
    assert!(t1 < t2);
    assert!(t2 < t3);

    // All should be within [min, max].
    assert!(t1 >= node1.config().election_timeout_min);
    assert!(t3 <= node3.config().election_timeout_max);
}

#[tokio::test]
async fn election_timeout_solo_gets_min() {
    let config = test_config(1, vec![]);
    let (node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.election_timeout(), node.config().election_timeout_min);
}

// ---- Three-node election simulation ----

#[tokio::test]
async fn three_node_election_simulation() {
    // Create 3 nodes.
    let cfg1 = test_config(1, vec![2, 3]);
    let cfg2 = test_config(2, vec![1, 3]);
    let cfg3 = test_config(3, vec![1, 2]);

    let (mut node1, mut rx1) =
        RaftNode::<String, _>::new(cfg1, MemStorage::new()).await.unwrap();
    let (mut node2, _rx2) =
        RaftNode::<String, _>::new(cfg2, MemStorage::new()).await.unwrap();
    let (mut node3, _rx3) =
        RaftNode::<String, _>::new(cfg3, MemStorage::new()).await.unwrap();

    // Node 1 starts an election.
    node1.start_election().await.unwrap();
    assert_eq!(node1.state(), RaftState::Candidate);

    // Collect RequestVote messages from node 1.
    let mut vote_requests = Vec::new();
    while let Ok((target, msg)) = rx1.try_recv() {
        if let RaftMessage::RequestVote(rv) = msg {
            vote_requests.push((target, rv));
        }
    }
    assert_eq!(vote_requests.len(), 2);

    // Node 2 and 3 process the RequestVote.
    let (_, rv) = &vote_requests[0];
    let resp2 = node2.handle_request_vote(rv.clone()).await.unwrap();
    let resp3 = node3.handle_request_vote(rv.clone()).await.unwrap();
    assert!(resp2.vote_granted);
    assert!(resp3.vote_granted);

    // Node 1 processes the vote responses.
    node1
        .handle_request_vote_response(NodeId::new(2), resp2)
        .await
        .unwrap();

    // After getting vote from node 2, node 1 has majority (2/3).
    assert_eq!(node1.state(), RaftState::Leader);
    assert_eq!(node1.current_leader(), Some(NodeId::new(1)));
}

// ---- Leader stepping down ----

#[tokio::test]
async fn leader_steps_down_on_higher_term() {
    let config = test_config(1, vec![]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();
    assert_eq!(node.state(), RaftState::Leader);

    // Receive a term higher than ours.
    node.handle_term_update(Term::new(5)).await.unwrap();
    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_term(), Term::new(5));
    assert_eq!(node.voted_for(), None);
}

#[tokio::test]
async fn candidate_steps_down_on_higher_term() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();
    assert_eq!(node.state(), RaftState::Candidate);

    let resp = RequestVoteResponse {
        term: Term::new(10),
        vote_granted: false,
    };
    node.handle_request_vote_response(NodeId::new(2), resp)
        .await
        .unwrap();
    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_term(), Term::new(10));
}

// ---- Vote granted in new term clears old vote ----

#[tokio::test]
async fn new_term_allows_new_vote() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Vote for node 2 in term 1.
    let req1 = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(2),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp1 = node.handle_request_vote(req1).await.unwrap();
    assert!(resp1.vote_granted);

    // New term 2 from node 3 — should be able to vote.
    let req2 = RequestVote {
        term: Term::new(2),
        candidate_id: NodeId::new(3),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    let resp2 = node.handle_request_vote(req2).await.unwrap();
    assert!(resp2.vote_granted);
    assert_eq!(node.voted_for(), Some(NodeId::new(3)));
    assert_eq!(node.current_term(), Term::new(2));
}

// ---- AppendEntriesResponse handling ----

#[tokio::test]
async fn append_entries_response_updates_leader_state() {
    let config = test_config(1, vec![2]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Make node leader.
    node.start_election().await.unwrap();

    // Peer 2 is missing from solo config, add it for this test.
    // Actually let's redo with proper config.
    let config2 = test_config(1, vec![2, 3]);
    let (mut leader, mut rx) = RaftNode::<String, _>::new(config2, MemStorage::new())
        .await
        .unwrap();

    leader.start_election().await.unwrap();
    // Drain RequestVotes.
    while rx.try_recv().is_ok() {}

    // Get one vote to become leader.
    let vote = RequestVoteResponse {
        term: Term::new(1),
        vote_granted: true,
    };
    leader
        .handle_request_vote_response(NodeId::new(2), vote)
        .await
        .unwrap();
    assert_eq!(leader.state(), RaftState::Leader);

    // Drain heartbeats.
    while rx.try_recv().is_ok() {}

    // Simulate successful AppendEntriesResponse.
    let resp = AppendEntriesResponse {
        term: Term::new(1),
        success: true,
        match_index: LogIndex::new(0),
    };
    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();

    let ls = leader.leader_state().unwrap();
    assert_eq!(ls.match_index[&NodeId::new(2)], LogIndex::new(0));
    assert_eq!(ls.next_index[&NodeId::new(2)], LogIndex::new(1));
}

#[tokio::test]
async fn append_entries_response_failure_decrements_next_index() {
    let config = test_config(1, vec![2, 3]);
    let (mut leader, mut rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Add a log entry so next_index starts at 2.
    leader.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd".to_owned(),
    });

    leader.start_election().await.unwrap();
    // After start_election, node is in term 1 (0 + 1).
    let election_term = leader.current_term();
    while rx.try_recv().is_ok() {}

    let vote = RequestVoteResponse {
        term: election_term,
        vote_granted: true,
    };
    leader
        .handle_request_vote_response(NodeId::new(2), vote)
        .await
        .unwrap();
    assert_eq!(leader.state(), RaftState::Leader);
    while rx.try_recv().is_ok() {}

    // Leader state should have next_index = 2 for peer 2.
    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(2)
    );

    // Failed response should decrement.
    let resp = AppendEntriesResponse {
        term: election_term,
        success: false,
        match_index: LogIndex::new(0),
    };
    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();

    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(1)
    );
}

