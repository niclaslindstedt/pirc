use std::sync::Mutex;

use crate::raft::node::RaftNode;
use crate::raft::rpc::{
    AppendEntries, AppendEntriesResponse, RaftMessage, RequestVoteResponse,
};
use crate::raft::snapshot::{InstallSnapshotResponse, NullStateMachine, Snapshot};
use crate::raft::storage::{RaftStorage, StorageResult};
use crate::raft::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

/// In-memory storage backend for testing (same pattern as node_tests).
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

fn entry(term: u64, index: u64, cmd: &str) -> LogEntry<String> {
    LogEntry {
        term: Term::new(term),
        index: LogIndex::new(index),
        command: cmd.to_owned(),
    }
}

/// Helper: create a leader node for a cluster.
///
/// Gathers enough votes from peers to reach quorum.
async fn make_leader(
    id: u64,
    peers: Vec<u64>,
) -> (
    RaftNode<String, MemStorage>,
    tokio::sync::mpsc::UnboundedReceiver<(NodeId, RaftMessage<String>)>,
) {
    let config = test_config(id, peers.clone());
    let (mut node, mut rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();
    // Drain RequestVotes.
    while rx.try_recv().is_ok() {}

    if !peers.is_empty() {
        // Collect enough votes to reach quorum (self already voted).
        let quorum = node.quorum_size();
        let votes_needed = quorum - 1; // self already counted.
        for &peer_id in peers.iter().take(votes_needed) {
            let vote = RequestVoteResponse {
                term: node.current_term(),
                vote_granted: true,
            };
            node.handle_request_vote_response(NodeId::new(peer_id), vote)
                .await
                .unwrap();
        }
        // Drain heartbeats sent after becoming leader.
        while rx.try_recv().is_ok() {}
    }

    assert_eq!(node.state(), RaftState::Leader);
    (node, rx)
}

// ---- replicate_to_peer ----

#[tokio::test]
async fn replicate_to_peer_sends_empty_when_no_entries() {
    let (leader, mut rx) = make_leader(1, vec![2, 3]).await;

    leader.replicate_to_peer(NodeId::new(2));

    let (target, msg) = rx.try_recv().unwrap();
    assert_eq!(target, NodeId::new(2));
    match msg {
        RaftMessage::AppendEntries(ae) => {
            assert_eq!(ae.term, leader.current_term());
            assert_eq!(ae.leader_id, NodeId::new(1));
            assert!(ae.entries.is_empty());
        }
        _ => panic!("expected AppendEntries"),
    }
}

#[tokio::test]
async fn replicate_to_peer_sends_entries() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    // Add an entry to the leader's log.
    leader.log.append(entry(leader.current_term().as_u64(), 1, "cmd1"));

    leader.replicate_to_peer(NodeId::new(2));

    let (target, msg) = rx.try_recv().unwrap();
    assert_eq!(target, NodeId::new(2));
    match msg {
        RaftMessage::AppendEntries(ae) => {
            assert_eq!(ae.entries.len(), 1);
            assert_eq!(ae.entries[0].command, "cmd1");
            assert_eq!(ae.prev_log_index, LogIndex::new(0));
            assert_eq!(ae.prev_log_term, Term::default());
        }
        _ => panic!("expected AppendEntries"),
    }
}

#[tokio::test]
async fn replicate_to_peer_with_advanced_next_index() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));
    leader.log.append(entry(term, 3, "cmd3"));

    // Manually set next_index for peer 2 to 3 (they already have entries 1-2).
    if let Some(ref mut ls) = leader.leader_state {
        ls.next_index.insert(NodeId::new(2), LogIndex::new(3));
        ls.match_index.insert(NodeId::new(2), LogIndex::new(2));
    }

    leader.replicate_to_peer(NodeId::new(2));

    let (_, msg) = rx.try_recv().unwrap();
    match msg {
        RaftMessage::AppendEntries(ae) => {
            assert_eq!(ae.entries.len(), 1);
            assert_eq!(ae.entries[0].command, "cmd3");
            assert_eq!(ae.prev_log_index, LogIndex::new(2));
            assert_eq!(ae.prev_log_term, Term::new(term));
        }
        _ => panic!("expected AppendEntries"),
    }
}

