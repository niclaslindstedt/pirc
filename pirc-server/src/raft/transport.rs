//! Raft transport bridge between the Raft driver and the network layer.
//!
//! Manages outbound Raft message delivery over TCP connections to peer nodes,
//! and inbound message routing from peer connections to the Raft driver.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Connector};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use super::rpc::RaftMessage;
use super::types::NodeId;

/// A thread-safe, dynamically updatable peer map.
///
/// Shared between the peer listener and [`PeerUpdater`] so that newly added
/// peers are recognized on inbound connections at runtime.
pub type SharedPeerMap = Arc<RwLock<PeerMap>>;

/// Maps node IDs to their network addresses.
#[derive(Debug, Clone)]
pub struct PeerMap {
    peers: HashMap<NodeId, SocketAddr>,
}

impl PeerMap {
    /// Create a new peer map from an iterator of `(NodeId, SocketAddr)` pairs.
    pub fn new(entries: impl IntoIterator<Item = (NodeId, SocketAddr)>) -> Self {
        Self {
            peers: entries.into_iter().collect(),
        }
    }

    /// Look up the address for a given node ID.
    pub fn get(&self, id: NodeId) -> Option<&SocketAddr> {
        self.peers.get(&id)
    }

    /// Insert a new peer or update an existing peer's address.
    ///
    /// Returns the previous address if the node was already present.
    pub fn insert(&mut self, id: NodeId, addr: SocketAddr) -> Option<SocketAddr> {
        self.peers.insert(id, addr)
    }

    /// Remove a peer by node ID.
    ///
    /// Returns the address if the node was present.
    pub fn remove(&mut self, id: NodeId) -> Option<SocketAddr> {
        self.peers.remove(&id)
    }

    /// Returns the number of peers in the map.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Returns `true` if the map contains no peers.
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Returns all peer node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.keys().copied()
    }

    /// Returns all `(NodeId, SocketAddr)` entries.
    pub fn entries(&self) -> impl Iterator<Item = (NodeId, SocketAddr)> + '_ {
        self.peers.iter().map(|(&id, &addr)| (id, addr))
    }
}

/// Manages TCP connections to Raft peer nodes.
///
/// Connections are established lazily on first send and reconnected
/// automatically when a send fails.
pub struct PeerConnections {
    peers: PeerMap,
    connections: HashMap<NodeId, Connection>,
    connector: Connector,
}

impl PeerConnections {
    /// Create a new connection manager for the given peers.
    pub fn new(peers: PeerMap) -> Self {
        Self {
            peers,
            connections: HashMap::new(),
            connector: Connector::new(),
        }
    }

    /// Send a protocol message to a specific peer.
    ///
    /// Lazily connects if no connection exists. If the send fails, the
    /// connection is dropped so a fresh one will be established on the
    /// next attempt.
    pub async fn send_to(
        &mut self,
        target: NodeId,
        msg: pirc_protocol::Message,
    ) -> Result<(), TransportError> {
        let addr = self
            .peers
            .get(target)
            .copied()
            .ok_or(TransportError::UnknownPeer(target))?;

        // Ensure we have a connection.
        if !self.connections.contains_key(&target) {
            match self.connector.connect(addr).await {
                Ok(conn) => {
                    debug!(%target, %addr, "connected to peer");
                    self.connections.insert(target, conn);
                }
                Err(e) => {
                    warn!(%target, %addr, error = %e, "failed to connect to peer");
                    return Err(TransportError::ConnectionFailed(target));
                }
            }
        }

        // Send the message.
        let conn = self.connections.get_mut(&target).expect("just inserted");
        if let Err(e) = conn.send(msg).await {
            warn!(%target, error = %e, "send to peer failed, dropping connection");
            self.connections.remove(&target);
            return Err(TransportError::SendFailed(target));
        }

        Ok(())
    }

    /// Add a new peer to the connection manager.
    ///
    /// If the peer already exists, its address is updated and any existing
    /// connection is dropped so a fresh one will be established on next send.
    pub fn add_peer(&mut self, id: NodeId, addr: SocketAddr) {
        self.peers.insert(id, addr);
        // Drop stale connection if address changed.
        self.connections.remove(&id);
    }

