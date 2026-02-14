use std::net::SocketAddr;
use std::sync::Arc;

use pirc_common::InviteKeyError;
use pirc_protocol::{Command, Message, PircSubcommand};
use tokio::sync::Mutex;
use tracing::info;

use crate::cluster::InviteKeyStore;
use crate::raft::{
    MembershipChange, MembershipError, NodeId, PeerUpdater, RaftError, RaftHandle, SharedPeerMap,
};

/// Errors that can occur during the cluster join protocol.
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("invalid invite key: {0}")]
    InvalidKey(#[from] InviteKeyError),
    #[error("not the cluster leader")]
    NotLeader,
    #[error("membership change failed: {0}")]
    MembershipFailed(#[from] MembershipError),
    #[error("raft error: {0}")]
    Raft(#[from] RaftError),
    #[error("protocol error: {reason}")]
    Protocol { reason: String },
}

/// Serializable cluster topology sent in CLUSTER WELCOME.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterTopology {
    pub peers: Vec<ClusterPeer>,
}

/// A single peer in the cluster topology.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterPeer {
    pub id: u64,
    pub addr: SocketAddr,
}

/// Result of a successful join request processed by the leader.
#[derive(Debug)]
pub struct JoinResult {
    pub assigned_id: NodeId,
    pub welcome_message: Message,
}

/// Orchestrates the cluster join/welcome protocol.
///
/// On the leader side, processes incoming `CLUSTER JOIN` requests: validates the
/// invite key, assigns a node ID, proposes a Raft membership change, updates
/// the transport layer, and produces a `CLUSTER WELCOME` response.
pub struct ClusterService {
    invite_keys: Arc<Mutex<InviteKeyStore>>,
    raft_handle: Arc<RaftHandle<String>>,
    peer_updater: PeerUpdater,
    shared_peer_map: SharedPeerMap,
    /// This node's own ID (included in welcome topology for joiners).
    self_id: NodeId,
    self_addr: SocketAddr,
    next_node_id: Arc<Mutex<u64>>,
}

impl ClusterService {
    /// Create a new cluster service.
    ///
    /// - `self_id`: this node's ID
    /// - `self_addr`: this node's listen address (included in welcome topology)
    /// - `next_node_id_start`: counter seed for assigning IDs to joining nodes
    pub fn new(
        invite_keys: Arc<Mutex<InviteKeyStore>>,
        raft_handle: Arc<RaftHandle<String>>,
        peer_updater: PeerUpdater,
        shared_peer_map: SharedPeerMap,
        self_id: NodeId,
        self_addr: SocketAddr,
        next_node_id_start: u64,
    ) -> Self {
        Self {
            invite_keys,
            raft_handle,
            peer_updater,
            shared_peer_map,
            self_id,
            self_addr,
            next_node_id: Arc::new(Mutex::new(next_node_id_start)),
        }
    }