#[tokio::test]
async fn replicate_to_peer_not_leader_is_noop() {
    let config = test_config(1, vec![2, 3]);
    let (node, mut rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.state(), RaftState::Follower);
    node.replicate_to_peer(NodeId::new(2));

    // No message should be sent.
    assert!(rx.try_recv().is_err());
}

// ---- send_append_entries_to_all ----

#[tokio::test]
async fn send_append_entries_to_all_sends_to_all_peers() {
    let (leader, mut rx) = make_leader(1, vec![2, 3]).await;

    leader.send_append_entries_to_all();

    let mut targets = std::collections::HashSet::new();
    while let Ok((target, msg)) = rx.try_recv() {
        assert!(matches!(msg, RaftMessage::AppendEntries(_)));
        targets.insert(target);
    }
    assert!(targets.contains(&NodeId::new(2)));
    assert!(targets.contains(&NodeId::new(3)));
}

// ---- send_heartbeats delegates to send_append_entries_to_all ----

#[tokio::test]
async fn send_heartbeats_sends_entries_to_all_peers() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    leader.send_heartbeats();

    let mut count = 0;
    while let Ok((_, msg)) = rx.try_recv() {
        match msg {
            RaftMessage::AppendEntries(ae) => {
                // Should include the entry since next_index starts at 1.
                assert_eq!(ae.entries.len(), 1);
                assert_eq!(ae.entries[0].command, "cmd1");
                count += 1;
            }
            _ => panic!("expected AppendEntries"),
        }
    }
    assert_eq!(count, 2); // Two peers.
}

// ---- advance_commit_index ----

#[tokio::test]
async fn advance_commit_index_no_entries() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    leader.advance_commit_index();
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(0));
}

#[tokio::test]
async fn advance_commit_index_majority_replicated() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));

    // Peer 2 has replicated up to index 2.
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(2));
    }

    leader.advance_commit_index();

    // Leader (index 2) + peer 2 (index 2) = 2 >= quorum(2).
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(2));
}

#[tokio::test]
async fn advance_commit_index_not_enough_replicas() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3, 4, 5]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    // Only peer 2 has replicated. Leader + peer 2 = 2 < quorum(3).
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(1));
    }

    leader.advance_commit_index();
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(0));
}

#[tokio::test]
async fn advance_commit_index_only_current_term() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    // Entry from a previous term — should NOT be committed even with majority.
    leader.log.append(entry(term - 1, 1, "old_cmd"));

    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(1));
    }

    leader.advance_commit_index();
    // Commit index should NOT advance because entry is from a previous term.
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(0));
}

#[tokio::test]
async fn advance_commit_index_current_term_commits_older_entries() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    // Entry from previous term.
    leader.log.append(entry(term - 1, 1, "old_cmd"));
    // Entry from current term.
    leader.log.append(entry(term, 2, "new_cmd"));

    // Peer 2 has replicated both.
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(2));
    }

    leader.advance_commit_index();
    // Should commit up to index 2 (the current-term entry), which indirectly
    // commits the older entry at index 1.
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(2));
}

#[tokio::test]
async fn advance_commit_index_finds_highest() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));
    leader.log.append(entry(term, 3, "cmd3"));

    // Peer 2 has replicated all 3, peer 3 only has 1.
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(3));
        ls.match_index.insert(NodeId::new(3), LogIndex::new(1));
    }

    leader.advance_commit_index();
    // For index 3: leader + peer 2 = 2 >= quorum(2). So commit index = 3.
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(3));
}

#[tokio::test]
async fn advance_commit_index_not_leader_is_noop() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.advance_commit_index();
    assert_eq!(node.volatile_state().commit_index, LogIndex::new(0));
}

// ---- apply_committed ----

