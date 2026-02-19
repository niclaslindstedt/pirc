//! Integration tests for Raft consensus cluster formation.
//!
//! Tests multi-node cluster formation, leader election, deterministic
//! election succession, leader failure and re-election, log replication,
//! and `ClusterCommand` state machine replication.
//!
//! Test modules are organized by scenario category:
//! - `formation` — cluster startup and leader agreement
//! - `leader_election` — leader failover, re-election, fault tolerance
//! - `log_replication` — log entry replication and consistency
//! - `state_machine` — `ClusterCommand` replication and state consistency

mod formation;
mod leader_election;
mod log_replication;
mod state_machine;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use pirc_server::raft::{
    LogEntry, LogIndex, NullStateMachine, RaftBuilder, RaftConfig, RaftHandle, RaftMessage,
    RaftState, ShutdownSender, StorageResult,
};
use pirc_server::raft::storage::RaftStorage;
use pirc_server::raft::types::{NodeId, Term};

// ---------------------------------------------------------------------------
// In-memory storage for integration tests (mirrors the crate-internal one)
// ---------------------------------------------------------------------------

pub struct MemStorage<T: Clone + Send + Sync = String> {
    term: Mutex<Term>,
    voted_for: Mutex<Option<NodeId>>,
    log: Mutex<Vec<LogEntry<T>>>,
    snapshot: Mutex<Option<(Vec<u8>, LogIndex, Term)>>,
}

impl<T: Clone + Send + Sync> MemStorage<T> {
    pub fn new() -> Self {
        Self {
            term: Mutex::new(Term::default()),
            voted_for: Mutex::new(None),
            log: Mutex::new(Vec::new()),
            snapshot: Mutex::new(None),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> RaftStorage<T> for MemStorage<T> {
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
        entries: &[LogEntry<T>],
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send {
        self.log.lock().unwrap().extend(entries.iter().cloned());
        async { Ok(()) }
    }

    fn load_log(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Vec<LogEntry<T>>>> + Send {
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
    ) -> impl std::future::Future<Output = StorageResult<Option<(Vec<u8>, LogIndex, Term)>>> + Send
    {
        let snap = self.snapshot.lock().unwrap().clone();
        async move { Ok(snap) }
    }
}

// ---------------------------------------------------------------------------
// Cluster harness
// ---------------------------------------------------------------------------

pub struct ClusterNode {
    handle: RaftHandle<String>,
    shutdown: ShutdownSender,
}

pub struct TestCluster {
    nodes: HashMap<u64, ClusterNode>,
}

pub fn test_config(node_id: u64, peers: Vec<u64>) -> RaftConfig {
    RaftConfig {
        election_timeout_min: Duration::from_millis(150),
        election_timeout_max: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(50),
        node_id: NodeId::new(node_id),
        peers: peers.into_iter().map(NodeId::new).collect(),
        ..RaftConfig::default()
    }
}

impl TestCluster {
    pub async fn start(node_ids: &[u64]) -> Self {
        let mut nodes = HashMap::new();
        let mut outbound_rxs = Vec::new();
        let mut inbound_senders: HashMap<
            u64,
            mpsc::UnboundedSender<(NodeId, RaftMessage<String>)>,
        > = HashMap::new();

        // Build all nodes first.
        for &id in node_ids {
            let peers: Vec<u64> = node_ids.iter().copied().filter(|&p| p != id).collect();
            let config = test_config(id, peers);

            let (mut driver, handle, shutdown, inbound_tx, outbound_rx) =
                RaftBuilder::<String, _, _>::new()
                    .config(config)
                    .storage(MemStorage::new())
                    .state_machine(NullStateMachine)
                    .build()
                    .await
                    .unwrap();

            inbound_senders.insert(id, inbound_tx.clone());
            outbound_rxs.push((id, outbound_rx));
            nodes.insert(id, ClusterNode {
                handle,
                shutdown,
            });

            // Spawn the driver task.
            tokio::spawn(async move {
                driver.run().await;
            });
        }

        // Spawn message routers: forward outbound messages to the target node's inbound channel.
        for (source_id, mut outbound_rx) in outbound_rxs {
            let senders = inbound_senders.clone();
            tokio::spawn(async move {
                while let Some((target, msg)) = outbound_rx.recv().await {
                    let target_id = target.as_u64();
                    if let Some(tx) = senders.get(&target_id) {
                        let _ = tx.send((NodeId::new(source_id), msg));
                    }
                }
            });
        }

        Self { nodes }
    }

    pub fn handle(&self, id: u64) -> &RaftHandle<String> {
        &self.nodes[&id].handle
    }

    pub fn shutdown_all(&self) {
        for node in self.nodes.values() {
            node.shutdown.shutdown();
        }
    }

    /// Disconnect a node by dropping its inbound sender from other nodes'
    /// perspective. We simulate this by sending a shutdown to the node's
    /// driver so it stops processing.
    pub fn kill_node(&self, id: u64) {
        if let Some(node) = self.nodes.get(&id) {
            node.shutdown.shutdown();
        }
    }

    /// Wait until a leader is elected (any node reports `Leader` state).
    /// Returns the leader's node ID.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Option<u64> {
        let deadline = time::Instant::now() + timeout;
        loop {
            for (&id, node) in &self.nodes {
                if node.handle.state() == RaftState::Leader {
                    return Some(id);
                }
            }
            if time::Instant::now() >= deadline {
                return None;
            }
            time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Wait until all live nodes agree on a leader.
    pub async fn wait_for_leader_agreement(&self, timeout: Duration) -> Option<u64> {
        let deadline = time::Instant::now() + timeout;
        loop {
            let mut leaders: Vec<Option<NodeId>> = Vec::new();
            for node in self.nodes.values() {
                leaders.push(node.handle.current_leader());
            }

            // All non-None values should agree.
            let known: Vec<NodeId> = leaders.into_iter().flatten().collect();
            if !known.is_empty() && known.iter().all(|&l| l == known[0]) {
                // Verify the claimed leader actually thinks it's leader.
                let leader_id = known[0].as_u64();
                if let Some(node) = self.nodes.get(&leader_id) {
                    if node.handle.state() == RaftState::Leader {
                        return Some(leader_id);
                    }
                }
            }
            if time::Instant::now() >= deadline {
                return None;
            }
            time::sleep(Duration::from_millis(25)).await;
        }
    }
}

impl Drop for TestCluster {
    fn drop(&mut self) {
        for node in self.nodes.values() {
            node.shutdown.shutdown();
        }
    }
}
