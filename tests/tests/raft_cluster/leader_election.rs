//! Leader election tests: leader failover, re-election timing, fault
//! tolerance, and term-based step-down.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use pirc_server::raft::{
    AppendEntries, LogIndex, NullStateMachine, RaftBuilder, RaftHandle, RaftMessage, RaftState,
    ShutdownSender,
};
use pirc_server::raft::types::{NodeId, Term};

use super::{MemStorage, TestCluster, test_config};

// ===========================================================================
// Leader Failure and Re-election
// ===========================================================================

#[tokio::test]
async fn leader_failure_triggers_re_election() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("first leader should be elected");

    let first_term = cluster.handle(first_leader).current_term();

    // Kill the leader.
    cluster.kill_node(first_leader);

    // Wait for remaining nodes to elect a new leader.
    time::sleep(Duration::from_millis(500)).await;

    let remaining: Vec<u64> = [1, 2, 3]
        .iter()
        .copied()
        .filter(|&id| id != first_leader)
        .collect();

    let mut new_leader = None;
    let deadline = time::Instant::now() + Duration::from_secs(3);
    loop {
        for &id in &remaining {
            if cluster.handle(id).state() == RaftState::Leader {
                new_leader = Some(id);
                break;
            }
        }
        if new_leader.is_some() || time::Instant::now() >= deadline {
            break;
        }
        time::sleep(Duration::from_millis(25)).await;
    }

    let new_leader = new_leader.expect("new leader should be elected after failure");
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
async fn re_election_completes_within_two_seconds() {
    let cluster = TestCluster::start(&[1, 2, 3]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("first leader should be elected");

    // Kill the leader.
    cluster.kill_node(first_leader);

    let remaining: Vec<u64> = [1, 2, 3]
        .iter()
        .copied()
        .filter(|&id| id != first_leader)
        .collect();

    // Re-election should complete within 2 seconds.
    let start = time::Instant::now();
    let mut new_leader = None;
    let deadline = start + Duration::from_secs(2);
    loop {
        for &id in &remaining {
            if cluster.handle(id).state() == RaftState::Leader {
                new_leader = Some(id);
                break;
            }
        }
        if new_leader.is_some() || time::Instant::now() >= deadline {
            break;
        }
        time::sleep(Duration::from_millis(25)).await;
    }

    let elapsed = start.elapsed();
    assert!(
        new_leader.is_some(),
        "re-election should complete within 2 seconds (took {elapsed:?})"
    );
}

// ===========================================================================
// Fault Tolerance
// ===========================================================================

#[tokio::test]
async fn five_node_cluster_tolerates_single_failure() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill the leader.
    cluster.kill_node(first_leader);

    // Remaining 4 nodes should elect a new leader (quorum = 3 of 5).
    let remaining: Vec<u64> = [1, 2, 3, 4, 5]
        .iter()
        .copied()
        .filter(|&id| id != first_leader)
        .collect();

    let mut new_leader = None;
    let deadline = time::Instant::now() + Duration::from_secs(3);
    loop {
        for &id in &remaining {
            if cluster.handle(id).state() == RaftState::Leader {
                new_leader = Some(id);
                break;
            }
        }
        if new_leader.is_some() || time::Instant::now() >= deadline {
            break;
        }
        time::sleep(Duration::from_millis(25)).await;
    }

    assert!(
        new_leader.is_some(),
        "5-node cluster should elect new leader after single failure"
    );

    cluster.shutdown_all();
}