    /// Remove a peer from the connection manager.
    ///
    /// Drops the peer's address mapping and any active connection.
    pub fn remove_peer(&mut self, id: NodeId) {
        self.peers.remove(id);
        self.connections.remove(&id);
    }
}

/// Errors from the Raft transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("unknown peer: {0}")]
    UnknownPeer(NodeId),
    #[error("failed to connect to peer: {0}")]
    ConnectionFailed(NodeId),
    #[error("failed to send to peer: {0}")]
    SendFailed(NodeId),
}

/// Handle for dynamically adding and removing peers at runtime.
///
/// Holds references to both the shared peer map (used by the peer listener for
/// inbound connection identification) and the peer connections (used for
/// outbound message delivery). When a peer is added or removed through this
/// handle, both data structures are updated atomically so the change is
/// reflected in both directions.
#[derive(Clone)]
pub struct PeerUpdater {
    shared_peer_map: SharedPeerMap,
    peer_connections: Arc<Mutex<PeerConnections>>,
}

impl PeerUpdater {
    /// Create a new peer updater.
    pub fn new(
        shared_peer_map: SharedPeerMap,
        peer_connections: Arc<Mutex<PeerConnections>>,
    ) -> Self {
        Self {
            shared_peer_map,
            peer_connections,
        }
    }

    /// Add a new peer to the transport layer.
    ///
    /// Updates both the shared peer map (for inbound identification) and the
    /// peer connections (for outbound delivery).
    pub async fn add_peer(&self, id: NodeId, addr: SocketAddr) {
        {
            let mut map = self.shared_peer_map.write().await;
            map.insert(id, addr);
        }
        {
            let mut conns = self.peer_connections.lock().await;
            conns.add_peer(id, addr);
        }
        info!(%id, %addr, "peer added to transport layer");
    }

    /// Remove a peer from the transport layer.
    ///
    /// Removes from both the shared peer map and the peer connections,
    /// dropping any active connection to the peer.
    pub async fn remove_peer(&self, id: NodeId) {
        {
            let mut map = self.shared_peer_map.write().await;
            map.remove(id);
        }
        {
            let mut conns = self.peer_connections.lock().await;
            conns.remove_peer(id);
        }
        info!(%id, "peer removed from transport layer");
    }
}

/// Spawns the outbound transport task that reads messages from the Raft driver
/// and sends them to the appropriate peer over TCP.
///
/// Returns a [`tokio::task::JoinHandle`] for the spawned task.
pub fn spawn_outbound_transport<T>(
    mut outbound_rx: mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
    peer_connections: Arc<Mutex<PeerConnections>>,
) -> tokio::task::JoinHandle<()>
where
    T: Clone
        + PartialEq
        + Send
        + Sync
        + serde::Serialize
        + serde::de::DeserializeOwned
        + 'static,
{
    tokio::spawn(async move {
        info!("raft outbound transport started");
        while let Some((target, raft_msg)) = outbound_rx.recv().await {
            let proto_msg = match raft_msg.to_protocol_message() {
                Ok(m) => m,
                Err(e) => {
                    error!(error = %e, "failed to serialize raft message");
                    continue;
                }
            };

            let mut conns = peer_connections.lock().await;
            if let Err(e) = conns.send_to(target, proto_msg).await {
                debug!(%target, error = %e, "outbound raft message dropped");
            }
        }
        info!("raft outbound transport stopped");
    })
}

