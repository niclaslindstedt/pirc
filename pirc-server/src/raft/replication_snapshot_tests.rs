use crate::raft::node::RaftNode;
use crate::raft::rpc::{RaftMessage, RequestVoteResponse};
use crate::raft::snapshot::{InstallSnapshotResponse, NullStateMachine, Snapshot};
use crate::raft::test_utils::MemStorage;
use crate::raft::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

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
