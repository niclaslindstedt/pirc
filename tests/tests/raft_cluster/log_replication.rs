//! Log replication tests: entry replication to followers, commit delivery,
//! and ordering guarantees.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use pirc_server::raft::{
    LogIndex, NullStateMachine, RaftBuilder, RaftHandle, RaftMessage, RaftState, ShutdownSender,
};
use pirc_server::raft::types::NodeId;

use super::{MemStorage, test_config};

#[tokio::test]
async fn leader_replicates_entry_to_followers() {
    // Build nodes manually to access commit channels on all nodes.
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

    // Wait for leader election.
    let deadline = time::Instant::now() + Duration::from_secs(2);
    let mut leader_id = None;
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

    // Take commit channels from all nodes.
    let mut commit_rxs: HashMap<u64, _> = HashMap::new();
    for &id in &ids {
        commit_rxs.insert(id, handles.get_mut(&id).unwrap().take_commit_rx());
    }

    // Propose a command to the leader.
    handles[&leader_id]
        .propose("set key value".to_owned())
        .unwrap();

    // Verify the leader receives the committed entry.
    let leader_entry = time::timeout(
        Duration::from_secs(2),
        commit_rxs.get_mut(&leader_id).unwrap().recv(),
    )
    .await
    .expect("leader should receive committed entry within timeout")
    .expect("leader commit channel should not be closed");
    assert_eq!(leader_entry.command, "set key value");
    assert_eq!(leader_entry.index, LogIndex::new(1));

    // Verify each follower also receives the committed entry.
    for &id in &ids {
        if id == leader_id {
            continue;
        }
        let follower_entry = time::timeout(
            Duration::from_secs(2),
            commit_rxs.get_mut(&id).unwrap().recv(),
        )
        .await
        .unwrap_or_else(|_| panic!("follower {id} should receive committed entry within timeout"))
        .unwrap_or_else(|| panic!("follower {id} commit channel should not be closed"));
        assert_eq!(
            follower_entry.command, "set key value",
            "follower {id} should have the same entry"
        );
        assert_eq!(
            follower_entry.index,
            LogIndex::new(1),
            "follower {id} entry should have index 1"
        );
    }

    for s in &shutdowns {
        s.shutdown();
    }
}

#[tokio::test]
async fn committed_entries_delivered_via_commit_channel() {
    // Build nodes manually to access commit_rx.
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

    // Wire up message routing.
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

    // Wait for leader election.
    let deadline = time::Instant::now() + Duration::from_secs(2);
    let mut leader_id = None;
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

    // Take commit channel from leader.
    let mut commit_rx = handles.get_mut(&leader_id).unwrap().take_commit_rx();

    // Propose a command.
    handles[&leader_id]
        .propose("hello world".to_owned())
        .unwrap();

    // Wait for the committed entry.
    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("should receive committed entry within timeout")
        .expect("commit channel should not be closed");

    assert_eq!(entry.command, "hello world");
    assert_eq!(entry.index, LogIndex::new(1));

    for s in &shutdowns {
        s.shutdown();
    }
}

#[tokio::test]
async fn multiple_entries_replicated_in_order() {
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

    let mut commit_rx = handles.get_mut(&leader_id).unwrap().take_commit_rx();

    // Propose multiple entries.
    for i in 1..=5 {
        handles[&leader_id]
            .propose(format!("entry-{i}"))
            .unwrap();
        // Small delay to allow processing.
        time::sleep(Duration::from_millis(20)).await;
    }

    // Collect committed entries.
    let mut committed = Vec::new();
    for _ in 0..5 {
        match time::timeout(Duration::from_secs(2), commit_rx.recv()).await {
            Ok(Some(entry)) => committed.push(entry),
            _ => break,
        }
    }

    assert_eq!(committed.len(), 5, "all 5 entries should be committed");
    for (i, entry) in committed.iter().enumerate() {
        assert_eq!(
            entry.command,
            format!("entry-{}", i + 1),
            "entries should be committed in order"
        );
        assert_eq!(
            entry.index,
            LogIndex::new((i + 1) as u64),
            "entry indices should be sequential"
        );
    }

    for s in &shutdowns {
        s.shutdown();
    }
}