#[tokio::test]
async fn apply_committed_applies_entries() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));

    // Set commit index to 2 manually.
    leader.volatile.commit_index = LogIndex::new(2);

    let mut applied_cmds = Vec::new();
    let results = leader.apply_committed(|cmd| {
        applied_cmds.push(cmd.clone());
    });

    assert_eq!(applied_cmds, vec!["cmd1", "cmd2"]);
    assert_eq!(results, vec!["cmd1", "cmd2"]);
    assert_eq!(leader.volatile_state().last_applied, LogIndex::new(2));
}

#[tokio::test]
async fn apply_committed_nothing_to_apply() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    // commit_index == last_applied == 0, nothing to apply.
    let results = leader.apply_committed(|_| {
        panic!("should not be called");
    });

    assert!(results.is_empty());
    assert_eq!(leader.volatile_state().last_applied, LogIndex::new(0));
}

#[tokio::test]
async fn apply_committed_incremental() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));
    leader.log.append(entry(term, 3, "cmd3"));

    // First batch: commit up to 2.
    leader.volatile.commit_index = LogIndex::new(2);
    let results1 = leader.apply_committed(|_| {});
    assert_eq!(results1, vec!["cmd1", "cmd2"]);
    assert_eq!(leader.volatile_state().last_applied, LogIndex::new(2));

    // Second batch: commit up to 3.
    leader.volatile.commit_index = LogIndex::new(3);
    let results2 = leader.apply_committed(|_| {});
    assert_eq!(results2, vec!["cmd3"]);
    assert_eq!(leader.volatile_state().last_applied, LogIndex::new(3));
}

// ---- client_request ----

#[tokio::test]
async fn client_request_appends_and_replicates() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let idx = leader.client_request("my_command".to_owned());
    assert_eq!(idx, Some(LogIndex::new(1)));
    assert_eq!(leader.log().len(), 1);
    assert_eq!(leader.log().get(LogIndex::new(1)).unwrap().command, "my_command");
    assert_eq!(
        leader.log().get(LogIndex::new(1)).unwrap().term,
        leader.current_term()
    );

    // Should have replicated to both peers.
    let mut count = 0;
    while let Ok((_, msg)) = rx.try_recv() {
        match msg {
            RaftMessage::AppendEntries(ae) => {
                assert_eq!(ae.entries.len(), 1);
                assert_eq!(ae.entries[0].command, "my_command");
                count += 1;
            }
            _ => panic!("expected AppendEntries"),
        }
    }
    assert_eq!(count, 2);
}

#[tokio::test]
async fn client_request_not_leader_returns_none() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    assert_eq!(node.client_request("cmd".to_owned()), None);
}

#[tokio::test]
async fn client_request_multiple_entries() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let idx1 = leader.client_request("cmd1".to_owned());
    let idx2 = leader.client_request("cmd2".to_owned());
    let idx3 = leader.client_request("cmd3".to_owned());

    assert_eq!(idx1, Some(LogIndex::new(1)));
    assert_eq!(idx2, Some(LogIndex::new(2)));
    assert_eq!(idx3, Some(LogIndex::new(3)));
    assert_eq!(leader.log().len(), 3);

    // Drain all messages. Each client_request sends 2 messages (one per peer).
    let mut msg_count = 0;
    while rx.try_recv().is_ok() {
        msg_count += 1;
    }
    assert_eq!(msg_count, 6); // 3 requests * 2 peers.
}

// ---- Full replication flow ----