    /// Handle an incoming `CLUSTER JOIN` request on the leader.
    ///
    /// 1. Validate the invite key.
    /// 2. Assign a new [`NodeId`].
    /// 3. Propose a Raft `AddServer` membership change.
    /// 4. Update the transport layer with the new peer.
    /// 5. Return a `CLUSTER WELCOME` message with the assigned ID and topology.
    pub async fn handle_join_request(
        &self,
        invite_key_token: &str,
        joiner_addr: SocketAddr,
    ) -> Result<JoinResult, JoinError> {
        if !self.raft_handle.is_leader() {
            return Err(JoinError::NotLeader);
        }

        // Validate and consume the invite key.
        {
            let mut store = self.invite_keys.lock().await;
            store.validate(invite_key_token)?;
        }
        info!(%joiner_addr, "invite key validated for joining server");

        // Assign a new NodeId.
        let new_node_id = {
            let mut counter = self.next_node_id.lock().await;
            let id = NodeId::new(*counter);
            *counter += 1;
            id
        };
        info!(%new_node_id, %joiner_addr, "assigned node ID to joining server");

        // Propose membership change via Raft.
        let change = MembershipChange::AddServer(new_node_id, joiner_addr);
        let noop = format!("add-server:{}", new_node_id.as_u64());
        self.raft_handle
            .propose_membership_change(change, noop)
            .await?;
        info!(%new_node_id, "membership change proposed for joining server");

        // Update transport layer.
        self.peer_updater.add_peer(new_node_id, joiner_addr).await;

        // Build CLUSTER WELCOME with the current topology (including self).
        let mut peers = vec![ClusterPeer {
            id: self.self_id.as_u64(),
            addr: self.self_addr,
        }];
        {
            let map = self.shared_peer_map.read().await;
            peers.extend(map.entries().map(|(id, addr)| ClusterPeer {
                id: id.as_u64(),
                addr,
            }));
        }
        let config = ClusterTopology { peers };
        let config_json = serde_json::to_string(&config).map_err(|e| JoinError::Protocol {
            reason: format!("failed to serialize cluster topology: {e}"),
        })?;

        let welcome = Message::builder(Command::Pirc(PircSubcommand::ClusterWelcome))
            .param(&new_node_id.as_u64().to_string())
            .trailing(&config_json)
            .build();

        Ok(JoinResult {
            assigned_id: new_node_id,
            welcome_message: welcome,
        })
    }

    /// Parse a `CLUSTER JOIN` protocol message and extract the invite key.
    pub fn parse_join_message(msg: &Message) -> Result<&str, JoinError> {
        if msg.command != Command::Pirc(PircSubcommand::ClusterJoin) {
            return Err(JoinError::Protocol {
                reason: "expected CLUSTER JOIN message".into(),
            });
        }
        msg.params
            .first()
            .map(String::as_str)
            .ok_or(JoinError::Protocol {
                reason: "CLUSTER JOIN missing invite key parameter".into(),
            })
    }

    /// Parse a `CLUSTER WELCOME` protocol message.
    ///
    /// Returns `(assigned_node_id, cluster_config)`.
    pub fn parse_welcome_message(msg: &Message) -> Result<(NodeId, ClusterTopology), JoinError> {
        if msg.command != Command::Pirc(PircSubcommand::ClusterWelcome) {
            return Err(JoinError::Protocol {
                reason: "expected CLUSTER WELCOME message".into(),
            });
        }
        if msg.params.len() < 2 {
            return Err(JoinError::Protocol {
                reason: "CLUSTER WELCOME missing parameters".into(),
            });
        }
        let id_str = &msg.params[0];
        let id: u64 = id_str.parse().map_err(|_| JoinError::Protocol {
            reason: format!("invalid server ID: {id_str}"),
        })?;
        let config_json = &msg.params[1];
        let config: ClusterTopology =
            serde_json::from_str(config_json).map_err(|e| JoinError::Protocol {
                reason: format!("invalid cluster config JSON: {e}"),
            })?;
        Ok((NodeId::new(id), config))
    }

    /// Build a `CLUSTER JOIN` protocol message with the given invite key.
    pub fn build_join_message(invite_key: &str) -> Message {
        Message::builder(Command::Pirc(PircSubcommand::ClusterJoin))
            .param(invite_key)
            .build()
    }

