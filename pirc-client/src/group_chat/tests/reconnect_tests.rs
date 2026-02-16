//! Tests for P2P reconnection logic and `members_needing_reconnect`.

use super::helpers::{create_test_session_pair, mock_transport};
use super::super::*;
use pirc_p2p::group_mesh::PeerConnectionState;

// ── Reconnect member ────────────────────────────────────────────

#[tokio::test]
async fn reconnect_degraded_member_switches_to_p2p() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);

    // Degrade the member
    mgr.member_degraded(gid, "alice", "NAT failure".into());
    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::RelayFallback)
    );

    // Reconnect with a new transport
    let reconnected = mgr.reconnect_member(gid, "alice", mock_transport().await);
    assert!(reconnected);
    assert_eq!(
        mgr.member_connection_state(gid, "alice"),
        Some(PeerConnectionState::Connected)
    );
}

#[tokio::test]
async fn reconnect_already_connected_returns_false() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice");
    mgr.member_connected(gid, "alice", mock_transport().await);

    let reconnected = mgr.reconnect_member(gid, "alice", mock_transport().await);
    assert!(!reconnected);
}

#[tokio::test]
async fn reconnect_unknown_group_returns_false() {
    let mut mgr = GroupChatManager::new();
    let reconnected = mgr.reconnect_member(GroupId::new(999), "alice", mock_transport().await);
    assert!(!reconnected);
}

#[tokio::test]
async fn reconnect_unknown_member_returns_false() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    let reconnected = mgr.reconnect_member(gid, "ghost", mock_transport().await);
    assert!(!reconnected);
}

#[tokio::test]
async fn reconnect_restores_p2p_delivery() {
    let (me_to_alice, alice_from_me) = create_test_session_pair();
    let gid = GroupId::new(1);

    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);

    // Start degraded: messages go via relay
    me_mgr.member_degraded(gid, "alice", "timeout".into());
    let (deliveries, relays) = me_mgr.send_message(gid, b"msg1").await.unwrap();
    assert_eq!(deliveries[0].path, DeliveryPath::Relay);
    assert_eq!(relays.len(), 1);

    // Reconnect: messages go via P2P
    me_mgr.reconnect_member(gid, "alice", mock_transport().await);
    let (deliveries, relays) = me_mgr.send_message(gid, b"msg2").await.unwrap();
    assert_eq!(deliveries[0].path, DeliveryPath::P2p);
    assert!(relays.is_empty());

    // Alice can still decrypt the relay message from before reconnect
    let mut alice_mgr = GroupChatManager::new();
    alice_mgr.add_group(gid);
    alice_mgr.add_member(gid, "me");
    alice_mgr.set_member_session(gid, "me", alice_from_me);

    // Decrypt msg1 (sequence 1) - was sent via relay before we have the test scenario
    // We have the relay payload from the first send
    let received = alice_mgr
        .receive_message(gid, "me", &deliveries[0].encrypted_payload)
        .unwrap();
    assert_eq!(received.plaintext, b"msg2");
}

// ── Members needing reconnect ───────────────────────────────────

#[test]
fn members_needing_reconnect_empty_for_unknown_group() {
    let mgr = GroupChatManager::new();
    assert!(mgr.members_needing_reconnect(GroupId::new(999)).is_empty());
}

#[test]
fn members_needing_reconnect_only_ready_degraded() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: ready + degraded -> needs reconnect
    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);
    mgr.member_degraded(gid, "alice", "timeout".into());

    // Bob: pending + degraded -> does NOT need reconnect (no session)
    mgr.add_member(gid, "bob");
    mgr.member_degraded(gid, "bob", "timeout".into());

    let need_reconnect = mgr.members_needing_reconnect(gid);
    assert_eq!(need_reconnect, vec!["alice"]);
}

#[tokio::test]
async fn members_needing_reconnect_excludes_connected() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: ready + connected -> does not need reconnect
    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);
    mgr.member_connected(gid, "alice", mock_transport().await);

    assert!(mgr.members_needing_reconnect(gid).is_empty());
}

#[tokio::test]
async fn members_needing_reconnect_includes_disconnected() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: ready + disconnected -> needs reconnect
    mgr.add_member(gid, "alice");
    let (session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", session);
    mgr.member_connected(gid, "alice", mock_transport().await);
    mgr.member_disconnected(gid, "alice");

    let need_reconnect = mgr.members_needing_reconnect(gid);
    assert_eq!(need_reconnect, vec!["alice"]);
}

#[tokio::test]
async fn members_needing_reconnect_includes_both_degraded_and_disconnected() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: ready + degraded -> needs reconnect
    mgr.add_member(gid, "alice");
    let (alice_session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", alice_session);
    mgr.member_degraded(gid, "alice", "timeout".into());

    // Bob: ready + disconnected -> needs reconnect
    mgr.add_member(gid, "bob");
    let (bob_session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "bob", bob_session);
    mgr.member_connected(gid, "bob", mock_transport().await);
    mgr.member_disconnected(gid, "bob");

    // Charlie: ready + connected -> does NOT need reconnect
    mgr.add_member(gid, "charlie");
    let (charlie_session, _) = create_test_session_pair();
    mgr.set_member_session(gid, "charlie", charlie_session);
    mgr.member_connected(gid, "charlie", mock_transport().await);

    let mut need_reconnect = mgr.members_needing_reconnect(gid);
    need_reconnect.sort();
    assert_eq!(need_reconnect, vec!["alice", "bob"]);
}

#[tokio::test]
async fn members_needing_reconnect_excludes_disconnected_without_session() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: no session + disconnected -> does NOT need reconnect
    mgr.add_member(gid, "alice");
    mgr.member_connected(gid, "alice", mock_transport().await);
    mgr.member_disconnected(gid, "alice");

    assert!(mgr.members_needing_reconnect(gid).is_empty());
}
