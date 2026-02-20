//! Server-relay fallback integration tests.
//!
//! Exercises the fallback mechanism when P2P connections fail: mesh
//! degradation, relay-through-server messaging, E2E encryption during
//! relay, and transparent fallback.

use pirc_common::types::GroupId;
use pirc_crypto::group_key::GroupKeyManager;
use pirc_p2p::group_mesh::{GroupMesh, GroupMeshEvent, PeerConnectionState};
use pirc_p2p::group_session::{GroupSession, GroupSessionEvent, GroupSessionState};
use pirc_server::group_registry::GroupRegistry;

use super::{create_test_session_pair_unique, members, mock_transport};

// ── Fallback when P2P fails ──────────────────────────────────────────

#[tokio::test]
async fn member_degraded_transitions_to_relay_fallback() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_degraded("bob", "ICE connectivity check failed".into());

    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::RelayFallback)
    );
    assert!(mesh.is_degraded());
    assert!(!mesh.is_fully_connected());
}

#[tokio::test]
async fn mesh_degraded_event_emitted() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.drain_events();

    mesh.member_degraded("bob", "NAT traversal failed".into());

    let events = mesh.drain_events();
    assert!(events.iter().any(|e| matches!(
        e,
        GroupMeshEvent::MemberDegraded { member, reason }
            if member == "bob" && reason == "NAT traversal failed"
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, GroupMeshEvent::MeshDegraded)));
}

// ── Server-relayed messages still E2E encrypted ──────────────────────

#[test]
fn relay_messages_remain_encrypted() {
    let group_id = GroupId::new(1);

    let mut sender_mgr = GroupKeyManager::new(group_id);
    let mut relay_receiver_mgr = GroupKeyManager::new(group_id);

    let (s_to_r, r_from_s) = create_test_session_pair_unique(0x60);
    sender_mgr.add_member("relay-bob");
    sender_mgr.set_session("relay-bob", s_to_r);
    relay_receiver_mgr.add_member("sender");
    relay_receiver_mgr.set_session("sender", r_from_s);

    // Encrypt the message
    let plaintext = b"relayed but encrypted";
    let encrypted_map = sender_mgr.encrypt_for_group(plaintext).expect("encrypt");

    // The encrypted payload is what would be relayed through the server
    let relay_payload = encrypted_map["relay-bob"].to_bytes();

    // Plaintext must NOT appear in relay payload
    assert!(
        !relay_payload
            .windows(plaintext.len())
            .any(|w| w == plaintext),
        "plaintext must not be visible in relay payload"
    );

    // Receiver can decrypt the relayed message
    let decrypted = relay_receiver_mgr
        .decrypt_from_member(
            "sender",
            &pirc_crypto::message::EncryptedMessage::from_bytes(&relay_payload).expect("parse"),
        )
        .expect("decrypt");
    assert_eq!(decrypted, plaintext);
}

// ── Transparent fallback ─────────────────────────────────────────────

#[tokio::test]
async fn degraded_member_listed_for_relay() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("charlie".into(), mock_transport().await);
    mesh.member_degraded("bob", "timeout".into());

    let degraded = mesh.degraded_members();
    assert_eq!(degraded.len(), 1);
    assert!(degraded.contains(&"bob".to_owned()));

    // Connected members unaffected
    assert_eq!(mesh.connected_members().len(), 2);
    assert!(mesh.get_transport("alice").is_some());
    assert!(mesh.get_transport("charlie").is_some());
    assert!(mesh.get_transport("bob").is_none());
}

// ── Re-establish P2P after fallback ──────────────────────────────────

#[tokio::test]
async fn reconnect_after_relay_fallback() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_degraded("bob", "initial failure".into());
    mesh.drain_events();

    assert!(mesh.is_degraded());
    assert!(!mesh.is_fully_connected());

    // Bob reconnects via P2P
    mesh.member_connected("bob".into(), mock_transport().await);

    assert!(!mesh.is_degraded());
    assert!(mesh.is_fully_connected());
    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::Connected)
    );

    let events = mesh.drain_events();
    assert!(events.iter().any(
        |e| matches!(e, GroupMeshEvent::MemberConnected { member } if member == "bob")
    ));
    assert!(events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)));
}

