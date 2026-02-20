//! Group mesh formation integration tests.
//!
//! Exercises mesh topology tracking, peer connection events, and
//! full-mesh establishment for group chats.

use pirc_p2p::group_mesh::{GroupMesh, GroupMeshEvent, PeerConnectionState};
use pirc_p2p::group_session::{GroupSession, GroupSessionEvent, GroupSessionState};

use super::{members, mock_transport};

// ── 3-peer mesh establishment ────────────────────────────────────────

#[tokio::test]
async fn three_peers_establish_full_mesh() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    assert_eq!(mesh.member_count(), 3);
    assert!(!mesh.is_fully_connected());

    // Each peer connects in turn
    mesh.member_connected("alice".into(), mock_transport().await);
    assert!(!mesh.is_fully_connected());

    mesh.member_connected("bob".into(), mock_transport().await);
    assert!(!mesh.is_fully_connected());

    mesh.member_connected("charlie".into(), mock_transport().await);
    assert!(mesh.is_fully_connected());
}

#[tokio::test]
async fn mesh_tracks_all_peer_connections() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("bob".into(), mock_transport().await);
    mesh.member_connected("charlie".into(), mock_transport().await);

    let connected = mesh.connected_members();
    assert_eq!(connected.len(), 3);
    assert!(connected.contains(&"alice".to_owned()));
    assert!(connected.contains(&"bob".to_owned()));
    assert!(connected.contains(&"charlie".to_owned()));
}

#[tokio::test]
async fn peer_connected_events_emitted_for_each_member() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("bob".into(), mock_transport().await);
    mesh.member_connected("charlie".into(), mock_transport().await);

    let events = mesh.drain_events();

    // Should have MemberConnected for each peer
    let connected_members: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            GroupMeshEvent::MemberConnected { member } => Some(member.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(connected_members.len(), 3);
    assert!(connected_members.contains(&"alice"));
    assert!(connected_members.contains(&"bob"));
    assert!(connected_members.contains(&"charlie"));
}

#[tokio::test]
async fn mesh_ready_emitted_when_all_connected() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("bob".into(), mock_transport().await);

    let events = mesh.drain_events();
    assert!(
        events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)),
        "MeshReady should be emitted when all members connect"
    );
}

#[tokio::test]
async fn mesh_topology_verified_full_mesh() {
    let mut mesh = GroupMesh::new("group-1".into());
    let peers = vec!["alice", "bob", "charlie"];

    for peer in &peers {
        mesh.add_member((*peer).to_owned());
    }

    // Connect all peers — simulating a full mesh
    for peer in &peers {
        mesh.member_connected((*peer).to_owned(), mock_transport().await);
    }

    // Verify full mesh: each peer is connected and has a transport
    for peer in &peers {
        assert_eq!(
            mesh.member_state(peer),
            Some(PeerConnectionState::Connected),
            "{peer} should be Connected"
        );
        assert!(
            mesh.get_transport(peer).is_some(),
            "{peer} should have a transport"
        );
    }

    assert!(mesh.is_fully_connected());
    assert!(!mesh.is_degraded());
    assert!(mesh.degraded_members().is_empty());
    assert!(mesh.members_needing_reconnect().is_empty());
}

// ── Members start as Connecting ──────────────────────────────────────

#[test]
fn added_members_start_as_connecting() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());

    assert_eq!(
        mesh.member_state("alice"),
        Some(PeerConnectionState::Connecting)
    );
    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::Connecting)
    );
}

// ── GroupSession state machine ───────────────────────────────────────

#[test]
fn group_session_three_peers_transitions_to_active() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob", "charlie"]));
    assert_eq!(session.state(), GroupSessionState::Creating);

    session.begin_establishing();
    assert_eq!(session.state(), GroupSessionState::Establishing);

    session.member_connected("alice");
    assert_eq!(session.state(), GroupSessionState::Establishing);

    session.member_connected("bob");
    assert_eq!(session.state(), GroupSessionState::Establishing);

    session.member_connected("charlie");
    assert_eq!(session.state(), GroupSessionState::Active);
}

#[test]
fn group_session_emits_state_changed_events() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
    session.begin_establishing();
    session.drain_events();

    session.member_connected("alice");
    session.member_connected("bob");

    let events = session.drain_events();
    assert!(events.iter().any(|e| matches!(
        e,
        GroupSessionEvent::StateChanged {
            new_state: GroupSessionState::Active
        }
    )));
}

#[test]
fn group_session_tracks_connected_members() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob", "charlie"]));
    session.begin_establishing();

    session.member_connected("alice");
    session.member_connected("charlie");

    assert!(session.connected_members().contains("alice"));
    assert!(session.connected_members().contains("charlie"));
    assert!(!session.connected_members().contains("bob"));
}

// ── Partial mesh ─────────────────────────────────────────────────────

#[tokio::test]
async fn partial_connections_mesh_not_ready() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    // Only alice connected

    assert!(!mesh.is_fully_connected());
    let events = mesh.drain_events();
    assert!(
        !events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)),
        "MeshReady should NOT be emitted with partial connections"
    );
}

// ── Session ID tracking ──────────────────────────────────────────────

#[test]
fn group_session_per_member_session_ids() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
    session.set_member_session("alice", "sess-001".into());
    session.set_member_session("bob", "sess-002".into());

    assert_eq!(session.get_member_session("alice"), Some("sess-001"));
    assert_eq!(session.get_member_session("bob"), Some("sess-002"));
    assert_eq!(session.get_member_session("charlie"), None);
}

// ── Large group mesh ─────────────────────────────────────────────────

#[tokio::test]
async fn five_peer_mesh_formation() {
    let mut mesh = GroupMesh::new("big-group".into());
    let peers: Vec<String> = (0..5).map(|i| format!("peer-{i}")).collect();

    for peer in &peers {
        mesh.add_member(peer.clone());
    }
    assert_eq!(mesh.member_count(), 5);

    for peer in &peers {
        mesh.member_connected(peer.clone(), mock_transport().await);
    }

    assert!(mesh.is_fully_connected());
    assert_eq!(mesh.connected_members().len(), 5);

    let events = mesh.drain_events();
    assert!(events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)));
}