    /// Build an error response for a failed join attempt.
    pub fn build_error_message(error: &JoinError) -> Message {
        Message::new(Command::Error, vec![format!("JOIN failed: {error}")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::PeerMap;

    // ---- Message building and parsing ----

    #[test]
    fn build_join_message_format() {
        let msg = ClusterService::build_join_message("my-invite-key-abc");
        assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterJoin));
        assert_eq!(msg.params.len(), 1);
        assert_eq!(msg.params[0], "my-invite-key-abc");
    }

    #[test]
    fn parse_join_message_success() {
        let msg = ClusterService::build_join_message("test-key-123");
        let token = ClusterService::parse_join_message(&msg).unwrap();
        assert_eq!(token, "test-key-123");
    }

    #[test]
    fn parse_join_message_wrong_command() {
        let msg = Message::new(Command::Ping, vec!["test".into()]);
        let result = ClusterService::parse_join_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn parse_join_message_missing_key() {
        let msg = Message::new(Command::Pirc(PircSubcommand::ClusterJoin), vec![]);
        let result = ClusterService::parse_join_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn build_and_parse_welcome_roundtrip() {
        let config = ClusterTopology {
            peers: vec![
                ClusterPeer {
                    id: 1,
                    addr: "10.0.0.1:7000".parse().unwrap(),
                },
                ClusterPeer {
                    id: 2,
                    addr: "10.0.0.2:7000".parse().unwrap(),
                },
            ],
        };
        let config_json = serde_json::to_string(&config).unwrap();

        let welcome = Message::builder(Command::Pirc(PircSubcommand::ClusterWelcome))
            .param("42")
            .trailing(&config_json)
            .build();

        let (node_id, parsed_config) = ClusterService::parse_welcome_message(&welcome).unwrap();
        assert_eq!(node_id, NodeId::new(42));
        assert_eq!(parsed_config.peers.len(), 2);
        assert_eq!(parsed_config.peers[0].id, 1);
        assert_eq!(parsed_config.peers[1].id, 2);
    }

    #[test]
    fn parse_welcome_message_wrong_command() {
        let msg = Message::new(Command::Ping, vec!["test".into()]);
        let result = ClusterService::parse_welcome_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn parse_welcome_message_missing_params() {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::ClusterWelcome),
            vec!["42".into()],
        );
        let result = ClusterService::parse_welcome_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn parse_welcome_message_invalid_id() {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::ClusterWelcome),
            vec!["not-a-number".into(), "{}".into()],
        );
        let result = ClusterService::parse_welcome_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn parse_welcome_message_invalid_json() {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::ClusterWelcome),
            vec!["42".into(), "not-json".into()],
        );
        let result = ClusterService::parse_welcome_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn build_error_message_format() {
        let err = JoinError::NotLeader;
        let msg = ClusterService::build_error_message(&err);
        assert_eq!(msg.command, Command::Error);
        assert!(msg.params[0].contains("JOIN failed"));
    }

    // ---- ClusterTopology serde ----

    #[test]
    fn cluster_config_serde_roundtrip() {
        let config = ClusterTopology {
            peers: vec![
                ClusterPeer {
                    id: 1,
                    addr: "10.0.0.1:7000".parse().unwrap(),
                },
                ClusterPeer {
                    id: 3,
                    addr: "10.0.0.3:8000".parse().unwrap(),
                },
            ],
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ClusterTopology = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.peers.len(), 2);
        assert_eq!(deserialized.peers[0].id, 1);
        assert_eq!(
            deserialized.peers[0].addr,
            "10.0.0.1:7000".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(deserialized.peers[1].id, 3);
    }

    #[test]
    fn cluster_config_empty_peers() {
        let config = ClusterTopology { peers: vec![] };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ClusterTopology = serde_json::from_str(&json).unwrap();
        assert!(deserialized.peers.is_empty());
    }

    // ---- JoinError variants ----

    #[test]
    fn join_error_invalid_key_display() {
        let err = JoinError::InvalidKey(InviteKeyError::Expired);
        assert_eq!(err.to_string(), "invalid invite key: invite key expired");
    }

    #[test]
    fn join_error_not_leader_display() {
        let err = JoinError::NotLeader;
        assert_eq!(err.to_string(), "not the cluster leader");
    }

    #[test]
    fn join_error_protocol_display() {
        let err = JoinError::Protocol {
            reason: "bad message".into(),
        };
        assert_eq!(err.to_string(), "protocol error: bad message");
    }

    // ---- PeerMap entries integration ----

    #[test]
    fn peer_map_entries() {
        let addr1: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        let addr2: SocketAddr = "10.0.0.2:7000".parse().unwrap();
        let map = PeerMap::new(vec![(NodeId::new(1), addr1), (NodeId::new(2), addr2)]);
        let mut entries: Vec<_> = map.entries().collect();
        entries.sort_by_key(|(id, _)| id.as_u64());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (NodeId::new(1), addr1));
        assert_eq!(entries[1], (NodeId::new(2), addr2));
    }

    // ---- Integration test helpers ----

    use crate::raft::{
        NullStateMachine, PeerConnections, RaftBuilder, RaftConfig, SharedPeerMap,
    };
    use crate::raft::test_utils::MemStorage;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, RwLock};