/// Spawns the inbound transport task for a single peer connection.
///
/// Reads messages from the TCP connection, deserializes them as Raft messages,
/// and forwards them to the Raft driver's inbound channel.
pub fn spawn_inbound_handler<T>(
    from: NodeId,
    mut connection: Connection,
    inbound_tx: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
) -> tokio::task::JoinHandle<()>
where
    T: Clone
        + PartialEq
        + Send
        + Sync
        + serde::Serialize
        + serde::de::DeserializeOwned
        + 'static,
{
    tokio::spawn(async move {
        debug!(%from, "inbound handler started for peer");
        loop {
            match connection.recv().await {
                Ok(Some(msg)) => {
                    match RaftMessage::from_protocol_message(&msg) {
                        Ok(raft_msg) => {
                            if inbound_tx.send((from, raft_msg)).is_err() {
                                debug!(%from, "inbound channel closed, stopping handler");
                                break;
                            }
                        }
                        Err(e) => {
                            // Not a raft message — ignore (could be other PIRC traffic).
                            debug!(%from, error = %e, "ignoring non-raft message from peer");
                        }
                    }
                }
                Ok(None) => {
                    info!(%from, "peer connection closed");
                    break;
                }
                Err(e) => {
                    warn!(%from, error = %e, "error reading from peer");
                    break;
                }
            }
        }
        debug!(%from, "inbound handler stopped for peer");
    })
}