#[tokio::test]
async fn full_replication_flow_three_nodes() {
    // Setup: node 1 = leader, nodes 2 and 3 = followers.
    let cfg1 = test_config(1, vec![2, 3]);
    let cfg2 = test_config(2, vec![1, 3]);
    let cfg3 = test_config(3, vec![1, 2]);

    let (mut node1, mut rx1) = RaftNode::<String, _>::new(cfg1, MemStorage::new())
        .await
        .unwrap();
    let (mut node2, _rx2) = RaftNode::<String, _>::new(cfg2, MemStorage::new())
        .await
        .unwrap();
    let (mut node3, _rx3) = RaftNode::<String, _>::new(cfg3, MemStorage::new())
        .await
        .unwrap();

    // Node 1 starts election and wins.
    node1.start_election().await.unwrap();
    while rx1.try_recv().is_ok() {} // Drain RequestVotes.

    let vote = RequestVoteResponse {
        term: node1.current_term(),
        vote_granted: true,
    };
    node1
        .handle_request_vote_response(NodeId::new(2), vote)
        .await
        .unwrap();
    assert_eq!(node1.state(), RaftState::Leader);
    while rx1.try_recv().is_ok() {} // Drain initial heartbeats.

    // Leader receives a client request.
    let idx = node1.client_request("set x 42".to_owned()).unwrap();
    assert_eq!(idx, LogIndex::new(1));

    // Collect AppendEntries messages.
    let mut ae_messages = Vec::new();
    while let Ok((target, msg)) = rx1.try_recv() {
        if let RaftMessage::AppendEntries(ae) = msg {
            ae_messages.push((target, ae));
        }
    }

    // Process AppendEntries on node 2.
    let ae_for_2 = ae_messages
        .iter()
        .find(|(t, _)| *t == NodeId::new(2))
        .map(|(_, ae)| ae.clone())
        .unwrap();
    let resp2 = node2.handle_append_entries(ae_for_2).await.unwrap();
    assert!(resp2.success);
    assert_eq!(resp2.match_index, LogIndex::new(1));
    assert_eq!(node2.log().len(), 1);

    // Process AppendEntries on node 3.
    let ae_for_3 = ae_messages
        .iter()
        .find(|(t, _)| *t == NodeId::new(3))
        .map(|(_, ae)| ae.clone())
        .unwrap();
    let resp3 = node3.handle_append_entries(ae_for_3).await.unwrap();
    assert!(resp3.success);
    assert_eq!(resp3.match_index, LogIndex::new(1));

    // Leader processes response from node 2.
    node1
        .handle_append_entries_response(NodeId::new(2), resp2)
        .await
        .unwrap();

    // After one successful response, we have majority (leader + node 2 = 2 >= 2).
    assert_eq!(node1.volatile_state().commit_index, LogIndex::new(1));

    // Apply committed entries.
    let mut applied = Vec::new();
    node1.apply_committed(|cmd| {
        applied.push(cmd.clone());
    });
    assert_eq!(applied, vec!["set x 42"]);
    assert_eq!(node1.volatile_state().last_applied, LogIndex::new(1));
}

// ---- Log inconsistency backtracking ----