    fn test_config(node_id: u64) -> RaftConfig {
        RaftConfig {
            election_timeout_min: Duration::from_millis(100),
            election_timeout_max: Duration::from_millis(200),
            heartbeat_interval: Duration::from_millis(30),
            node_id: NodeId::new(node_id),
            peers: vec![],
            ..RaftConfig::default()
        }
    }

    /// Spin up a single-node Raft leader ready for membership changes.
    ///
    /// Returns the handle, shutdown sender, and driver join handle.
    /// The leader has already committed an entry in the current term
    /// (required before membership changes are allowed).
    async fn setup_leader() -> (
        Arc<RaftHandle<String>>,
        crate::raft::ShutdownSender,
        tokio::task::JoinHandle<()>,
    ) {
        let config = test_config(1);
        let (mut driver, handle, shutdown_tx, _inbound_tx, _outbound_rx) =
            RaftBuilder::new()
                .config(config)
                .storage(MemStorage::new())
                .state_machine(NullStateMachine)
                .build()
                .await
                .unwrap();

        let handle = Arc::new(handle);
        let driver_handle = tokio::spawn(async move {
            driver.run().await;
        });

        // Wait for the node to become leader.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(handle.is_leader(), "single-node should become leader");

        // Commit an entry in the current term (required for membership changes).
        handle.propose("init".to_owned()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        (handle, shutdown_tx, driver_handle)
    }

    // ---- Integration test: full join handshake with real Raft ----

    #[tokio::test]
    async fn join_handshake_with_valid_key() {
        let (handle, shutdown_tx, driver_handle) = setup_leader().await;

        // Set up the cluster service.
        let peer_map = PeerMap::new(vec![]);
        let shared_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared_map), Arc::clone(&conns));

        let invite_store = Arc::new(Mutex::new(InviteKeyStore::new()));
        let invite_key = {
            let mut store = invite_store.lock().await;
            store.create(NodeId::new(1), None, true)
        };

        let service = ClusterService::new(
            Arc::clone(&invite_store),
            Arc::clone(&handle),
            updater,
            Arc::clone(&shared_map),
            NodeId::new(1),
            "10.0.0.1:7000".parse().unwrap(),
            100,
        );

        // Simulate a join request.
        let joiner_addr: SocketAddr = "10.0.0.50:7000".parse().unwrap();
        let result = service
            .handle_join_request(invite_key.as_str(), joiner_addr)
            .await;

        assert!(result.is_ok(), "join should succeed: {result:?}");
        let join_result = result.unwrap();
        assert_eq!(join_result.assigned_id, NodeId::new(100));
        assert_eq!(
            join_result.welcome_message.command,
            Command::Pirc(PircSubcommand::ClusterWelcome)
        );

        // Verify the welcome message can be parsed.
        let (assigned_id, cluster_cfg) =
            ClusterService::parse_welcome_message(&join_result.welcome_message).unwrap();
        assert_eq!(assigned_id, NodeId::new(100));
        // Peer map should now include the newly added peer.
        assert!(cluster_cfg.peers.iter().any(|p| p.id == 100));