/// Spawns a listener task that accepts incoming peer connections on the Raft
/// port and creates inbound handlers for each.
///
/// Uses a [`SharedPeerMap`] so that peers added at runtime via [`PeerUpdater`]
/// are recognized on inbound connections without restarting the listener.
pub fn spawn_peer_listener<T>(
    listener: pirc_network::Listener,
    inbound_tx: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
    shared_peer_map: SharedPeerMap,
    mut shutdown: pirc_network::ShutdownSignal,
) -> tokio::task::JoinHandle<()>
where
    T: Clone
        + PartialEq
        + Send
        + Sync
        + serde::Serialize
        + serde::de::DeserializeOwned
        + 'static,
{
    tokio::spawn(async move {
        info!("raft peer listener started");
        loop {
            match listener.accept_with_shutdown(&mut shutdown).await {
                Ok(Some((connection, peer_addr))) => {
                    // Look up the peer IP in the shared map on every accept
                    // so that dynamically added peers are recognized.
                    let node_id = {
                        let map = shared_peer_map.read().await;
                        let found = map.node_ids().find(|id| {
                            map.get(*id)
                                .is_some_and(|addr| addr.ip() == peer_addr.ip())
                        });
                        found
                    };

                    let node_id = node_id.unwrap_or_else(|| {
                        // Unknown peer IP — assign a synthetic ID from the address.
                        warn!(%peer_addr, "accepted connection from unknown peer IP");
                        NodeId::new(u64::from(match peer_addr.ip() {
                            std::net::IpAddr::V4(ip) => u32::from(ip),
                            std::net::IpAddr::V6(_) => 0,
                        }))
                    });

                    info!(%node_id, %peer_addr, "accepted peer connection");
                    spawn_inbound_handler::<T>(node_id, connection, inbound_tx.clone());
                }
                Ok(None) => {
                    info!("raft peer listener shutting down");
                    break;
                }
                Err(e) => {
                    warn!(error = %e, "failed to accept peer connection");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirc_protocol::{Command, PircSubcommand};

    #[test]
    fn peer_map_lookup() {
        let addr1: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        let addr2: SocketAddr = "10.0.0.2:7000".parse().unwrap();
        let map = PeerMap::new(vec![(NodeId::new(1), addr1), (NodeId::new(2), addr2)]);

        assert_eq!(map.get(NodeId::new(1)), Some(&addr1));
        assert_eq!(map.get(NodeId::new(2)), Some(&addr2));
        assert_eq!(map.get(NodeId::new(3)), None);
    }

    #[test]
    fn peer_map_node_ids() {
        let map = PeerMap::new(vec![
            (NodeId::new(1), "10.0.0.1:7000".parse().unwrap()),
            (NodeId::new(2), "10.0.0.2:7000".parse().unwrap()),
        ]);
        let mut ids: Vec<_> = map.node_ids().collect();
        ids.sort();
        assert_eq!(ids, vec![NodeId::new(1), NodeId::new(2)]);
    }

    #[tokio::test]
    async fn outbound_transport_serializes_and_sends() {
        use tokio::net::TcpListener;
        use tokio::net::TcpStream;

        // Set up a TCP listener to act as a peer.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let peer_id = NodeId::new(42);
        let peer_map = PeerMap::new(vec![(peer_id, addr)]);
        let peer_conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));

        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();

        let handle = spawn_outbound_transport::<String>(outbound_rx, peer_conns);

        // Accept the connection from the peer side.
        let accept_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            Connection::new(stream).unwrap()
        });

        // Send a raft message via the outbound channel.
        let raft_msg: RaftMessage<String> =
            RaftMessage::RequestVote(super::super::rpc::RequestVote {
                term: super::super::types::Term::new(1),
                candidate_id: NodeId::new(1),
                last_log_index: super::super::types::LogIndex::new(0),
                last_log_term: super::super::types::Term::new(0),
            });
        outbound_tx.send((peer_id, raft_msg.clone())).unwrap();

        // Give the transport time to connect and send.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Read from the peer side.
        let mut peer_conn = accept_handle.await.unwrap();
        let received = peer_conn.recv().await.unwrap().unwrap();

        // Verify it's a PIRC CLUSTER RAFT message.
        assert_eq!(
            received.command,
            Command::Pirc(PircSubcommand::ClusterRaft)
        );

        // Verify we can deserialize it back.
        let decoded: RaftMessage<String> =
            RaftMessage::from_protocol_message(&received).unwrap();
        assert_eq!(decoded, raft_msg);

        // Shut down.
        drop(outbound_tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn inbound_handler_forwards_raft_messages() {
        use tokio::net::TcpListener;
        use tokio::net::TcpStream;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel();
        let peer_id = NodeId::new(99);

        // Connect client side.
        let mut client = {
            let stream = TcpStream::connect(addr).await.unwrap();
            Connection::new(stream).unwrap()
        };

        // Accept server side and spawn inbound handler.
        let (server_stream, _) = listener.accept().await.unwrap();
        let server_conn = Connection::new(server_stream).unwrap();
        let handle = spawn_inbound_handler::<String>(peer_id, server_conn, inbound_tx);

        // Send a raft message from the client side.
        let raft_msg: RaftMessage<String> =
            RaftMessage::RequestVote(super::super::rpc::RequestVote {
                term: super::super::types::Term::new(2),
                candidate_id: NodeId::new(99),
                last_log_index: super::super::types::LogIndex::new(5),
                last_log_term: super::super::types::Term::new(1),
            });
        let proto = raft_msg.to_protocol_message().unwrap();
        client.send(proto).await.unwrap();

        // Receive from the inbound channel.
        let (from, received) = inbound_rx.recv().await.unwrap();
        assert_eq!(from, peer_id);
        assert_eq!(received, raft_msg);

        // Close and verify handler shuts down.
        drop(client);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn peer_connections_send_to_unknown_peer() {
        let peer_map = PeerMap::new(vec![]);
        let mut conns = PeerConnections::new(peer_map);
        let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
        let result = conns.send_to(NodeId::new(1), msg).await;
        assert!(matches!(result, Err(TransportError::UnknownPeer(_))));
    }

    #[test]
    fn peer_map_insert() {
        let mut map = PeerMap::new(vec![]);
        assert!(map.is_empty());

        let addr: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        assert!(map.insert(NodeId::new(1), addr).is_none());
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(NodeId::new(1)), Some(&addr));

        // Inserting same key with different address returns old address.
        let addr2: SocketAddr = "10.0.0.1:8000".parse().unwrap();
        assert_eq!(map.insert(NodeId::new(1), addr2), Some(addr));
        assert_eq!(map.get(NodeId::new(1)), Some(&addr2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn peer_map_remove() {
        let addr: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        let mut map = PeerMap::new(vec![(NodeId::new(1), addr)]);
        assert_eq!(map.len(), 1);

        assert_eq!(map.remove(NodeId::new(1)), Some(addr));
        assert!(map.is_empty());
        assert_eq!(map.get(NodeId::new(1)), None);

        // Removing non-existent key returns None.
        assert_eq!(map.remove(NodeId::new(99)), None);
    }

    #[test]
    fn peer_map_len_and_is_empty() {
        let mut map = PeerMap::new(vec![]);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        map.insert(NodeId::new(1), "10.0.0.1:7000".parse().unwrap());
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);

        map.insert(NodeId::new(2), "10.0.0.2:7000".parse().unwrap());
        assert_eq!(map.len(), 2);
    }

    #[tokio::test]
    async fn peer_connections_add_peer() {
        let peer_map = PeerMap::new(vec![]);
        let mut conns = PeerConnections::new(peer_map);

        // Initially unknown.
        let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
        assert!(matches!(
            conns.send_to(NodeId::new(1), msg).await,
            Err(TransportError::UnknownPeer(_))
        ));

        // After adding, the peer is known (send will fail to connect but
        // with ConnectionFailed, not UnknownPeer).
        let addr: SocketAddr = "10.0.0.99:7000".parse().unwrap();
        conns.add_peer(NodeId::new(1), addr);

        let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
        let result = conns.send_to(NodeId::new(1), msg).await;
        assert!(matches!(result, Err(TransportError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn peer_connections_remove_peer() {
        let addr: SocketAddr = "10.0.0.99:7000".parse().unwrap();
        let peer_map = PeerMap::new(vec![(NodeId::new(1), addr)]);
        let mut conns = PeerConnections::new(peer_map);

        // Peer is known before removal.
        let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
        let result = conns.send_to(NodeId::new(1), msg).await;
        assert!(matches!(result, Err(TransportError::ConnectionFailed(_))));

        // After removal, peer is unknown.
        conns.remove_peer(NodeId::new(1));
        let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
        let result = conns.send_to(NodeId::new(1), msg).await;
        assert!(matches!(result, Err(TransportError::UnknownPeer(_))));
    }

    #[tokio::test]
    async fn peer_updater_add_and_remove() {
        let peer_map = PeerMap::new(vec![]);
        let shared = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared), Arc::clone(&conns));

        let addr: SocketAddr = "10.0.0.5:7000".parse().unwrap();

        // Add a peer via the updater.
        updater.add_peer(NodeId::new(5), addr).await;

        // Verify shared map was updated.
        {
            let map = shared.read().await;
            assert_eq!(map.get(NodeId::new(5)), Some(&addr));
            assert_eq!(map.len(), 1);
        }

        // Verify peer connections were updated (peer is now known).
        {
            let mut c = conns.lock().await;
            let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
            let result = c.send_to(NodeId::new(5), msg).await;
            // ConnectionFailed means it's known but unreachable, not UnknownPeer.
            assert!(matches!(result, Err(TransportError::ConnectionFailed(_))));
        }

        // Remove the peer via the updater.
        updater.remove_peer(NodeId::new(5)).await;

        // Verify shared map was updated.
        {
            let map = shared.read().await;
            assert_eq!(map.get(NodeId::new(5)), None);
            assert!(map.is_empty());
        }

        // Verify peer connections were updated (peer is now unknown).
        {
            let mut c = conns.lock().await;
            let msg = pirc_protocol::Message::new(Command::Ping, vec!["test".into()]);
            let result = c.send_to(NodeId::new(5), msg).await;
            assert!(matches!(result, Err(TransportError::UnknownPeer(_))));
        }
    }

    #[tokio::test]
    async fn peer_updater_is_cloneable() {
        let peer_map = PeerMap::new(vec![]);
        let shared = Arc::new(RwLock::new(peer_map.clone()));
        let conns = Arc::new(Mutex::new(PeerConnections::new(peer_map)));
        let updater = PeerUpdater::new(Arc::clone(&shared), Arc::clone(&conns));

        // Clone and use from both handles.
        let updater2 = updater.clone();

        let addr1: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        let addr2: SocketAddr = "10.0.0.2:7000".parse().unwrap();

        updater.add_peer(NodeId::new(1), addr1).await;
        updater2.add_peer(NodeId::new(2), addr2).await;

        let map = shared.read().await;
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(NodeId::new(1)), Some(&addr1));
        assert_eq!(map.get(NodeId::new(2)), Some(&addr2));
    }
}
