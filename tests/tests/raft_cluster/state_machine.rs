//! State machine tests: `ClusterCommand` replication including user
//! registration and channel join commands.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use pirc_server::raft::{
    ClusterCommand, ClusterStateMachine, RaftBuilder, RaftHandle, RaftMessage,
    RaftState, ShutdownSender,
};
use pirc_server::raft::types::NodeId;

use super::{MemStorage, test_config};

#[tokio::test]
async fn cluster_command_replicated_across_nodes() {
    // Build a 3-node cluster with ClusterStateMachine.
    let ids = [1u64, 2, 3];
    let mut inbound_senders: HashMap<
        u64,
        mpsc::UnboundedSender<(NodeId, RaftMessage<ClusterCommand>)>,
    > = HashMap::new();
    let mut handles: HashMap<u64, RaftHandle<ClusterCommand>> = HashMap::new();
    let mut shutdowns: Vec<ShutdownSender> = Vec::new();
    let mut outbound_rxs = Vec::new();

    for &id in &ids {
        let peers: Vec<u64> = ids.iter().copied().filter(|&p| p != id).collect();
        let config = test_config(id, peers);

        let (mut driver, handle, shutdown, inbound_tx, outbound_rx) =
            RaftBuilder::<ClusterCommand, _, _>::new()
                .config(config)
                .storage(MemStorage::<ClusterCommand>::new())
                .state_machine(ClusterStateMachine::new())
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

    // Propose a UserRegistered command.
    let cmd = ClusterCommand::UserRegistered {
        connection_id: 42,
        nickname: "alice".to_owned(),
        username: "alice".to_owned(),
        realname: "Alice Smith".to_owned(),
        hostname: "localhost".to_owned(),
        signon_time: 1_000_000,
        home_node: Some(NodeId::new(leader_id)),
    };

    handles[&leader_id].propose(cmd).unwrap();

    // Wait for commit.
    let entry = time::timeout(Duration::from_secs(2), commit_rx.recv())
        .await
        .expect("should receive committed ClusterCommand")
        .expect("commit channel should not be closed");

    if let ClusterCommand::UserRegistered { nickname, .. } = &entry.command {
        assert_eq!(nickname, "alice");
    } else {
        panic!("expected UserRegistered command, got {:?}", entry.command);
    }

    for s in &shutdowns {
        s.shutdown();
    }
}

#[tokio::test]
async fn channel_join_command_replicated() {
    let ids = [1u64, 2, 3];
    let mut inbound_senders: HashMap<
        u64,
        mpsc::UnboundedSender<(NodeId, RaftMessage<ClusterCommand>)>,
    > = HashMap::new();
    let mut handles: HashMap<u64, RaftHandle<ClusterCommand>> = HashMap::new();
    let mut shutdowns: Vec<ShutdownSender> = Vec::new();
    let mut outbound_rxs = Vec::new();

    for &id in &ids {
        let peers: Vec<u64> = ids.iter().copied().filter(|&p| p != id).collect();
        let config = test_config(id, peers);

        let (mut driver, handle, shutdown, inbound_tx, outbound_rx) =
            RaftBuilder::<ClusterCommand, _, _>::new()
                .config(config)
                .storage(MemStorage::<ClusterCommand>::new())
                .state_machine(ClusterStateMachine::new())
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

    // Register a user first, then join a channel.
    handles[&leader_id]
        .propose(ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "bob".to_owned(),
            username: "bob".to_owned(),
            realname: "Bob".to_owned(),
            hostname: "localhost".to_owned(),
            signon_time: 100,
            home_node: None,
        })
        .unwrap();

    time::sleep(Duration::from_millis(100)).await;

    handles[&leader_id]
        .propose(ClusterCommand::ChannelJoined {
            nickname: "bob".to_owned(),
            channel: "#test".to_owned(),
            status: "".to_owned(),
        })
        .unwrap();

    // Collect both committed entries.
    let mut committed = Vec::new();
    for _ in 0..2 {
        match time::timeout(Duration::from_secs(2), commit_rx.recv()).await {
            Ok(Some(entry)) => committed.push(entry),
            _ => break,
        }
    }

    assert_eq!(committed.len(), 2, "both commands should be committed");
    assert!(matches!(
        &committed[0].command,
        ClusterCommand::UserRegistered { nickname, .. } if nickname == "bob"
    ));
    assert!(matches!(
        &committed[1].command,
        ClusterCommand::ChannelJoined { channel, .. } if channel == "#test"
    ));

    for s in &shutdowns {
        s.shutdown();
    }
}
