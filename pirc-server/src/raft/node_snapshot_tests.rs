use crate::raft::log::RaftLog;
use crate::raft::node::RaftNode;
use crate::raft::snapshot::{InstallSnapshot, NullStateMachine, Snapshot, StateMachine};
use crate::raft::storage::RaftStorage;
use crate::raft::test_utils::MemStorage;
use crate::raft::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

fn test_config(id: u64, peers: Vec<u64>) -> RaftConfig {
    RaftConfig {
        node_id: NodeId::new(id),
        peers: peers.into_iter().map(NodeId::new).collect(),
        ..RaftConfig::default()
    }
}

// ---- Snapshot: create_snapshot ----

/// A simple counting state machine for testing.
struct CountingStateMachine {
    count: usize,
}

impl CountingStateMachine {
    fn new() -> Self {
        Self { count: 0 }
    }
}

impl StateMachine<String> for CountingStateMachine {
    fn apply(&mut self, _command: &String) {
        self.count += 1;
    }

    fn snapshot(&self) -> Vec<u8> {
        self.count.to_le_bytes().to_vec()
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), crate::raft::snapshot::SnapshotError> {
        if data.len() < 8 {
            return Err(crate::raft::snapshot::SnapshotError::InvalidData(
                "too short".into(),
            ));
        }
        let bytes: [u8; 8] = data[..8].try_into().unwrap();
        self.count = usize::from_le_bytes(bytes);
        Ok(())
    }
}

#[tokio::test]
async fn create_snapshot_compacts_log() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Add entries and simulate applying them.
    for i in 1..=5 {
        node.log.append(LogEntry {
            term: Term::new(1),
            index: LogIndex::new(i),
            command: format!("cmd{i}"),
        });
    }
    node.volatile.commit_index = LogIndex::new(5);
    node.volatile.last_applied = LogIndex::new(5);

    let mut sm = CountingStateMachine::new();
    sm.count = 5; // Simulate 5 applied commands.

    node.create_snapshot(&sm).await.unwrap();

    // Verify snapshot was created.
    assert!(node.last_snapshot().is_some());
    let snap = node.last_snapshot().unwrap();
    assert_eq!(snap.last_included_index, LogIndex::new(5));
    assert_eq!(snap.last_included_term, Term::new(1));
    assert_eq!(snap.data, 5usize.to_le_bytes().to_vec());

    // Log should be compacted.
    assert_eq!(node.log().offset(), LogIndex::new(5));
    assert_eq!(node.log().len(), 0);
}

#[tokio::test]
async fn create_snapshot_skips_when_nothing_applied() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let sm = NullStateMachine;
    node.create_snapshot(&sm).await.unwrap();

    assert!(node.last_snapshot().is_none());
}

#[tokio::test]
async fn create_snapshot_skips_when_already_at_same_index() {
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });
    node.volatile.commit_index = LogIndex::new(1);
    node.volatile.last_applied = LogIndex::new(1);

    let sm = NullStateMachine;
    node.create_snapshot(&sm).await.unwrap();
    assert!(node.last_snapshot().is_some());

    // Second call should be a no-op since we already have a snapshot at index 1.
    node.create_snapshot(&sm).await.unwrap();
    assert_eq!(
        node.last_snapshot().unwrap().last_included_index,
        LogIndex::new(1)
    );
}