#[tokio::test]
async fn five_node_cluster_tolerates_two_failures() {
    let cluster = TestCluster::start(&[1, 2, 3, 4, 5]).await;

    let first_leader = cluster
        .wait_for_leader(Duration::from_secs(2))
        .await
        .expect("leader should be elected");

    // Kill two nodes (leader + one more).
    cluster.kill_node(first_leader);
    let second_kill = [1, 2, 3, 4, 5]
        .iter()
        .copied()
        .find(|&id| id != first_leader)
        .unwrap();
    cluster.kill_node(second_kill);

    let remaining: Vec<u64> = [1, 2, 3, 4, 5]
        .iter()
        .copied()
        .filter(|&id| id != first_leader && id != second_kill)
        .collect();

    // Remaining 3 nodes should still form a majority (quorum = 3 of 5).
    let mut new_leader = None;
    let deadline = time::Instant::now() + Duration::from_secs(4);
    loop {
        for &id in &remaining {
            if cluster.handle(id).state() == RaftState::Leader {
                new_leader = Some(id);
                break;
            }
        }
        if new_leader.is_some() || time::Instant::now() >= deadline {
            break;
        }
        time::sleep(Duration::from_millis(25)).await;
    }

    assert!(
        new_leader.is_some(),
        "5-node cluster should tolerate 2 failures (3 remaining = quorum)"
    );

    cluster.shutdown_all();
}

// ===========================================================================
// Term-based Step-down
// ===========================================================================

#[tokio::test]
async fn higher_term_causes_step_down() {
    // Build a 3-node cluster, let leader emerge, then inject a higher-term message.
    let ids = [1u64, 2, 3];
    let mut inbound_senders: HashMap<u64, mpsc::UnboundedSender<(NodeId, RaftMessage<String>)>> =
        HashMap::new();
    let mut handles: HashMap<u64, RaftHandle<String>> = HashMap::new();
    let mut shutdowns: Vec<ShutdownSender> = Vec::new();
    let mut outbound_rxs = Vec::new();

    for &id in &ids {
        let peers: Vec<u64> = ids.iter().copied().filter(|&p| p != id).collect();
        let config = test_config(id, peers);

        let (mut driver, handle, shutdown, inbound_tx, outbound_rx) =
            RaftBuilder::<String, _, _>::new()
                .config(config)
                .storage(MemStorage::new())
                .state_machine(NullStateMachine)
                .build()
                .await
                .unwrap();

        inbound_senders.insert(id, inbound_tx);
        handles.insert(id, handle);
        shutdowns.push(shutdown);
        outbound_rxs.push((id, outbound_rx));

        tokio::spawn(async move {
            driver.run().await;
        });
    }

    for (source_id, mut outbound_rx) in outbound_rxs {
        let senders = inbound_senders.clone();
        tokio::spawn(async move {
            while let Some((target, msg)) = outbound_rx.recv().await {
                if let Some(tx) = senders.get(&target.as_u64()) {
                    let _ = tx.send((NodeId::new(source_id), msg));
                }
            }
        });
    }

    // Wait for leader.
    let mut leader_id = None;
    let deadline = time::Instant::now() + Duration::from_secs(2);
    loop {
        for &id in &ids {
            if handles[&id].state() == RaftState::Leader {
                leader_id = Some(id);
                break;
            }
        }
        if leader_id.is_some() || time::Instant::now() >= deadline {
            break;
        }
        time::sleep(Duration::from_millis(25)).await;
    }
    let leader_id = leader_id.expect("leader should be elected");
    let current_term = handles[&leader_id].current_term();

    // Inject an AppendEntries with a much higher term from a "phantom" leader.
    let higher_term = Term::new(current_term.as_u64() + 10);
    let ae = AppendEntries {
        term: higher_term,
        leader_id: NodeId::new(99), // phantom node
        prev_log_index: LogIndex::new(0),
        prev_log_term: Term::new(0),
        entries: vec![],
        leader_commit: LogIndex::new(0),
    };

    inbound_senders[&leader_id]
        .send((NodeId::new(99), RaftMessage::AppendEntries(ae)))
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    // Leader should have stepped down.
    assert_eq!(
        handles[&leader_id].state(),
        RaftState::Follower,
        "leader should step down on higher term"
    );
    assert_eq!(
        handles[&leader_id].current_term(),
        higher_term,
        "leader should adopt the higher term"
    );

    for s in &shutdowns {
        s.shutdown();
    }
}
