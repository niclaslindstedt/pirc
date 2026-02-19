//! Shared cluster test harness for Raft integration tests.
//!
//! Provides [`MemStorage`], [`TestCluster`], [`ClusterNode`], and
//! [`test_config`] used by both `raft_cluster` and `cluster_failover`
//! test suites.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use pirc_server::raft::storage::RaftStorage;
use pirc_server::raft::types::{NodeId, Term};
use pirc_server::raft::{
    ClusterCommand, ClusterStateMachine, LogEntry, LogIndex, NullStateMachine, RaftBuilder,
    RaftConfig, RaftHandle, RaftMessage, RaftState, ShutdownSender, StateMachine, StorageResult,
};

// ---------------------------------------------------------------------------
// In-memory storage for integration tests
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
// Cluster harness (generic over command type T)
// ---------------------------------------------------------------------------

pub struct ClusterNode<T: Send + 'static> {
    handle: RaftHandle<T>,
    shutdown: ShutdownSender,
}

pub struct TestCluster<T: Send + 'static> {
    nodes: HashMap<u64, ClusterNode<T>>,
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

impl<T> TestCluster<T>
where
    T: Clone
        + PartialEq
        + Send
        + Sync
        + serde::Serialize
        + serde::de::DeserializeOwned
        + 'static,
{
    pub async fn start_with<M, F>(node_ids: &[u64], state_machine_factory: F) -> Self
    where
        M: StateMachine<T> + Send + Sync + 'static,
        F: Fn() -> M,
    {
        let mut nodes = HashMap::new();
        let mut outbound_rxs = Vec::new();
        let mut inbound_senders: HashMap<
            u64,
            mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
        > = HashMap::new();

        for &id in node_ids {
            let peers: Vec<u64> = node_ids.iter().copied().filter(|&p| p != id).collect();
            let config = test_config(id, peers);

            let (mut driver, handle, shutdown, inbound_tx, outbound_rx) =
                RaftBuilder::<T, _, _>::new()
                    .config(config)
                    .storage(MemStorage::<T>::new())
                    .state_machine(state_machine_factory())
                    .build()
                    .await
                    .unwrap();

            inbound_senders.insert(id, inbound_tx.clone());
            outbound_rxs.push((id, outbound_rx));
            nodes.insert(id, ClusterNode { handle, shutdown });

            tokio::spawn(async move {
                driver.run().await;
            });
        }

        // Spawn message routers.
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

    pub fn handle(&self, id: u64) -> &RaftHandle<T> {
        &self.nodes[&id].handle
    }

    pub fn handle_mut(&mut self, id: u64) -> &mut RaftHandle<T> {
        &mut self.nodes.get_mut(&id).unwrap().handle
    }

    pub fn shutdown_all(&self) {
        for node in self.nodes.values() {
            node.shutdown.shutdown();
        }
    }

    /// Simulate killing a node by shutting down its driver.
    pub fn kill_node(&self, id: u64) {
        if let Some(node) = self.nodes.get(&id) {
            node.shutdown.shutdown();
        }
    }

    /// Wait until a leader is elected among live nodes.
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

    /// Wait until a leader is elected among a specific set of nodes.
    pub async fn wait_for_leader_among(
        &self,
        candidates: &[u64],
        timeout: Duration,
    ) -> Option<u64> {
        let deadline = time::Instant::now() + timeout;
        loop {
            for &id in candidates {
                if let Some(node) = self.nodes.get(&id) {
                    if node.handle.state() == RaftState::Leader {
                        return Some(id);
                    }
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

            let known: Vec<NodeId> = leaders.into_iter().flatten().collect();
            if !known.is_empty() && known.iter().all(|&l| l == known[0]) {
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

    /// Get node IDs excluding a specific set.
    pub fn remaining_nodes(&self, exclude: &[u64]) -> Vec<u64> {
        self.nodes
            .keys()
            .copied()
            .filter(|id| !exclude.contains(id))
            .collect()
    }
}

impl<T: Send + 'static> Drop for TestCluster<T> {
    fn drop(&mut self) {
        for node in self.nodes.values() {
            node.shutdown.shutdown();
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience constructors for common test configurations
// ---------------------------------------------------------------------------

impl TestCluster<String> {
    /// Start a cluster with `NullStateMachine` (for basic Raft consensus tests).
    pub async fn start(node_ids: &[u64]) -> Self {
        Self::start_with(node_ids, || NullStateMachine).await
    }
}

impl TestCluster<ClusterCommand> {
    /// Start a cluster with `ClusterStateMachine` (for cluster failover tests).
    pub async fn start(node_ids: &[u64]) -> Self {
        Self::start_with(node_ids, ClusterStateMachine::new).await
    }
}

// ---------------------------------------------------------------------------
// Type aliases for test modules
// ---------------------------------------------------------------------------

/// Alias for basic Raft consensus tests (uses `String` commands).
pub type RaftTestCluster = TestCluster<String>;

/// Alias for cluster failover tests (uses `ClusterCommand`).
pub type FailoverTestCluster = TestCluster<ClusterCommand>;
