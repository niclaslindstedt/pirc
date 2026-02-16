//! Group mesh topology manager for P2P group chat connections.
//!
//! Manages the full mesh of P2P connections for a group chat, tracking
//! per-member connection state and emitting [`GroupMeshEvent`] events as
//! members connect, disconnect, or degrade to relay fallback.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::encrypted_transport::EncryptedP2pTransport;

/// Connection state for a single group member's P2P link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerConnectionState {
    /// P2P connection is being established.
    Connecting,
    /// Direct or relayed P2P connection is active.
    Connected,
    /// P2P connection failed; member needs server relay fallback.
    RelayFallback,
    /// P2P connection was closed or never established.
    Disconnected,
}

/// Events emitted by the [`GroupMesh`] as member connectivity changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupMeshEvent {
    /// A member has established a P2P connection.
    MemberConnected {
        /// Nickname of the connected member.
        member: String,
    },
    /// A member has disconnected from P2P.
    MemberDisconnected {
        /// Nickname of the disconnected member.
        member: String,
    },
    /// A member's P2P connection failed; they need relay fallback.
    MemberDegraded {
        /// Nickname of the degraded member.
        member: String,
        /// Reason for degradation.
        reason: String,
    },
    /// All members are connected via P2P.
    MeshReady,
    /// Some P2P connections have failed (mesh is partially degraded).
    MeshDegraded,
}

/// Manages a P2P transport for a single group member.
struct MemberTransport {
    /// The encrypted P2P transport for this member.
    transport: EncryptedP2pTransport,
    /// Current connection state.
    state: PeerConnectionState,
}

/// Manages the full mesh of P2P connections for a group chat.
///
/// Each group member has a tracked [`PeerConnectionState`] and, when
/// connected, an associated [`EncryptedP2pTransport`]. The mesh emits
/// [`GroupMeshEvent`] events for connectivity changes and provides
/// queries for connected and degraded members.
pub struct GroupMesh {
    /// The group identifier (opaque string for flexibility).
    group_id: String,
    /// Per-member transport and state tracking.
    members: HashMap<String, MemberTransport>,
    /// Members that are tracked but have no transport yet (connecting or
    /// disconnected without a transport).
    tracked_members: HashMap<String, PeerConnectionState>,
    /// Pending outbound events.
    events: Vec<GroupMeshEvent>,
}