#[tokio::test]
async fn log_backtracking_converges() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));
    leader.log.append(entry(term, 2, "cmd2"));
    leader.log.append(entry(term, 3, "cmd3"));

    // Reinitialize leader state so next_index reflects new log length.
    leader.leader_state = Some(crate::raft::state::LeaderState::new(
        &leader.config().peers.clone(),
        leader.log().last_index(),
    ));

    // Create a follower with conflicting log.
    let cfg2 = test_config(2, vec![1, 3]);
    let (mut follower, _rx2) = RaftNode::<String, _>::new(cfg2, MemStorage::new())
        .await
        .unwrap();

    // Follower has entries from an old term at indices 1-2.
    follower.log.append(entry(term - 1, 1, "old1"));
    follower.log.append(entry(term - 1, 2, "old2"));

    // First replication attempt: leader sends entries starting at index 4
    // (next_index = 4, prev_log_index = 3). Follower doesn't have index 3.
    leader.replicate_to_peer(NodeId::new(2));
    let (_, msg) = rx.try_recv().unwrap();
    let ae = match msg {
        RaftMessage::AppendEntries(ae) => ae,
        _ => panic!("expected AppendEntries"),
    };
    assert_eq!(ae.prev_log_index, LogIndex::new(3));

    let resp = follower.handle_append_entries(ae).await.unwrap();
    assert!(!resp.success);

    // Leader processes failure, decrements next_index.
    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();
    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(3)
    );

    // Second attempt: prev_log_index = 2, follower has index 2 but wrong term.
    leader.replicate_to_peer(NodeId::new(2));
    let (_, msg) = rx.try_recv().unwrap();
    let ae = match msg {
        RaftMessage::AppendEntries(ae) => ae,
        _ => panic!("expected AppendEntries"),
    };
    assert_eq!(ae.prev_log_index, LogIndex::new(2));

    let resp = follower.handle_append_entries(ae).await.unwrap();
    assert!(!resp.success);

    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();
    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(2)
    );

    // Third attempt: prev_log_index = 1, follower has index 1 but wrong term.
    leader.replicate_to_peer(NodeId::new(2));
    let (_, msg) = rx.try_recv().unwrap();
    let ae = match msg {
        RaftMessage::AppendEntries(ae) => ae,
        _ => panic!("expected AppendEntries"),
    };
    assert_eq!(ae.prev_log_index, LogIndex::new(1));

    let resp = follower.handle_append_entries(ae).await.unwrap();
    assert!(!resp.success);

    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();
    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(1)
    );

    // Fourth attempt: prev_log_index = 0 (start of log), should succeed.
    leader.replicate_to_peer(NodeId::new(2));
    let (_, msg) = rx.try_recv().unwrap();
    let ae = match msg {
        RaftMessage::AppendEntries(ae) => ae,
        _ => panic!("expected AppendEntries"),
    };
    assert_eq!(ae.prev_log_index, LogIndex::new(0));
    assert_eq!(ae.entries.len(), 3);

    let resp = follower.handle_append_entries(ae).await.unwrap();
    assert!(resp.success);
    assert_eq!(resp.match_index, LogIndex::new(3));

    // Follower should now have the leader's log.
    assert_eq!(follower.log().len(), 3);
    assert_eq!(follower.log().get(LogIndex::new(1)).unwrap().command, "cmd1");
    assert_eq!(follower.log().get(LogIndex::new(2)).unwrap().command, "cmd2");
    assert_eq!(follower.log().get(LogIndex::new(3)).unwrap().command, "cmd3");

    // Leader updates match_index and next_index.
    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();
    assert_eq!(
        leader.leader_state().unwrap().match_index[&NodeId::new(2)],
        LogIndex::new(3)
    );
    assert_eq!(
        leader.leader_state().unwrap().next_index[&NodeId::new(2)],
        LogIndex::new(4)
    );
}

// ---- Heartbeat prevents election ----

#[tokio::test]
async fn heartbeat_resets_follower_state() {
    let config = test_config(2, vec![1, 3]);
    let (mut follower, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Follower becomes a candidate.
    follower.start_election().await.unwrap();
    assert_eq!(follower.state(), RaftState::Candidate);

    // Heartbeat from a leader in the same term resets to follower.
    let hb: AppendEntries<String> = AppendEntries {
        term: follower.current_term(),
        leader_id: NodeId::new(1),
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::default(),
        entries: vec![],
        leader_commit: LogIndex::new(0),
    };
    let resp = follower.handle_append_entries(hb).await.unwrap();
    assert!(resp.success);
    assert_eq!(follower.state(), RaftState::Follower);
    assert_eq!(follower.current_leader(), Some(NodeId::new(1)));
}

// ---- Heartbeat carries leader_commit ----

#[tokio::test]
async fn heartbeat_updates_follower_commit_index() {
    let config = test_config(2, vec![1, 3]);
    let (mut follower, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Add entries to follower's log.
    follower.log.append(entry(1, 1, "cmd1"));
    follower.log.append(entry(1, 2, "cmd2"));

    let hb: AppendEntries<String> = AppendEntries {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        prev_log_index: LogIndex::new(2),
        prev_log_term: Term::new(1),
        entries: vec![],
        leader_commit: LogIndex::new(2),
    };
    let resp = follower.handle_append_entries(hb).await.unwrap();
    assert!(resp.success);
    assert_eq!(follower.volatile_state().commit_index, LogIndex::new(2));
}

// ---- Commit index clamped to log end ----

#[tokio::test]
async fn commit_index_clamped_to_last_log_index() {
    let config = test_config(2, vec![1, 3]);
    let (mut follower, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    follower.log.append(entry(1, 1, "cmd1"));

    // Leader says commit_index=5 but follower only has 1 entry.
    let hb: AppendEntries<String> = AppendEntries {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        prev_log_index: LogIndex::new(1),
        prev_log_term: Term::new(1),
        entries: vec![],
        leader_commit: LogIndex::new(5),
    };
    let resp = follower.handle_append_entries(hb).await.unwrap();
    assert!(resp.success);
    // Clamped to min(5, 1) = 1.
    assert_eq!(follower.volatile_state().commit_index, LogIndex::new(1));
}

// ---- Five-node commit advancement ----

#[tokio::test]
async fn five_node_commit_requires_three_replicas() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3, 4, 5]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    // Two peers replicated = leader + 2 = 3 >= quorum(3).
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(1));
        ls.match_index.insert(NodeId::new(3), LogIndex::new(1));
    }

    leader.advance_commit_index();
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(1));
}

