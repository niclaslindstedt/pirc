use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use crate::raft::driver::{RaftBuilder, RaftError};
use crate::raft::rpc::{
    AppendEntries, AppendEntriesResponse, RaftMessage, RequestVote, RequestVoteResponse,
};
use crate::raft::snapshot::NullStateMachine;
use crate::raft::test_utils::MemStorage;
use crate::raft::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

fn test_config(node_id: u64, peers: Vec<u64>) -> RaftConfig {
    RaftConfig {
        election_timeout_min: Duration::from_millis(100),
        election_timeout_max: Duration::from_millis(200),
        heartbeat_interval: Duration::from_millis(30),
        node_id: NodeId::new(node_id),
        peers: peers.into_iter().map(NodeId::new).collect(),
        ..RaftConfig::default()
    }
}

// --- RaftBuilder tests ---

#[tokio::test]
async fn builder_creates_driver_and_handle() {
    let (mut driver, handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    assert_eq!(handle.state(), RaftState::Follower);
    assert_eq!(handle.current_term(), Term::new(0));
    assert_eq!(handle.current_leader(), None);
    assert!(!handle.is_leader());
    assert_eq!(driver.node().node_id(), NodeId::new(1));

    shutdown.shutdown();
}

#[tokio::test]
#[should_panic(expected = "config is required")]
async fn builder_panics_without_config() {
    let _ = RaftBuilder::<String, MemStorage, NullStateMachine>::new()
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await;
}

#[tokio::test]
#[should_panic(expected = "storage is required")]
async fn builder_panics_without_storage() {
    let _ = RaftBuilder::<String, MemStorage, NullStateMachine>::new()
        .config(test_config(1, vec![2, 3]))
        .state_machine(NullStateMachine)
        .build()
        .await;
}

#[tokio::test]
#[should_panic(expected = "state_machine is required")]
async fn builder_panics_without_state_machine() {
    let _ = RaftBuilder::<String, MemStorage, NullStateMachine>::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .build()
        .await;
}

// --- RaftHandle tests ---

#[tokio::test]
async fn handle_propose_fails_when_not_leader() {
    let (_driver, handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    let err = handle.propose("test".to_owned()).unwrap_err();
    assert!(matches!(err, RaftError::NotLeader));

    shutdown.shutdown();
}

#[tokio::test]
async fn handle_take_commit_rx() {
    let (_driver, mut handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    let _rx = handle.take_commit_rx();
    // Second take returns an empty receiver.
    let mut rx2 = handle.take_commit_rx();
    // Empty receiver should immediately return None when sender is dropped.
    assert!(rx2.try_recv().is_err());

    shutdown.shutdown();
}

// --- Driver event loop tests ---

#[tokio::test]
async fn driver_shuts_down_on_signal() {
    let (mut driver, _handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    // Signal shutdown before running — run should exit immediately.
    shutdown.shutdown();

    // Run should complete without hanging.
    time::timeout(Duration::from_secs(2), driver.run())
        .await
        .expect("driver should shut down within timeout");
}

#[tokio::test]
async fn driver_solo_node_becomes_leader_on_election_timeout() {
    let (mut driver, handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    let shutdown_clone = shutdown;

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for the election timeout to fire and the solo node to become leader.
    time::sleep(Duration::from_millis(250)).await;

    assert!(handle.is_leader());
    assert_eq!(handle.current_leader(), Some(NodeId::new(1)));
    assert!(handle.current_term() >= Term::new(1));

    shutdown_clone.shutdown();
}

#[tokio::test]
async fn driver_election_timeout_fires_and_starts_election() {
    let (mut driver, handle, shutdown, _inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for the election timeout to fire.
    time::sleep(Duration::from_millis(250)).await;

    // Node should have become a candidate.
    assert_eq!(handle.state(), RaftState::Candidate);
    assert!(handle.current_term() >= Term::new(1));

    // Should have sent RequestVote to both peers.
    let mut vote_requests = Vec::new();
    while let Ok(msg) = outbound_rx.try_recv() {
        if let (target, RaftMessage::RequestVote(_)) = &msg {
            vote_requests.push(*target);
        }
    }
    assert!(vote_requests.contains(&NodeId::new(2)));
    assert!(vote_requests.contains(&NodeId::new(3)));

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_becomes_leader_after_majority_votes() {
    let (mut driver, handle, shutdown, inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for election to start.
    time::sleep(Duration::from_millis(250)).await;

    let term = handle.current_term();

    // Send a vote grant from node 2.
    inbound_tx
        .send((
            NodeId::new(2),
            RaftMessage::RequestVoteResponse(RequestVoteResponse {
                term,
                vote_granted: true,
            }),
        ))
        .unwrap();

    // Allow the message to be processed.
    time::sleep(Duration::from_millis(50)).await;

    // Should now be leader (self + node 2 = 2 votes, quorum = 2).
    assert!(handle.is_leader());
    assert_eq!(handle.current_leader(), Some(NodeId::new(1)));

    // Leader should start sending heartbeats — drain outbound.
    time::sleep(Duration::from_millis(100)).await;
    let mut heartbeat_count = 0;
    while let Ok((_, msg)) = outbound_rx.try_recv() {
        if matches!(msg, RaftMessage::AppendEntries(_)) {
            heartbeat_count += 1;
        }
    }
    assert!(heartbeat_count > 0, "leader should send heartbeats");

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_handles_append_entries_from_leader() {
    let (mut driver, handle, shutdown, inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(2, vec![1, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Send an AppendEntries from node 1 (pretending to be leader).
    let ae = AppendEntries {
        term: Term::new(1),
        leader_id: NodeId::new(1),
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::new(0),
        entries: vec![LogEntry {
            term: Term::new(1),
            index: LogIndex::new(1),
            command: "cmd1".to_owned(),
        }],
        leader_commit: LogIndex::new(1),
    };

    inbound_tx
        .send((NodeId::new(1), RaftMessage::AppendEntries(ae)))
        .unwrap();

    time::sleep(Duration::from_millis(50)).await;

    assert_eq!(handle.state(), RaftState::Follower);
    assert_eq!(handle.current_leader(), Some(NodeId::new(1)));
    assert_eq!(handle.current_term(), Term::new(1));

    // Should have sent an AppendEntriesResponse.
    let mut found_response = false;
    while let Ok((target, msg)) = outbound_rx.try_recv() {
        if let RaftMessage::AppendEntriesResponse(resp) = &msg {
            assert_eq!(target, NodeId::new(1));
            assert!(resp.success);
            found_response = true;
        }
    }
    assert!(found_response, "should send AppendEntriesResponse");

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_election_timer_resets_on_append_entries() {
    let (mut driver, handle, shutdown, inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(2, vec![1, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Send periodic AppendEntries to prevent election timeout.
    for _ in 0..5 {
        let ae = AppendEntries {
            term: Term::new(1),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::new(0),
            prev_log_term: Term::new(0),
            entries: vec![],
            leader_commit: LogIndex::new(0),
        };
        inbound_tx
            .send((NodeId::new(1), RaftMessage::AppendEntries(ae)))
            .unwrap();
        time::sleep(Duration::from_millis(80)).await;
    }

    // After 400ms of heartbeats, node should still be Follower (no election).
    assert_eq!(handle.state(), RaftState::Follower);
    assert_eq!(handle.current_leader(), Some(NodeId::new(1)));

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_handles_request_vote_and_grants() {
    let (mut driver, handle, shutdown, inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(2, vec![1, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Give it a moment to start.
    time::sleep(Duration::from_millis(10)).await;

    // Send a RequestVote from node 1 with term 1.
    let rv = RequestVote {
        term: Term::new(1),
        candidate_id: NodeId::new(1),
        last_log_index: LogIndex::new(0),
        last_log_term: Term::new(0),
    };
    inbound_tx
        .send((NodeId::new(1), RaftMessage::RequestVote(rv)))
        .unwrap();

    time::sleep(Duration::from_millis(50)).await;

    assert_eq!(handle.current_term(), Term::new(1));

    // Should have sent a vote response.
    let mut found_grant = false;
    while let Ok((target, msg)) = outbound_rx.try_recv() {
        if let RaftMessage::RequestVoteResponse(resp) = &msg {
            if resp.vote_granted {
                assert_eq!(target, NodeId::new(1));
                found_grant = true;
            }
        }
    }
    assert!(found_grant, "should grant vote");

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_client_proposal_as_leader() {
    // Use a solo node so it becomes leader automatically.
    let (mut driver, handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for solo node to become leader.
    time::sleep(Duration::from_millis(250)).await;
    assert!(handle.is_leader());

    // Propose a command.
    handle.propose("set x 42".to_owned()).unwrap();

    // Allow proposal to be processed.
    time::sleep(Duration::from_millis(50)).await;

    // Solo node should commit immediately.
    // State should still be leader.
    assert!(handle.is_leader());

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_committed_entries_sent_to_commit_channel() {
    // Solo node — commits are immediate.
    let (mut driver, mut handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
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

    // Wait for solo node to become leader.
    time::sleep(Duration::from_millis(250)).await;
    assert!(handle.is_leader());

    // Propose a command.
    handle.propose("hello".to_owned()).unwrap();

    // Wait for it to be committed and applied.
    let entry = time::timeout(Duration::from_secs(1), commit_rx.recv())
        .await
        .expect("should receive committed entry within timeout")
        .expect("commit channel should not be closed");

    assert_eq!(entry.command, "hello");
    assert_eq!(entry.index, LogIndex::new(1));

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_heartbeats_sent_periodically_as_leader() {
    let (mut driver, handle, shutdown, _inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    // Make it a solo config to auto-elect, but with peers for heartbeats.
    // Actually, with peers [2, 3] it won't auto-elect. Let's use a trick:
    // start as solo, then we'll test differently.
    shutdown.shutdown();
    drop(driver);
    drop(outbound_rx);
    drop(handle);

    // Use solo node with no peers — it auto-elects, but no heartbeats to send.
    // Instead test with 3-node cluster that wins election.
    let (mut driver, handle, shutdown, inbound_tx, mut outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for election.
    time::sleep(Duration::from_millis(250)).await;

    let term = handle.current_term();

    // Grant vote to become leader.
    inbound_tx
        .send((
            NodeId::new(2),
            RaftMessage::RequestVoteResponse(RequestVoteResponse {
                term,
                vote_granted: true,
            }),
        ))
        .unwrap();

    time::sleep(Duration::from_millis(50)).await;
    assert!(handle.is_leader());

    // Drain all existing messages.
    while outbound_rx.try_recv().is_ok() {}

    // Wait for heartbeat interval (30ms) to fire a few times.
    time::sleep(Duration::from_millis(120)).await;

    let mut heartbeat_count = 0;
    while let Ok((_, msg)) = outbound_rx.try_recv() {
        if matches!(msg, RaftMessage::AppendEntries(_)) {
            heartbeat_count += 1;
        }
    }
    // Should have sent multiple heartbeats (to 2 peers, multiple intervals).
    assert!(
        heartbeat_count >= 2,
        "expected at least 2 heartbeats, got {heartbeat_count}"
    );

    shutdown.shutdown();
}

#[tokio::test]
async fn driver_steps_down_on_higher_term() {
    let (mut driver, handle, shutdown, inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![2, 3]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    tokio::spawn(async move {
        driver.run().await;
    });

    // Wait for election to start (node becomes Candidate).
    time::sleep(Duration::from_millis(250)).await;
    assert_eq!(handle.state(), RaftState::Candidate);

    // Send a vote from peer 2 to become leader.
    let term = handle.current_term();
    inbound_tx
        .send((
            NodeId::new(2),
            RaftMessage::RequestVoteResponse(RequestVoteResponse {
                term,
                vote_granted: true,
            }),
        ))
        .unwrap();

    time::sleep(Duration::from_millis(50)).await;
    assert!(handle.is_leader());

    // Send AppendEntries with higher term — should step down.
    let ae = AppendEntries {
        term: Term::new(term.as_u64() + 5),
        leader_id: NodeId::new(3),
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::new(0),
        entries: vec![],
        leader_commit: LogIndex::new(0),
    };
    inbound_tx
        .send((NodeId::new(3), RaftMessage::AppendEntries(ae)))
        .unwrap();

    time::sleep(Duration::from_millis(50)).await;

    assert_eq!(handle.state(), RaftState::Follower);
    assert_eq!(handle.current_leader(), Some(NodeId::new(3)));

    shutdown.shutdown();
}

#[tokio::test]
async fn shutdown_sender_can_be_called_multiple_times() {
    let (mut driver, _handle, shutdown, _inbound_tx, _outbound_rx) = RaftBuilder::new()
        .config(test_config(1, vec![]))
        .storage(MemStorage::new())
        .state_machine(NullStateMachine)
        .build()
        .await
        .unwrap();

    shutdown.shutdown();
    shutdown.shutdown(); // Should not panic.

    time::timeout(Duration::from_secs(1), driver.run())
        .await
        .expect("driver should shut down");
}

#[tokio::test]
async fn raft_error_display() {
    assert_eq!(RaftError::NotLeader.to_string(), "not the leader");
    assert_eq!(
        RaftError::Shutdown.to_string(),
        "raft driver has shut down"
    );
}