#[tokio::test]
async fn create_snapshot_persists_to_storage() {
    let storage = MemStorage::new();
    let config = test_config(1, vec![2, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, storage)
        .await
        .unwrap();

    node.log.append(LogEntry {
        term: Term::new(2),
        index: LogIndex::new(1),
        command: "cmd1".to_owned(),
    });
    node.volatile.last_applied = LogIndex::new(1);

    let sm = NullStateMachine;
    node.create_snapshot(&sm).await.unwrap();

    // Verify storage has the snapshot.
    let stored = node.storage.load_snapshot().await.unwrap();
    assert!(stored.is_some());
    let (data, idx, term) = stored.unwrap();
    assert_eq!(idx, LogIndex::new(1));
    assert_eq!(term, Term::new(2));
    assert_eq!(data, Vec::<u8>::new()); // NullStateMachine returns empty.
}

// ---- Snapshot: should_snapshot ----

#[tokio::test]
async fn should_snapshot_returns_false_below_threshold() {
    let mut cfg = test_config(1, vec![2, 3]);
    cfg.snapshot_threshold = 10;
    let (node, _rx) = RaftNode::<String, _>::new(cfg, MemStorage::new())
        .await
        .unwrap();

    assert!(!node.should_snapshot());
}

#[tokio::test]
async fn should_snapshot_returns_true_at_threshold() {
    let mut cfg = test_config(1, vec![2, 3]);
    cfg.snapshot_threshold = 3;
    let (mut node, _rx) = RaftNode::<String, _>::new(cfg, MemStorage::new())
        .await
        .unwrap();

    for i in 1..=3 {
        node.log.append(LogEntry {
            term: Term::new(1),
            index: LogIndex::new(i),
            command: format!("cmd{i}"),
        });
    }

    assert!(node.should_snapshot());
}

// ---- Snapshot: handle_install_snapshot ----

#[tokio::test]
async fn handle_install_snapshot_single_chunk() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let snapshot_data = b"test-state".to_vec();
    let req = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(1),
        offset: 0,
        data: snapshot_data.clone(),
        done: true,
    };

    let mut sm = NullStateMachine;
    let resp = node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(resp.term, Term::new(1));

    // Verify snapshot was installed.
    let snap = node.last_snapshot().unwrap();
    assert_eq!(snap.last_included_index, LogIndex::new(10));
    assert_eq!(snap.last_included_term, Term::new(1));
    assert_eq!(snap.data, snapshot_data);

    // Verify volatile state updated.
    assert_eq!(node.volatile_state().commit_index, LogIndex::new(10));
    assert_eq!(node.volatile_state().last_applied, LogIndex::new(10));

    // Verify log was reset.
    assert_eq!(node.log().offset(), LogIndex::new(10));
    assert!(node.log().is_empty());
}

#[tokio::test]
async fn handle_install_snapshot_multi_chunk() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let mut sm = NullStateMachine;

    // First chunk.
    let req1 = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(1),
        offset: 0,
        data: b"chunk1-".to_vec(),
        done: false,
    };
    let resp1 = node.handle_install_snapshot(req1, &mut sm).await.unwrap();
    assert_eq!(resp1.term, Term::new(1));
    assert!(node.last_snapshot().is_none()); // Not installed yet.

    // Second chunk (final).
    let req2 = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(1),
        offset: 7,
        data: b"chunk2".to_vec(),
        done: true,
    };
    let resp2 = node.handle_install_snapshot(req2, &mut sm).await.unwrap();
    assert_eq!(resp2.term, Term::new(1));

    // Verify complete snapshot was assembled.
    let snap = node.last_snapshot().unwrap();
    assert_eq!(snap.data, b"chunk1-chunk2");
    assert_eq!(snap.last_included_index, LogIndex::new(10));
}

#[tokio::test]
async fn handle_install_snapshot_rejects_stale_term() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.current_term = Term::new(5);

    let req = InstallSnapshot {
        term: Term::new(3),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(3),
        offset: 0,
        data: b"data".to_vec(),
        done: true,
    };

    let mut sm = NullStateMachine;
    let resp = node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(resp.term, Term::new(5));
    // Snapshot should NOT be installed.
    assert!(node.last_snapshot().is_none());
}