#[tokio::test]
async fn five_node_commit_fails_with_only_one_replica() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3, 4, 5]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    // Only one peer replicated = leader + 1 = 2 < quorum(3).
    if let Some(ref mut ls) = leader.leader_state {
        ls.match_index.insert(NodeId::new(2), LogIndex::new(1));
    }

    leader.advance_commit_index();
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(0));
}

// ---- Solo leader commits immediately ----

#[tokio::test]
async fn solo_leader_commits_immediately() {
    let (mut leader, _rx) = make_leader(1, vec![]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    // Solo leader: quorum = 1, leader itself counts.
    leader.advance_commit_index();
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(1));
}

// ---- handle_append_entries_response triggers commit advancement ----

#[tokio::test]
async fn response_triggers_commit_advancement() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.log.append(entry(term, 1, "cmd1"));

    // Simulate successful response from peer 2.
    let resp = AppendEntriesResponse {
        term: Term::new(term),
        success: true,
        match_index: LogIndex::new(1),
    };
    leader
        .handle_append_entries_response(NodeId::new(2), resp)
        .await
        .unwrap();

    // Leader + peer 2 = 2 >= quorum(2). Commit should advance.
    assert_eq!(leader.volatile_state().commit_index, LogIndex::new(1));
}

// ---- Snapshot-based replication ----

#[tokio::test]
async fn replicate_sends_snapshot_when_peer_behind_compaction() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    // Add entries 1..=5 and compact to 3.
    for i in 1..=5 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }
    leader.log.compact_to(LogIndex::new(3));

    // Set a snapshot for the leader to send.
    leader.last_snapshot = Some(Snapshot {
        last_included_index: LogIndex::new(3),
        last_included_term: Term::new(term),
        data: b"snapshot-data".to_vec(),
    });

    // Peer 2's next_index is 1, which is below the compaction offset (3).
    // replicate_to_peer should send InstallSnapshot instead of AppendEntries.
    leader.replicate_to_peer(NodeId::new(2));

    let mut got_snapshot = false;
    while let Ok((target, msg)) = rx.try_recv() {
        assert_eq!(target, NodeId::new(2));
        if let RaftMessage::InstallSnapshot(is) = msg {
            got_snapshot = true;
            assert_eq!(is.last_included_index, LogIndex::new(3));
            assert_eq!(is.last_included_term, Term::new(term));
        }
    }
    assert!(got_snapshot, "expected InstallSnapshot message");
}

#[tokio::test]
async fn replicate_sends_append_entries_when_peer_at_offset() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    for i in 1..=5 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }
    leader.log.compact_to(LogIndex::new(3));

    leader.last_snapshot = Some(Snapshot {
        last_included_index: LogIndex::new(3),
        last_included_term: Term::new(term),
        data: Vec::new(),
    });

    // Peer's next_index is at offset + 1 (4), so prev_log_index = 3 = offset.
    // term_at(3) should return snapshot_term and we should send AppendEntries.
    if let Some(ref mut ls) = leader.leader_state {
        ls.next_index.insert(NodeId::new(2), LogIndex::new(4));
    }

    leader.replicate_to_peer(NodeId::new(2));

    let (target, msg) = rx.try_recv().unwrap();
    assert_eq!(target, NodeId::new(2));
    match msg {
        RaftMessage::AppendEntries(ae) => {
            assert_eq!(ae.prev_log_index, LogIndex::new(3));
            assert_eq!(ae.prev_log_term, Term::new(term));
            assert_eq!(ae.entries.len(), 2); // Entries 4 and 5.
        }
        _ => panic!("expected AppendEntries, got {:?}", msg),
    }
}