impl GroupMesh {
    /// Creates a new empty `GroupMesh` for the given group.
    #[must_use]
    pub fn new(group_id: String) -> Self {
        Self {
            group_id,
            members: HashMap::new(),
            tracked_members: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Returns the group identifier.
    #[must_use]
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Drains all pending outbound events.
    pub fn drain_events(&mut self) -> Vec<GroupMeshEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns the connection state for a member, or `None` if the member
    /// is not tracked.
    #[must_use]
    pub fn member_state(&self, member: &str) -> Option<PeerConnectionState> {
        if let Some(mt) = self.members.get(member) {
            Some(mt.state)
        } else {
            self.tracked_members.get(member).copied()
        }
    }

    /// Adds a member to the mesh and begins tracking them as `Connecting`.
    ///
    /// If the member is already tracked, this is a no-op.
    pub fn add_member(&mut self, member: String) {
        if self.members.contains_key(&member) || self.tracked_members.contains_key(&member) {
            debug!(group = %self.group_id, member = %member, "member already tracked");
            return;
        }
        info!(group = %self.group_id, member = %member, "adding member to mesh");
        self.tracked_members
            .insert(member, PeerConnectionState::Connecting);
    }

    /// Removes a member from the mesh, tearing down their transport.
    ///
    /// Emits a `MemberDisconnected` event if the member was connected.
    pub fn remove_member(&mut self, member: &str) {
        let was_connected = self
            .members
            .get(member)
            .is_some_and(|mt| mt.state == PeerConnectionState::Connected);

        self.members.remove(member);
        self.tracked_members.remove(member);

        if was_connected {
            info!(group = %self.group_id, member = %member, "removing connected member");
            self.events.push(GroupMeshEvent::MemberDisconnected {
                member: member.to_owned(),
            });
            self.check_mesh_state();
        } else {
            debug!(group = %self.group_id, member = %member, "removing member from mesh");
        }
    }

    /// Records that a P2P transport has been established for a member.
    ///
    /// Transitions the member to `Connected` and emits a `MemberConnected`
    /// event. May also emit `MeshReady` if all members are now connected.
    pub fn member_connected(&mut self, member: String, transport: EncryptedP2pTransport) {
        if !self.members.contains_key(&member) && !self.tracked_members.contains_key(&member) {
            warn!(group = %self.group_id, member = %member, "ignoring connected event for unknown member");
            return;
        }
        info!(group = %self.group_id, member = %member, "member P2P connected");
        self.tracked_members.remove(&member);
        self.members.insert(
            member.clone(),
            MemberTransport {
                transport,
                state: PeerConnectionState::Connected,
            },
        );
        self.events.push(GroupMeshEvent::MemberConnected {
            member,
        });
        self.check_mesh_state();
    }

    /// Records that a member's P2P connection has failed.
    ///
    /// Transitions the member to `RelayFallback` and emits a
    /// `MemberDegraded` event. May also emit `MeshDegraded`.
    pub fn member_degraded(&mut self, member: &str, reason: String) {
        if !self.members.contains_key(member) && !self.tracked_members.contains_key(member) {
            warn!(group = %self.group_id, member = %member, "ignoring degraded event for unknown member");
            return;
        }
        warn!(
            group = %self.group_id,
            member = %member,
            reason = %reason,
            "member P2P connection degraded"
        );

        // Remove any existing transport for this member.
        self.members.remove(member);
        self.tracked_members
            .insert(member.to_owned(), PeerConnectionState::RelayFallback);

        self.events.push(GroupMeshEvent::MemberDegraded {
            member: member.to_owned(),
            reason,
        });
        self.check_mesh_state();
    }

    /// Records that a member has disconnected.
    ///
    /// Transitions the member to `Disconnected` and emits a
    /// `MemberDisconnected` event.
    pub fn member_disconnected(&mut self, member: &str) {
        let was_connected = self.members.contains_key(member);
        let was_tracked = self.tracked_members.contains_key(member);

        if !was_connected && !was_tracked {
            warn!(group = %self.group_id, member = %member, "ignoring disconnected event for unknown member");
            return;
        }

        self.members.remove(member);
        self.tracked_members
            .insert(member.to_owned(), PeerConnectionState::Disconnected);

        if was_connected {
            info!(group = %self.group_id, member = %member, "member disconnected");
            self.events.push(GroupMeshEvent::MemberDisconnected {
                member: member.to_owned(),
            });
            self.check_mesh_state();
        }
    }

    /// Returns a reference to the encrypted transport for a connected member.
    #[must_use]
    pub fn get_transport(&self, member: &str) -> Option<&EncryptedP2pTransport> {
        self.members.get(member).and_then(|mt| {
            if mt.state == PeerConnectionState::Connected {
                Some(&mt.transport)
            } else {
                None
            }
        })
    }

    /// Returns the nicknames of all members with active P2P connections.
    #[must_use]
    pub fn connected_members(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|(_, mt)| mt.state == PeerConnectionState::Connected)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Returns the nicknames of members whose P2P connections failed
    /// (relay fallback needed).
    #[must_use]
    pub fn degraded_members(&self) -> Vec<String> {
        self.tracked_members
            .iter()
            .filter(|(_, state)| **state == PeerConnectionState::RelayFallback)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Returns the nicknames of members in `RelayFallback` or `Disconnected`
    /// state that could benefit from a P2P reconnection attempt.
    #[must_use]
    pub fn members_needing_reconnect(&self) -> Vec<String> {
        self.tracked_members
            .iter()
            .filter(|(_, state)| {
                matches!(
                    state,
                    PeerConnectionState::RelayFallback | PeerConnectionState::Disconnected
                )
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Returns the total number of tracked members (all states).
    #[must_use]
    pub fn member_count(&self) -> usize {
        self.members.len() + self.tracked_members.len()
    }

    /// Returns `true` if all tracked members are connected via P2P.
    #[must_use]
    pub fn is_fully_connected(&self) -> bool {
        if self.members.is_empty() && self.tracked_members.is_empty() {
            return true;
        }
        self.tracked_members.is_empty()
            && self
                .members
                .values()
                .all(|mt| mt.state == PeerConnectionState::Connected)
    }

    /// Returns `true` if any member has degraded to relay fallback.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.tracked_members
            .values()
            .any(|s| *s == PeerConnectionState::RelayFallback)
    }

    /// Checks the overall mesh state and emits `MeshReady` or `MeshDegraded`
    /// events as appropriate.
    fn check_mesh_state(&mut self) {
        if self.member_count() == 0 {
            return;
        }

        if self.is_fully_connected() {
            self.events.push(GroupMeshEvent::MeshReady);
        } else if self.is_degraded() {
            self.events.push(GroupMeshEvent::MeshDegraded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encrypted_transport::TransportCipher;
    use crate::transport::{P2pTransport, UdpTransport};
    use std::sync::Arc;
    use tokio::net::UdpSocket;

    /// A no-op cipher for testing purposes (data passes through unchanged).
    struct NoopCipher;

    impl TransportCipher for NoopCipher {
        fn encrypt(&mut self, plaintext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Ok(plaintext.to_vec())
        }

        fn decrypt(&mut self, ciphertext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Ok(ciphertext.to_vec())
        }
    }

    /// Creates a mock `EncryptedP2pTransport` for testing.
    async fn mock_transport() -> EncryptedP2pTransport {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = sock.local_addr().unwrap();
        sock.connect(addr).await.unwrap();
        let udp = UdpTransport::new(Arc::new(sock));
        EncryptedP2pTransport::new(P2pTransport::Direct(udp), Box::new(NoopCipher))
    }

    // --- Construction ---

    #[test]
    fn new_mesh_is_empty() {
        let mesh = GroupMesh::new("group1".into());
        assert_eq!(mesh.group_id(), "group1");
        assert_eq!(mesh.member_count(), 0);
        assert!(mesh.connected_members().is_empty());
        assert!(mesh.degraded_members().is_empty());
        assert!(mesh.is_fully_connected());
        assert!(!mesh.is_degraded());
    }

    // --- Add / Remove members ---

    #[test]
    fn add_member_tracks_as_connecting() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        assert_eq!(mesh.member_count(), 1);
        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::Connecting)
        );
        assert!(mesh.connected_members().is_empty());
    }

    #[test]
    fn add_duplicate_member_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("alice".into());
        assert_eq!(mesh.member_count(), 1);
    }

    #[tokio::test]
    async fn remove_connected_member_emits_disconnected() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.drain_events();

        mesh.remove_member("alice");
        assert_eq!(mesh.member_count(), 0);
        assert!(mesh.member_state("alice").is_none());

        let events = mesh.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, GroupMeshEvent::MemberDisconnected { member } if member == "alice")
        ));
    }