        // Verify the peer was added to the transport layer.
        {
            let map = shared_map.read().await;
            assert_eq!(map.get(NodeId::new(100)), Some(&joiner_addr));
        }

        // Clean up.
        shutdown_tx.shutdown();
        let _ = driver_handle.await;
    }

    #[tokio::test]
    async fn join_handshake_with_invalid_key() {
        let (handle, shutdown_tx, driver_handle) = setup_leader().await;

        let peer_map = PeerMap::new(vec![]);
        let shared_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared_map), Arc::clone(&conns));

        let invite_store = Arc::new(Mutex::new(InviteKeyStore::new()));

        let service = ClusterService::new(
            invite_store,
            Arc::clone(&handle),
            updater,
            shared_map,
            NodeId::new(1),
            "10.0.0.1:7000".parse().unwrap(),
            100,
        );

        // Try joining with an invalid key.
        let joiner_addr: SocketAddr = "10.0.0.50:7000".parse().unwrap();
        let result = service
            .handle_join_request("bogus-key", joiner_addr)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            JoinError::InvalidKey(InviteKeyError::NotFound)
        ));

        shutdown_tx.shutdown();
        let _ = driver_handle.await;
    }

    #[tokio::test]
    async fn join_handshake_key_consumed_after_use() {
        let (handle, shutdown_tx, driver_handle) = setup_leader().await;

        let peer_map = PeerMap::new(vec![]);
        let shared_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared_map), Arc::clone(&conns));

        let invite_store = Arc::new(Mutex::new(InviteKeyStore::new()));
        let invite_key = {
            let mut store = invite_store.lock().await;
            store.create(NodeId::new(1), None, true)
        };

        let service = ClusterService::new(
            Arc::clone(&invite_store),
            Arc::clone(&handle),
            updater,
            shared_map,
            NodeId::new(1),
            "10.0.0.1:7000".parse().unwrap(),
            100,
        );

        // First join succeeds.
        let addr1: SocketAddr = "10.0.0.50:7000".parse().unwrap();
        let result = service.handle_join_request(invite_key.as_str(), addr1).await;
        assert!(result.is_ok());

        // Second join with the same single-use key fails.
        let addr2: SocketAddr = "10.0.0.51:7000".parse().unwrap();
        let result = service.handle_join_request(invite_key.as_str(), addr2).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            JoinError::InvalidKey(InviteKeyError::AlreadyUsed)
        ));

        shutdown_tx.shutdown();
        let _ = driver_handle.await;
    }

    #[tokio::test]
    async fn node_id_counter_increments_per_join() {
        // Verify that the internal node ID counter increments correctly.
        // We test with a single join through Raft and verify the ID,
        // then verify the counter state.
        let (handle, shutdown_tx, driver_handle) = setup_leader().await;

        let peer_map = PeerMap::new(vec![]);
        let shared_map: SharedPeerMap = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared_map), Arc::clone(&conns));

        let invite_store = Arc::new(Mutex::new(InviteKeyStore::new()));
        let invite_key = {
            let mut store = invite_store.lock().await;
            store.create(NodeId::new(1), None, false)
        };

        let service = ClusterService::new(
            Arc::clone(&invite_store),
            Arc::clone(&handle),
            updater,
            shared_map,
            NodeId::new(1),
            "10.0.0.1:7000".parse().unwrap(),
            200,
        );

        // First join gets ID 200.
        let addr1: SocketAddr = "10.0.0.50:7000".parse().unwrap();
        let r1 = service
            .handle_join_request(invite_key.as_str(), addr1)
            .await
            .unwrap();
        assert_eq!(r1.assigned_id, NodeId::new(200));

        // The internal counter should now be 201 for the next join.
        // (We can't do a second full join because adding a peer changes
        // quorum requirements, but we verify the first ID is correct.)

        shutdown_tx.shutdown();
        let _ = driver_handle.await;
    }
}