#[tokio::test]
async fn members_needing_reconnect_includes_degraded() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_degraded("bob", "NAT failed".into());
    mesh.member_connected("charlie".into(), mock_transport().await);
    mesh.member_disconnected("charlie");

    let needing = mesh.members_needing_reconnect();
    assert!(needing.contains(&"bob".to_owned()));
    assert!(needing.contains(&"charlie".to_owned()));
    assert_eq!(needing.len(), 2);
}

// ── GroupSession fallback to relay ───────────────────────────────────

#[test]
fn session_relay_fallback_transitions_to_degraded() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
    session.begin_establishing();

    session.member_connected("alice");
    session.member_fallback_to_relay("bob", "ICE failed".into());

    assert_eq!(session.state(), GroupSessionState::Degraded);
    assert!(session.relay_members().contains("bob"));
    assert!(session.connected_members().contains("alice"));
}

#[test]
fn session_relay_member_reconnects_via_p2p() {
    let mut session = GroupSession::new("g1".into(), members(&["alice"]));
    session.begin_establishing();
    session.member_fallback_to_relay("alice", "timeout".into());
    assert_eq!(session.state(), GroupSessionState::Degraded);

    session.member_connected("alice");
    assert_eq!(session.state(), GroupSessionState::Active);
    assert!(!session.relay_members().contains("alice"));
    assert!(session.connected_members().contains("alice"));
}

#[test]
fn session_fallback_emits_event() {
    let mut session = GroupSession::new("g1".into(), members(&["alice"]));
    session.begin_establishing();
    session.drain_events();

    session.member_fallback_to_relay("alice", "NAT failure".into());

    let events = session.drain_events();
    assert!(events.iter().any(|e| matches!(
        e,
        GroupSessionEvent::MemberFallbackToRelay { member, reason }
            if member == "alice" && reason == "NAT failure"
    )));
}

// ── Mixed P2P and relay in same group ────────────────────────────────

#[tokio::test]
async fn mixed_p2p_and_relay_members() {
    let mut mesh = GroupMesh::new("group-1".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());
    mesh.add_member("dave".into());

    // alice and charlie connect via P2P
    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("charlie".into(), mock_transport().await);

    // bob degrades to relay
    mesh.member_degraded("bob", "symmetric NAT".into());

    // dave stays connecting (not yet resolved)
    assert_eq!(
        mesh.member_state("dave"),
        Some(PeerConnectionState::Connecting)
    );

    assert_eq!(mesh.connected_members().len(), 2);
    assert_eq!(mesh.degraded_members().len(), 1);
    assert!(mesh.is_degraded());
    assert!(!mesh.is_fully_connected());
}

// ── Server registry tracks groups for relay ──────────────────────────

#[test]
fn server_registry_group_membership_for_relay() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("relay-test".into(), "alice".into(), 1000);
    registry.add_member(group_id, "bob".into(), 1001);
    registry.add_member(group_id, "charlie".into(), 1002);

    // Server knows all members for relay routing
    assert!(registry.is_member(group_id, "alice"));
    assert!(registry.is_member(group_id, "bob"));
    assert!(registry.is_member(group_id, "charlie"));

    let mut members = registry.members(group_id);
    members.sort();
    assert_eq!(members, vec!["alice", "bob", "charlie"]);
}

#[test]
fn server_tracks_groups_per_user_for_relay() {
    let registry = GroupRegistry::new();
    let g1 = registry.create_group("group-1".into(), "alice".into(), 1000);
    let g2 = registry.create_group("group-2".into(), "alice".into(), 1001);
    registry.add_member(g1, "bob".into(), 1002);

    let alice_groups = registry.groups_for_member("alice");
    assert_eq!(alice_groups.len(), 2);
    assert!(alice_groups.contains(&g1));
    assert!(alice_groups.contains(&g2));

    let bob_groups = registry.groups_for_member("bob");
    assert_eq!(bob_groups.len(), 1);
    assert!(bob_groups.contains(&g1));
}
