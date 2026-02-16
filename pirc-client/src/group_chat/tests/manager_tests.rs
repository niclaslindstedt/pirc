//! Tests for `GroupChatManager` construction, member management,
//! mesh state, encryption readiness, queries, join/leave, and
//! group member listing.

use super::helpers::{create_test_session_pair, mock_transport};
use super::super::*;
use pirc_crypto::group_key::GroupEncryptionState;
use pirc_p2p::group_mesh::PeerConnectionState;

// ── GroupChatManager construction ───────────────────────────────

#[test]
fn new_manager_is_empty() {
    let mgr = GroupChatManager::new();
    assert!(!mgr.has_group(GroupId::new(1)));
    assert!(mgr.group_ids().is_empty());
}

#[test]
fn default_creates_empty_manager() {
    let mgr = GroupChatManager::default();
    assert!(mgr.group_ids().is_empty());
}

#[test]
fn add_group_and_check() {
    let mut mgr = GroupChatManager::new();
    mgr.add_group(GroupId::new(1));
    assert!(mgr.has_group(GroupId::new(1)));
    assert!(!mgr.has_group(GroupId::new(2)));
}

#[test]
fn add_group_idempotent() {
    let mut mgr = GroupChatManager::new();
    mgr.add_group(GroupId::new(1));
    mgr.add_group(GroupId::new(1));
    assert_eq!(mgr.group_ids().len(), 1);
}

#[test]
fn remove_group() {
    let mut mgr = GroupChatManager::new();
    mgr.add_group(GroupId::new(1));
    mgr.remove_group(GroupId::new(1));
    assert!(!mgr.has_group(GroupId::new(1)));
}

// ── Member management ───────────────────────────────────────────

#[test]
fn add_member_to_group() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");

    assert_eq!(
        mgr.member_encryption_state(gid, "alice"),
        Some(GroupEncryptionState::Pending)
    );
    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::Connecting)
    );
}

#[test]
fn add_member_to_nonexistent_group() {
    let mut mgr = GroupChatManager::new();
    mgr.add_member(GroupId::new(999), "alice");
    // should not panic, just no-op
    assert!(mgr
        .member_encryption_state(GroupId::new(999), "alice")
        .is_none());
}

#[test]
fn remove_member_cleans_up() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.remove_member(gid, "alice");
    assert!(mgr.member_encryption_state(gid, "alice").is_none());
}

#[test]
fn set_member_establishing() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.set_member_establishing(gid, "alice");
    assert_eq!(
        mgr.member_encryption_state(gid, "alice"),
        Some(GroupEncryptionState::Establishing)
    );
}

#[test]
fn set_member_session_ready() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    let (sender, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", sender);
    assert_eq!(
        mgr.member_encryption_state(gid, "alice"),
        Some(GroupEncryptionState::Ready)
    );
}

// ── Mesh state management ───────────────────────────────────────

#[tokio::test]
async fn member_connected_updates_state() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.member_connected(gid, "alice", mock_transport().await);

    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::Connected)
    );
    assert!(mgr.connected_members(gid).contains(&"alice".to_owned()));
}

#[test]
fn member_degraded_updates_state() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.member_degraded(gid, "alice", "ICE timeout".into());

    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::RelayFallback)
    );
    assert!(mgr.degraded_members(gid).contains(&"alice".to_owned()));
}

#[tokio::test]
async fn drain_mesh_events() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.member_connected(gid, "alice", mock_transport().await);

    let events = mgr.drain_mesh_events(gid);
    assert!(!events.is_empty());

    // Second drain should be empty
    let events = mgr.drain_mesh_events(gid);
    assert!(events.is_empty());
}

// ── Encryption readiness ────────────────────────────────────────

#[test]
fn all_encryption_ready_false_with_pending() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    assert!(!mgr.all_encryption_ready(gid));
}

#[test]
fn all_encryption_ready_true_when_all_have_sessions() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    let (sender, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", sender);
    assert!(mgr.all_encryption_ready(gid));
}

// ── Connected/degraded queries ──────────────────────────────────

#[test]
fn connected_members_empty_for_unknown_group() {
    let mgr = GroupChatManager::new();
    assert!(mgr.connected_members(GroupId::new(999)).is_empty());
}

#[test]
fn degraded_members_empty_for_unknown_group() {
    let mgr = GroupChatManager::new();
    assert!(mgr.degraded_members(GroupId::new(999)).is_empty());
}

#[test]
fn drain_events_empty_for_unknown_group() {
    let mut mgr = GroupChatManager::new();
    assert!(mgr.drain_mesh_events(GroupId::new(999)).is_empty());
}

#[test]
fn all_encryption_ready_false_for_unknown_group() {
    let mgr = GroupChatManager::new();
    assert!(!mgr.all_encryption_ready(GroupId::new(999)));
}

// ── Member disconnected ─────────────────────────────────────────

#[tokio::test]
async fn member_disconnected_updates_state() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.member_connected(gid, "alice", mock_transport().await);
    mgr.member_disconnected(gid, "alice");

    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::Disconnected)
    );
}

// ── handle_member_join ──────────────────────────────────────────

#[test]
fn handle_member_join_adds_to_group() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    assert!(mgr.handle_member_join(gid, "alice"));
    assert_eq!(
        mgr.member_encryption_state(gid, "alice"),
        Some(GroupEncryptionState::Pending)
    );
    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::Connecting)
    );
}

#[test]
fn handle_member_join_duplicate_returns_false() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    assert!(mgr.handle_member_join(gid, "alice"));
    assert!(!mgr.handle_member_join(gid, "alice"));
}

#[test]
fn handle_member_join_nonexistent_group_returns_false() {
    let mut mgr = GroupChatManager::new();
    assert!(!mgr.handle_member_join(GroupId::new(999), "alice"));
}

// ── handle_member_leave ─────────────────────────────────────────

#[test]
fn handle_member_leave_removes_from_group() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);

    assert!(mgr.handle_member_leave(gid, "alice"));
    assert!(mgr.member_encryption_state(gid, "alice").is_none());
    assert!(mgr.member_connection_state(gid, "alice").is_none());
}

#[test]
fn handle_member_leave_nonexistent_member_returns_false() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    assert!(!mgr.handle_member_leave(gid, "ghost"));
}

#[test]
fn handle_member_leave_nonexistent_group_returns_false() {
    let mut mgr = GroupChatManager::new();
    assert!(!mgr.handle_member_leave(GroupId::new(999), "alice"));
}

#[tokio::test]
async fn handle_member_leave_cleans_up_encryption_and_mesh() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);
    mgr.member_connected(gid, "alice", mock_transport().await);

    assert!(mgr.all_encryption_ready(gid));
    assert!(!mgr.connected_members(gid).is_empty());

    assert!(mgr.handle_member_leave(gid, "alice"));

    // Both encryption and mesh should be cleaned up
    assert!(mgr.member_encryption_state(gid, "alice").is_none());
    assert!(mgr.connected_members(gid).is_empty());
}

// ── group_members query ─────────────────────────────────────────

#[test]
fn group_members_returns_all() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.add_member(gid, "bob");

    let mut members = mgr.group_members(gid);
    members.sort();
    assert_eq!(members, vec!["alice", "bob"]);
}

#[test]
fn group_members_empty_for_unknown_group() {
    let mgr = GroupChatManager::new();
    assert!(mgr.group_members(GroupId::new(999)).is_empty());
}
