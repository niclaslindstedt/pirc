//! Peer join/leave integration tests.
//!
//! Exercises new peer joining an existing group, graceful and ungraceful
//! leave, and group continuity after departure.

use pirc_common::types::GroupId;
use pirc_crypto::group_key::{GroupEncryptionState, GroupKeyManager};
use pirc_p2p::group_mesh::{GroupMeshEvent, PeerConnectionState};
use pirc_p2p::group_session::{GroupSession, GroupSessionEvent, GroupSessionState};
use pirc_server::group_registry::GroupRegistry;

use super::{create_test_session_pair_unique, members, mock_transport, setup_three_peer_mesh};

// ── New peer joins existing group ────────────────────────────────────

#[tokio::test]
async fn new_peer_joins_existing_mesh() {
    let mut mesh = setup_three_peer_mesh().await;

    // Dave joins the group
    mesh.add_member("dave".into());

    assert_eq!(mesh.member_count(), 4);
    assert!(!mesh.is_fully_connected(), "mesh should not be fully connected until dave connects");
    assert_eq!(
        mesh.member_state("dave"),
        Some(PeerConnectionState::Connecting)
    );

    // Dave establishes P2P connections
    mesh.member_connected("dave".into(), mock_transport().await);

    assert!(mesh.is_fully_connected());
    assert_eq!(mesh.member_count(), 4);

    let events = mesh.drain_events();
    assert!(events.iter().any(
        |e| matches!(e, GroupMeshEvent::MemberConnected { member } if member == "dave")
    ));
    assert!(events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)));
}

// ── Graceful leave ───────────────────────────────────────────────────

#[tokio::test]
async fn peer_leaves_gracefully_emits_disconnected() {
    let mut mesh = setup_three_peer_mesh().await;

    mesh.remove_member("charlie");

    assert_eq!(mesh.member_count(), 2);
    assert!(mesh.member_state("charlie").is_none());

    let events = mesh.drain_events();
    assert!(events.iter().any(
        |e| matches!(e, GroupMeshEvent::MemberDisconnected { member } if member == "charlie")
    ));
}

#[tokio::test]
async fn group_continues_after_graceful_departure() {
    let mut mesh = setup_three_peer_mesh().await;

    mesh.remove_member("charlie");

    // Remaining members still connected
    assert_eq!(
        mesh.member_state("alice"),
        Some(PeerConnectionState::Connected)
    );
    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::Connected)
    );
    assert!(mesh.is_fully_connected());
}

// ── Ungraceful leave (crash) ─────────────────────────────────────────

#[tokio::test]
async fn peer_crashes_detected_via_disconnect() {
    let mut mesh = setup_three_peer_mesh().await;

    // Simulate crash: member_disconnected (not remove_member)
    mesh.member_disconnected("bob");

    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::Disconnected)
    );
    assert!(!mesh.is_fully_connected());

    let events = mesh.drain_events();
    assert!(events.iter().any(
        |e| matches!(e, GroupMeshEvent::MemberDisconnected { member } if member == "bob")
    ));
}

#[tokio::test]
async fn group_continues_after_crash() {
    let mut mesh = setup_three_peer_mesh().await;

    mesh.member_disconnected("bob");

    // Alice and Charlie still connected
    assert_eq!(
        mesh.member_state("alice"),
        Some(PeerConnectionState::Connected)
    );
    assert_eq!(
        mesh.member_state("charlie"),
        Some(PeerConnectionState::Connected)
    );
    assert_eq!(mesh.connected_members().len(), 2);

    // Bob shows as needing reconnect
    let needing_reconnect = mesh.members_needing_reconnect();
    assert!(needing_reconnect.contains(&"bob".to_owned()));
}

// ── GroupSession join/leave ──────────────────────────────────────────

#[test]
fn session_new_member_breaks_active_state() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
    session.begin_establishing();
    session.member_connected("alice");
    session.member_connected("bob");
    assert_eq!(session.state(), GroupSessionState::Active);

    // New member joins
    session.add_expected_member("charlie".into());
    assert_eq!(session.state(), GroupSessionState::Establishing);
}