#[tokio::test]
async fn handle_install_snapshot_steps_down_candidate() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    node.start_election().await.unwrap();
    assert_eq!(node.state(), RaftState::Candidate);
    let election_term = node.current_term();

    let req = InstallSnapshot {
        term: election_term,
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(5),
        last_included_term: Term::new(1),
        offset: 0,
        data: Vec::new(),
        done: true,
    };

    let mut sm = NullStateMachine;
    node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(node.state(), RaftState::Follower);
    assert_eq!(node.current_leader(), Some(NodeId::new(1)));
}

#[tokio::test]
async fn handle_install_snapshot_keeps_log_suffix_on_match() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Add log entries at indices 1..=5, all term 1.
    for i in 1..=5 {
        node.log.append(LogEntry {
            term: Term::new(1),
            index: LogIndex::new(i),
            command: format!("cmd{i}"),
        });
    }

    // Snapshot covers up to index 3 with matching term 1.
    let req = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(3),
        last_included_term: Term::new(1),
        offset: 0,
        data: b"state".to_vec(),
        done: true,
    };

    let mut sm = NullStateMachine;
    node.handle_install_snapshot(req, &mut sm).await.unwrap();

    // Entries 4-5 should be kept (suffix after snapshot).
    assert_eq!(node.log().offset(), LogIndex::new(3));
    assert_eq!(node.log().len(), 2);
    assert_eq!(
        node.log().get(LogIndex::new(4)).unwrap().command,
        "cmd4"
    );
    assert_eq!(
        node.log().get(LogIndex::new(5)).unwrap().command,
        "cmd5"
    );
}

#[tokio::test]
async fn handle_install_snapshot_discards_log_on_mismatch() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Add entries at indices 1..=3, term 1.
    for i in 1..=3 {
        node.log.append(LogEntry {
            term: Term::new(1),
            index: LogIndex::new(i),
            command: format!("cmd{i}"),
        });
    }

    // Snapshot covers up to index 3 but with term 2 (mismatch).
    let req = InstallSnapshot {
        term: Term::new(2),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(3),
        last_included_term: Term::new(2),
        offset: 0,
        data: b"state".to_vec(),
        done: true,
    };

    let mut sm = NullStateMachine;
    node.handle_install_snapshot(req, &mut sm).await.unwrap();

    // Entire log should be discarded.
    assert_eq!(node.log().offset(), LogIndex::new(3));
    assert!(node.log().is_empty());
}

#[tokio::test]
async fn handle_install_snapshot_restores_state_machine() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    let mut sm = CountingStateMachine::new();
    assert_eq!(sm.count, 0);

    let snapshot_data = 42usize.to_le_bytes().to_vec();
    let req = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(42),
        last_included_term: Term::new(1),
        offset: 0,
        data: snapshot_data,
        done: true,
    };

    node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(sm.count, 42);
}

#[tokio::test]
async fn handle_install_snapshot_non_first_chunk_without_buffer() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Send a non-first chunk without a pending buffer — should be ignored.
    let req = InstallSnapshot {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(1),
        offset: 100,
        data: b"orphan-chunk".to_vec(),
        done: false,
    };

    let mut sm = NullStateMachine;
    let resp = node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(resp.term, Term::new(1));
    assert!(node.last_snapshot().is_none());
}

#[tokio::test]
async fn handle_install_snapshot_higher_term_updates_node() {
    let config = test_config(2, vec![1, 3]);
    let (mut node, _rx) = RaftNode::<String, _>::new(config, MemStorage::new())
        .await
        .unwrap();

    // Node is at term 1.
    node.current_term = Term::new(1);

    let req = InstallSnapshot {
        term: Term::new(5),
        leader_id: NodeId::new(1),
        last_included_index: LogIndex::new(10),
        last_included_term: Term::new(4),
        offset: 0,
        data: Vec::new(),
        done: true,
    };

    let mut sm = NullStateMachine;
    let resp = node.handle_install_snapshot(req, &mut sm).await.unwrap();
    assert_eq!(resp.term, Term::new(5));
    assert_eq!(node.current_term(), Term::new(5));
    assert_eq!(node.state(), RaftState::Follower);
}
