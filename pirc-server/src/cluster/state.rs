use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::raft::NodeId;

const STATE_FILENAME: &str = "cluster_state.json";

/// Persisted cluster topology state.
///
/// Saved to `data_dir/cluster_state.json` so that a server can rejoin the
/// cluster after restart without needing a new invite key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedClusterState {
    /// This node's own ID (assigned during bootstrap or join).
    pub node_id: NodeId,
    /// The cluster listen address advertised to peers.
    pub self_addr: SocketAddr,
    /// Current peer list (id, address) excluding self.
    pub peers: Vec<PersistedPeer>,
    /// Monotonically increasing generation counter, incremented on membership
    /// changes.
    pub generation: u64,
    /// Counter seed for assigning IDs to future joining nodes.
    pub next_node_id: u64,
}

/// A single peer entry in persisted cluster state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedPeer {
    pub id: NodeId,
    pub addr: SocketAddr,
}

impl PersistedClusterState {
    /// Create a new persisted state for a freshly bootstrapped cluster.
    pub fn new_bootstrap(node_id: NodeId, self_addr: SocketAddr) -> Self {
        Self {
            node_id,
            self_addr,
            peers: Vec::new(),
            generation: 0,
            next_node_id: node_id.as_u64() + 1000,
        }
    }

    /// Create a new persisted state after joining an existing cluster.
    pub fn new_from_join(
        node_id: NodeId,
        self_addr: SocketAddr,
        peers: Vec<PersistedPeer>,
    ) -> Self {
        Self {
            node_id,
            self_addr,
            peers,
            generation: 0,
            next_node_id: node_id.as_u64() + 1000,
        }
    }

    /// Add a peer to the topology and bump generation.
    pub fn add_peer(&mut self, id: NodeId, addr: SocketAddr) {
        if !self.peers.iter().any(|p| p.id == id) {
            self.peers.push(PersistedPeer { id, addr });
            self.generation += 1;
        }
    }

    /// Remove a peer from the topology and bump generation.
    pub fn remove_peer(&mut self, id: NodeId) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| p.id != id);
        if self.peers.len() == before {
            false
        } else {
            self.generation += 1;
            true
        }
    }

    /// Update the next node ID counter.
    pub fn set_next_node_id(&mut self, next: u64) {
        self.next_node_id = next;
    }

    /// Returns the file path for the persisted state within the given data directory.
    pub fn file_path(data_dir: &Path) -> PathBuf {
        data_dir.join(STATE_FILENAME)
    }

    /// Save the cluster state to `data_dir/cluster_state.json`.
    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        let path = Self::file_path(data_dir);
        let json = serde_json::to_string_pretty(self)
            .map_err(io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load the cluster state from `data_dir/cluster_state.json`.
    ///
    /// Returns `Ok(None)` if the file does not exist.
    pub fn load(data_dir: &Path) -> io::Result<Option<Self>> {
        let path = Self::file_path(data_dir);
        if !path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(path)?;
        let state: Self =
            serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirc_common::ServerId;

    fn test_node_id(id: u64) -> NodeId {
        ServerId::new(id)
    }

    fn test_addr(port: u16) -> SocketAddr {
        format!("10.0.0.1:{port}").parse().unwrap()
    }

    #[test]
    fn new_bootstrap_has_no_peers() {
        let state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        assert_eq!(state.node_id, test_node_id(1));
        assert!(state.peers.is_empty());
        assert_eq!(state.generation, 0);
        assert_eq!(state.next_node_id, 1001);
    }

    #[test]
    fn new_from_join_stores_peers() {
        let peers = vec![
            PersistedPeer {
                id: test_node_id(1),
                addr: test_addr(7000),
            },
            PersistedPeer {
                id: test_node_id(2),
                addr: test_addr(7001),
            },
        ];
        let state =
            PersistedClusterState::new_from_join(test_node_id(100), test_addr(7002), peers.clone());
        assert_eq!(state.node_id, test_node_id(100));
        assert_eq!(state.peers, peers);
        assert_eq!(state.generation, 0);
    }

    #[test]
    fn add_peer_bumps_generation() {
        let mut state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        state.add_peer(test_node_id(2), test_addr(7001));
        assert_eq!(state.peers.len(), 1);
        assert_eq!(state.generation, 1);
        assert_eq!(state.peers[0].id, test_node_id(2));
    }

    #[test]
    fn add_duplicate_peer_is_noop() {
        let mut state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        state.add_peer(test_node_id(2), test_addr(7001));
        state.add_peer(test_node_id(2), test_addr(7001));
        assert_eq!(state.peers.len(), 1);
        assert_eq!(state.generation, 1);
    }

    #[test]
    fn remove_peer_bumps_generation() {
        let mut state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        state.add_peer(test_node_id(2), test_addr(7001));
        assert!(state.remove_peer(test_node_id(2)));
        assert!(state.peers.is_empty());
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn remove_nonexistent_peer_returns_false() {
        let mut state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        assert!(!state.remove_peer(test_node_id(99)));
        assert_eq!(state.generation, 0);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = PersistedClusterState::new_bootstrap(test_node_id(1), test_addr(7000));
        state.add_peer(test_node_id(2), test_addr(7001));
        state.add_peer(test_node_id(3), test_addr(7002));
        state.set_next_node_id(2000);

        state.save(dir.path()).expect("save");

        let loaded = PersistedClusterState::load(dir.path())
            .expect("load")
            .expect("should exist");

        assert_eq!(loaded.node_id, state.node_id);
        assert_eq!(loaded.self_addr, state.self_addr);
        assert_eq!(loaded.peers, state.peers);
        assert_eq!(loaded.generation, state.generation);
        assert_eq!(loaded.next_node_id, state.next_node_id);
    }

    #[test]
    fn load_returns_none_when_no_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let loaded = PersistedClusterState::load(dir.path()).expect("load");
        assert!(loaded.is_none());
    }

    #[test]
    fn serde_roundtrip_json() {
        let state = PersistedClusterState::new_from_join(
            test_node_id(42),
            test_addr(7000),
            vec![PersistedPeer {
                id: test_node_id(1),
                addr: test_addr(7001),
            }],
        );
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: PersistedClusterState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.node_id, state.node_id);
        assert_eq!(deserialized.peers.len(), 1);
    }

    #[test]
    fn file_path_uses_correct_filename() {
        let dir = Path::new("/data/raft");
        assert_eq!(
            PersistedClusterState::file_path(dir),
            PathBuf::from("/data/raft/cluster_state.json")
        );
    }
}