#[tokio::test]
async fn send_snapshot_to_peer_chunked() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    // Create a snapshot with enough data to require multiple chunks.
    let data = vec![0xAB; 200]; // 200 bytes.
    leader.last_snapshot = Some(Snapshot {
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(term),
        data: data.clone(),
    });

    // Set chunk size to 80 to force multiple chunks.
    leader.config.snapshot_chunk_size = 80;

    // Compact log so replication triggers snapshot sending.
    for i in 1..=10 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }
    leader.log.compact_to(LogIndex::new(10));

    // Peer needs entries from 1 (below offset 10).
    leader.replicate_to_peer(NodeId::new(2));

    // Collect all InstallSnapshot messages.
    let mut chunks = Vec::new();
    while let Ok((target, msg)) = rx.try_recv() {
        assert_eq!(target, NodeId::new(2));
        if let RaftMessage::InstallSnapshot(is) = msg {
            chunks.push(is);
        }
    }

    // With 200 bytes and chunk_size=80, expect 3 chunks: 80+80+40.
    assert_eq!(chunks.len(), 3);

    assert_eq!(chunks[0].offset, 0);
    assert!(!chunks[0].done);
    assert_eq!(chunks[0].data.len(), 80);

    assert_eq!(chunks[1].offset, 80);
    assert!(!chunks[1].done);
    assert_eq!(chunks[1].data.len(), 80);

    assert_eq!(chunks[2].offset, 160);
    assert!(chunks[2].done);
    assert_eq!(chunks[2].data.len(), 40);

    // Reassemble and verify.
    let mut assembled = Vec::new();
    for chunk in &chunks {
        assembled.extend_from_slice(&chunk.data);
    }
    assert_eq!(assembled, data);
}

#[tokio::test]
async fn send_snapshot_empty_data() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.last_snapshot = Some(Snapshot {
        last_included_index: LogIndex::new(5),
        last_included_term: Term::new(term),
        data: Vec::new(), // Empty snapshot.
    });

    for i in 1..=5 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }
    leader.log.compact_to(LogIndex::new(5));

    leader.replicate_to_peer(NodeId::new(2));

    let (_, msg) = rx.try_recv().unwrap();
    match msg {
        RaftMessage::InstallSnapshot(is) => {
            assert!(is.done);
            assert_eq!(is.offset, 0);
            assert!(is.data.is_empty());
        }
        _ => panic!("expected InstallSnapshot"),
    }
}

#[tokio::test]
async fn replicate_no_snapshot_available_is_noop() {
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    for i in 1..=3 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }
    leader.log.compact_to(LogIndex::new(2));

    // No snapshot set, but peer is behind compaction.
    assert!(leader.last_snapshot.is_none());

    leader.replicate_to_peer(NodeId::new(2));

    // Should log a warning but not crash. No snapshot to send.
    // The only message would be from the warning path — no InstallSnapshot.
    let mut snapshot_count = 0;
    while let Ok((_, msg)) = rx.try_recv() {
        if matches!(msg, RaftMessage::InstallSnapshot(_)) {
            snapshot_count += 1;
        }
    }
    assert_eq!(snapshot_count, 0);
}

// ---- handle_install_snapshot_response ----

#[tokio::test]
async fn install_snapshot_response_updates_peer_state() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    leader.last_snapshot = Some(Snapshot {
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(term),
        data: Vec::new(),
    });

    let resp = InstallSnapshotResponse {
        term: Term::new(term),
    };
    leader
        .handle_install_snapshot_response(NodeId::new(2), resp)
        .await
        .unwrap();

    let ls = leader.leader_state().unwrap();
    assert_eq!(ls.next_index[&NodeId::new(2)], LogIndex::new(11));
    assert_eq!(ls.match_index[&NodeId::new(2)], LogIndex::new(10));
}