    #[test]
    fn remove_connecting_member_no_disconnect_event() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.drain_events();

        mesh.remove_member("alice");
        let events = mesh.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn remove_unknown_member_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.remove_member("ghost");
        assert!(mesh.drain_events().is_empty());
    }

    // --- Connection lifecycle ---

    #[tokio::test]
    async fn member_connected_transitions_and_emits_event() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());

        mesh.member_connected("alice".into(), mock_transport().await);

        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::Connected)
        );
        let connected = mesh.connected_members();
        assert_eq!(connected.len(), 1);
        assert!(connected.contains(&"alice".to_owned()));

        let events = mesh.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, GroupMeshEvent::MemberConnected { member } if member == "alice")
        ));
    }

    #[tokio::test]
    async fn member_connected_emits_mesh_ready_when_all_connected() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.drain_events();

        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_connected("bob".into(), mock_transport().await);

        let events = mesh.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupMeshEvent::MeshReady)));
    }

    #[tokio::test]
    async fn partial_connections_no_mesh_ready() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.drain_events();

        mesh.member_connected("alice".into(), mock_transport().await);

        let events = mesh.drain_events();
        assert!(!events
            .iter()
            .any(|e| matches!(e, GroupMeshEvent::MeshReady)));
    }

    #[tokio::test]
    async fn member_degraded_transitions_and_emits_event() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.drain_events();

        mesh.member_degraded("alice", "ICE timeout".into());

        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::RelayFallback)
        );
        let degraded = mesh.degraded_members();
        assert_eq!(degraded.len(), 1);
        assert!(degraded.contains(&"alice".to_owned()));
        assert!(mesh.is_degraded());

        let events = mesh.drain_events();
        assert!(events.iter().any(|e| matches!(
            e,
            GroupMeshEvent::MemberDegraded { member, .. } if member == "alice"
        )));
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupMeshEvent::MeshDegraded)));
    }

    #[tokio::test]
    async fn member_disconnected_from_connected() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.drain_events();

        mesh.member_disconnected("alice");

        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::Disconnected)
        );
        assert!(mesh.connected_members().is_empty());

        let events = mesh.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, GroupMeshEvent::MemberDisconnected { member } if member == "alice")
        ));
    }

    #[test]
    fn member_disconnected_unknown_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.member_disconnected("ghost");
        assert!(mesh.drain_events().is_empty());
        assert_eq!(mesh.member_count(), 0);
    }

    #[tokio::test]
    async fn member_connected_unknown_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.member_connected("ghost".into(), mock_transport().await);
        assert!(mesh.drain_events().is_empty());
        assert_eq!(mesh.member_count(), 0);
    }

    #[test]
    fn member_degraded_unknown_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.member_degraded("ghost", "ICE timeout".into());
        assert!(mesh.drain_events().is_empty());
        assert_eq!(mesh.member_count(), 0);
    }

    // --- Transport access ---

    #[tokio::test]
    async fn get_transport_returns_some_for_connected() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);

        assert!(mesh.get_transport("alice").is_some());
    }

    #[test]
    fn get_transport_returns_none_for_connecting() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        assert!(mesh.get_transport("alice").is_none());
    }

    #[test]
    fn get_transport_returns_none_for_unknown() {
        let mesh = GroupMesh::new("g1".into());
        assert!(mesh.get_transport("ghost").is_none());
    }

    #[tokio::test]
    async fn get_transport_returns_none_after_disconnect() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_disconnected("alice");

        assert!(mesh.get_transport("alice").is_none());
    }

    // --- Mesh state queries ---

    #[tokio::test]
    async fn is_fully_connected_with_all_connected() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_connected("bob".into(), mock_transport().await);

        assert!(mesh.is_fully_connected());
        assert!(!mesh.is_degraded());
    }

    #[tokio::test]
    async fn is_degraded_with_relay_fallback() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_degraded("bob", "timeout".into());

        assert!(!mesh.is_fully_connected());
        assert!(mesh.is_degraded());
    }

    // --- Event lifecycle ---

    #[test]
    fn drain_events_clears_events() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_degraded("alice", "fail".into());

        assert!(!mesh.drain_events().is_empty());
        assert!(mesh.drain_events().is_empty());
    }

    // --- Complex scenarios ---

    #[tokio::test]
    async fn reconnect_after_degradation() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_degraded("alice", "timeout".into());
        mesh.drain_events();

        // Alice reconnects via P2P
        mesh.member_connected("alice".into(), mock_transport().await);

        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::Connected)
        );
        assert!(!mesh.is_degraded());
        assert!(mesh.is_fully_connected());

        let events = mesh.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, GroupMeshEvent::MemberConnected { member } if member == "alice")
        ));
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupMeshEvent::MeshReady)));
    }

    #[tokio::test]
    async fn mixed_connected_and_degraded_members() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.add_member("charlie".into());

        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_degraded("bob", "NAT traversal failed".into());
        mesh.member_connected("charlie".into(), mock_transport().await);

        let connected = mesh.connected_members();
        assert_eq!(connected.len(), 2);
        assert!(connected.contains(&"alice".to_owned()));
        assert!(connected.contains(&"charlie".to_owned()));

        let degraded = mesh.degraded_members();
        assert_eq!(degraded.len(), 1);
        assert!(degraded.contains(&"bob".to_owned()));

        assert!(!mesh.is_fully_connected());
        assert!(mesh.is_degraded());
    }

    #[tokio::test]
    async fn members_needing_reconnect_includes_both_states() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.add_member("charlie".into());
        mesh.add_member("dave".into());

        // alice: connected (should NOT appear)
        mesh.member_connected("alice".into(), mock_transport().await);
        // bob: degraded to relay (should appear)
        mesh.member_degraded("bob", "NAT failed".into());
        // charlie: disconnected (should appear)
        mesh.member_connected("charlie".into(), mock_transport().await);
        mesh.member_disconnected("charlie");
        // dave: still connecting (should NOT appear)

        let needing = mesh.members_needing_reconnect();
        assert_eq!(needing.len(), 2);
        assert!(needing.contains(&"bob".to_owned()));
        assert!(needing.contains(&"charlie".to_owned()));
    }

    #[test]
    fn members_needing_reconnect_empty_mesh() {
        let mesh = GroupMesh::new("g1".into());
        assert!(mesh.members_needing_reconnect().is_empty());
    }

    #[tokio::test]
    async fn remove_degraded_member_clears_degradation() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.add_member("bob".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.member_degraded("bob", "fail".into());
        mesh.drain_events();

        mesh.remove_member("bob");

        assert!(mesh.is_fully_connected());
        assert!(!mesh.is_degraded());
    }

    #[tokio::test]
    async fn add_member_after_mesh_ready_breaks_readiness() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        assert!(mesh.is_fully_connected());
        mesh.drain_events();

        mesh.add_member("bob".into());
        assert!(!mesh.is_fully_connected());
    }

    #[tokio::test]
    async fn duplicate_add_after_connected_is_noop() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        mesh.drain_events();

        mesh.add_member("alice".into());
        assert_eq!(mesh.member_count(), 1);
        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::Connected)
        );
    }

    #[tokio::test]
    async fn member_degraded_removes_existing_transport() {
        let mut mesh = GroupMesh::new("g1".into());
        mesh.add_member("alice".into());
        mesh.member_connected("alice".into(), mock_transport().await);
        assert!(mesh.get_transport("alice").is_some());

        mesh.member_degraded("alice", "connection lost".into());
        assert!(mesh.get_transport("alice").is_none());
        assert_eq!(
            mesh.member_state("alice"),
            Some(PeerConnectionState::RelayFallback)
        );
    }

    // --- Event variant tests ---

    #[test]
    fn event_debug_formatting() {
        let event = GroupMeshEvent::MemberConnected {
            member: "alice".into(),
        };
        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("MemberConnected"));
        assert!(debug_str.contains("alice"));

        let event = GroupMeshEvent::MeshReady;
        assert!(format!("{event:?}").contains("MeshReady"));

        let event = GroupMeshEvent::MeshDegraded;
        assert!(format!("{event:?}").contains("MeshDegraded"));
    }

    #[test]
    fn event_equality() {
        let a = GroupMeshEvent::MemberConnected {
            member: "alice".into(),
        };
        let b = GroupMeshEvent::MemberConnected {
            member: "alice".into(),
        };
        assert_eq!(a, b);

        let c = GroupMeshEvent::MemberConnected {
            member: "bob".into(),
        };
        assert_ne!(a, c);

        assert_eq!(GroupMeshEvent::MeshReady, GroupMeshEvent::MeshReady);
        assert_ne!(GroupMeshEvent::MeshReady, GroupMeshEvent::MeshDegraded);
    }

    #[test]
    fn event_clone() {
        let event = GroupMeshEvent::MemberDegraded {
            member: "alice".into(),
            reason: "timeout".into(),
        };
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn peer_connection_state_equality() {
        assert_eq!(PeerConnectionState::Connecting, PeerConnectionState::Connecting);
        assert_eq!(PeerConnectionState::Connected, PeerConnectionState::Connected);
        assert_eq!(
            PeerConnectionState::RelayFallback,
            PeerConnectionState::RelayFallback
        );
        assert_eq!(
            PeerConnectionState::Disconnected,
            PeerConnectionState::Disconnected
        );
        assert_ne!(PeerConnectionState::Connected, PeerConnectionState::Disconnected);
    }
}