#[test]
fn session_new_member_then_connects_returns_to_active() {
    let mut session = GroupSession::new("g1".into(), members(&["alice"]));
    session.begin_establishing();
    session.member_connected("alice");
    assert_eq!(session.state(), GroupSessionState::Active);

    session.add_expected_member("bob".into());
    session.member_connected("bob");
    assert_eq!(session.state(), GroupSessionState::Active);
}

#[test]
fn session_member_removed_recalculates_state() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob", "charlie"]));
    session.begin_establishing();
    session.member_connected("alice");
    assert_eq!(session.state(), GroupSessionState::Establishing);

    // Remove unconnected members — now only alice is expected
    session.remove_member("bob");
    session.remove_member("charlie");
    assert_eq!(session.state(), GroupSessionState::Active);
}

#[test]
fn session_member_disconnect_emits_event() {
    let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
    session.begin_establishing();
    session.member_connected("alice");
    session.member_connected("bob");
    session.drain_events();

    session.member_disconnected("bob");

    let events = session.drain_events();
    assert!(events.iter().any(
        |e| matches!(e, GroupSessionEvent::MemberDisconnected { member } if member == "bob")
    ));
}

// ── Server-side group registry ───────────────────────────────────────

#[test]
fn registry_member_joins_group() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("test-group".into(), "alice".into(), 1000);

    assert!(registry.add_member(group_id, "bob".into(), 1001));
    assert!(registry.add_member(group_id, "charlie".into(), 1002));

    let mut members = registry.members(group_id);
    members.sort();
    assert_eq!(members, vec!["alice", "bob", "charlie"]);
}

#[test]
fn registry_member_leaves_group() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("test-group".into(), "alice".into(), 1000);
    registry.add_member(group_id, "bob".into(), 1001);
    registry.add_member(group_id, "charlie".into(), 1002);

    let result = registry.remove_member(group_id, "charlie").unwrap();
    assert!(!result.group_destroyed);
    assert!(!registry.is_member(group_id, "charlie"));

    let mut remaining = registry.members(group_id);
    remaining.sort();
    assert_eq!(remaining, vec!["alice", "bob"]);
}

#[test]
fn registry_admin_leave_transfers_admin() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("test-group".into(), "alice".into(), 1000);
    registry.add_member(group_id, "bob".into(), 1001);

    let result = registry.remove_member(group_id, "alice").unwrap();
    assert_eq!(result.new_admin, Some("bob".into()));
    assert!(registry.is_admin(group_id, "bob"));
}

#[test]
fn registry_last_member_leaves_destroys_group() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("test-group".into(), "alice".into(), 1000);

    let result = registry.remove_member(group_id, "alice").unwrap();
    assert!(result.group_destroyed);
    assert!(!registry.exists(group_id));
}

// ── Encryption state after join/leave ────────────────────────────────

#[test]
fn new_member_starts_with_pending_encryption() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    key_mgr.add_member("alice");
    assert_eq!(
        key_mgr.member_state("alice"),
        Some(GroupEncryptionState::Pending)
    );
}

#[test]
fn removed_member_encryption_state_cleared() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    let (session, _) = create_test_session_pair_unique(0x50);
    key_mgr.add_member("alice");
    key_mgr.set_session("alice", session);
    assert!(key_mgr.has_session("alice"));

    key_mgr.remove_member("alice");
    assert!(key_mgr.member_state("alice").is_none());
    assert!(!key_mgr.has_session("alice"));
}

#[test]
fn remaining_members_can_still_encrypt_after_departure() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    let (s_alice, _) = create_test_session_pair_unique(0x10);
    let (s_bob, _) = create_test_session_pair_unique(0x20);
    let (s_charlie, _) = create_test_session_pair_unique(0x30);

    key_mgr.add_member("alice");
    key_mgr.set_session("alice", s_alice);
    key_mgr.add_member("bob");
    key_mgr.set_session("bob", s_bob);
    key_mgr.add_member("charlie");
    key_mgr.set_session("charlie", s_charlie);

    assert!(key_mgr.all_ready());

    // Charlie leaves
    key_mgr.remove_member("charlie");

    // Remaining members can still encrypt
    assert!(key_mgr.all_ready());
    let encrypted = key_mgr.encrypt_for_group(b"after departure").expect("encrypt");
    assert_eq!(encrypted.len(), 2);
    assert!(encrypted.contains_key("alice"));
    assert!(encrypted.contains_key("bob"));
}