#[tokio::test]
async fn install_snapshot_response_higher_term_steps_down() {
    let (mut leader, _rx) = make_leader(1, vec![2, 3]).await;

    let resp = InstallSnapshotResponse {
        term: Term::new(100),
    };
    leader
        .handle_install_snapshot_response(NodeId::new(2), resp)
        .await
        .unwrap();

    assert_eq!(leader.state(), RaftState::Follower);
    assert_eq!(leader.current_term(), Term::new(100));
}

#[tokio::test]
async fn install_snapshot_response_not_leader_is_noop() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let resp = InstallSnapshotResponse {
        term: Term::new(1),
    };
    // Should not panic or error on a non-leader.
    node.handle_install_snapshot_response(NodeId::new(2), resp)
        .await
        .unwrap();
}

// ---- Full snapshot flow ----

#[tokio::test]
async fn full_snapshot_flow_leader_to_follower() {
    // Leader has entries 1..=10, compacted to 5, follower starts fresh.
    let (mut leader, mut rx) = make_leader(1, vec![2, 3]).await;

    let term = leader.current_term().as_u64();
    for i in 1..=10 {
        leader.log.append(entry(term, i, &format!("cmd{i}")));
    }

    // Apply and snapshot.
    leader.volatile.commit_index = LogIndex::new(5);
    leader.volatile.last_applied = LogIndex::new(5);

    let sm = NullStateMachine;
    leader.create_snapshot(&sm).await.unwrap();
    assert!(leader.last_snapshot().is_some());
    assert_eq!(leader.log().offset(), LogIndex::new(5));

    // Create a fresh follower.
    let cfg2 = test_config(2, vec![1, 3]);
    let (mut follower, _rx2) = RaftNode::<String, _>::new(cfg2, MemStorage::new())
        .await
        .unwrap();

    // Leader replicates to peer 2 — peer is at next_index=1, which is below
    // offset=5, so it should send a snapshot.
    leader.replicate_to_peer(NodeId::new(2));

    // Collect InstallSnapshot messages.
    let mut snapshots = Vec::new();
    while let Ok((target, msg)) = rx.try_recv() {
        if target == NodeId::new(2) {
            if let RaftMessage::InstallSnapshot(is) = msg {
                snapshots.push(is);
            }
        }
    }
    assert!(!snapshots.is_empty());

    // Feed all snapshot chunks to the follower.
    let mut sm2 = NullStateMachine;
    let mut last_resp = None;
    for snap_msg in snapshots {
        let resp = follower
            .handle_install_snapshot(snap_msg, &mut sm2)
            .await
            .unwrap();
        last_resp = Some(resp);
    }

    // Verify follower state after snapshot install.
    assert_eq!(follower.log().offset(), LogIndex::new(5));
    assert_eq!(follower.volatile_state().commit_index, LogIndex::new(5));
    assert_eq!(follower.volatile_state().last_applied, LogIndex::new(5));
    assert!(follower.last_snapshot().is_some());

    // Leader processes the response.
    leader
        .handle_install_snapshot_response(NodeId::new(2), last_resp.unwrap())
        .await
        .unwrap();

    // Leader should update peer's next_index to 6.
    let ls = leader.leader_state().unwrap();
    assert_eq!(ls.next_index[&NodeId::new(2)], LogIndex::new(6));
    assert_eq!(ls.match_index[&NodeId::new(2)], LogIndex::new(5));

    // Now leader can send remaining entries (6..=10) via AppendEntries.
    leader.replicate_to_peer(NodeId::new(2));
    let (_, msg) = rx.try_recv().unwrap();
    match msg {
        RaftMessage::AppendEntries(ae) => {
            assert_eq!(ae.prev_log_index, LogIndex::new(5));
            assert_eq!(ae.entries.len(), 5); // Entries 6, 7, 8, 9, 10.
            assert_eq!(ae.entries[0].index, LogIndex::new(6));
        }
        _ => panic!("expected AppendEntries after snapshot install"),
    }
}
