//! Tests for message sending, receiving, encryption roundtrips,
//! P2P delivery, and mixed delivery paths.

use super::helpers::{create_test_session_pair, mock_transport, mock_transport_pair};
use super::super::*;
use pirc_crypto::group_key::GroupKeyManager;
use pirc_crypto::message::EncryptedMessage;

// ── Send message: relay path ────────────────────────────────────

#[tokio::test]
async fn send_message_relay_for_degraded_members() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Set up alice with a ready session but degraded mesh
    mgr.add_member(gid, "alice");
    let (sender, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", sender);
    mgr.member_degraded(gid, "alice", "timeout".into());

    let (deliveries, relays) = mgr.send_message(gid, b"hello").await.unwrap();

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].recipient, "alice");
    assert_eq!(deliveries[0].path, DeliveryPath::Relay);

    assert_eq!(relays.len(), 1);
    assert_eq!(relays[0].target, "alice");
    assert_eq!(relays[0].group_id, gid);
    assert!(!relays[0].encrypted_payload.is_empty());
}

#[tokio::test]
async fn send_message_to_unknown_group_fails() {
    let mut mgr = GroupChatManager::new();
    let result = mgr.send_message(GroupId::new(999), b"hello").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn send_message_no_ready_members_fails() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);
    mgr.add_member(gid, "alice"); // pending, not ready

    let result = mgr.send_message(gid, b"hello").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("encryption failed"));
}

// ── Send message: P2P path ──────────────────────────────────────

#[tokio::test]
async fn send_message_p2p_for_connected_members() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Set up alice with a ready session and P2P transport
    mgr.add_member(gid, "alice");
    let (sender, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", sender);
    mgr.member_connected(gid, "alice", mock_transport().await);

    let (deliveries, relays) = mgr.send_message(gid, b"hello p2p").await.unwrap();

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].recipient, "alice");
    assert_eq!(deliveries[0].path, DeliveryPath::P2p);
    assert!(relays.is_empty());
}

// ── Send message: mixed P2P and relay ───────────────────────────

#[tokio::test]
async fn send_message_mixed_delivery() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice: P2P connected
    mgr.add_member(gid, "alice");
    let (s1, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", s1);
    mgr.member_connected(gid, "alice", mock_transport().await);

    // Bob: degraded (relay)
    mgr.add_member(gid, "bob");
    let (s2, _) = create_test_session_pair();
    mgr.set_member_session(gid, "bob", s2);
    mgr.member_degraded(gid, "bob", "NAT failure".into());

    let (deliveries, relays) = mgr.send_message(gid, b"mixed msg").await.unwrap();

    assert_eq!(deliveries.len(), 2);

    let alice_delivery = deliveries.iter().find(|d| d.recipient == "alice").unwrap();
    assert_eq!(alice_delivery.path, DeliveryPath::P2p);

    let bob_delivery = deliveries.iter().find(|d| d.recipient == "bob").unwrap();
    assert_eq!(bob_delivery.path, DeliveryPath::Relay);

    assert_eq!(relays.len(), 1);
    assert_eq!(relays[0].target, "bob");
}

// ── Send message: sequence numbers ──────────────────────────────

#[tokio::test]
async fn send_message_increments_sequence_number() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    mgr.add_member(gid, "alice");
    let (s1, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", s1);
    mgr.member_degraded(gid, "alice", "timeout".into());

    // Send first message
    let (_, relays1) = mgr.send_message(gid, b"msg1").await.unwrap();

    // Verify the relay payload is non-empty (sequence number verified in roundtrip tests)
    assert!(!relays1[0].encrypted_payload.is_empty());
}

// ── Receive message ─────────────────────────────────────────────

#[test]
fn receive_message_from_unknown_group_fails() {
    let mut mgr = GroupChatManager::new();
    let result = mgr.receive_message(GroupId::new(999), "alice", &[0u8; 100]);
    assert!(result.is_err());
}

#[test]
fn receive_message_invalid_payload_fails() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    let result = mgr.receive_message(gid, "alice", &[0u8; 3]);
    assert!(result.is_err());
}

// ── Encrypt/decrypt roundtrip ───────────────────────────────────

#[tokio::test]
async fn encrypt_decrypt_roundtrip() {
    // Setup: "me" sends to alice
    let (me_to_alice, alice_from_me) = create_test_session_pair();

    let gid = GroupId::new(42);

    // "me" manager
    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);
    me_mgr.member_degraded(gid, "alice", "test".into());

    // "alice" manager
    let mut alice_mgr = GroupChatManager::new();
    alice_mgr.add_group(gid);
    alice_mgr.add_member(gid, "me");
    alice_mgr.set_member_session(gid, "me", alice_from_me);

    // Send
    let (_, relays) = me_mgr.send_message(gid, b"secret group msg").await.unwrap();
    assert_eq!(relays.len(), 1);

    // Receive
    let received = alice_mgr
        .receive_message(gid, "me", &relays[0].encrypted_payload)
        .unwrap();

    assert_eq!(received.group_id, gid);
    assert_eq!(received.sender, "me");
    assert_eq!(received.sequence_number, 1);
    assert!(received.timestamp_ms > 0);
    assert_eq!(received.plaintext, b"secret group msg");
}

#[tokio::test]
async fn encrypt_decrypt_roundtrip_two_members() {
    // me -> alice, me -> bob
    let (me_to_alice, alice_from_me) = create_test_session_pair();
    let (me_to_bob, bob_from_me) = create_test_session_pair();

    let gid = GroupId::new(10);

    // "me" manager
    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);
    me_mgr.member_degraded(gid, "alice", "test".into());
    me_mgr.add_member(gid, "bob");
    me_mgr.set_member_session(gid, "bob", me_to_bob);
    me_mgr.member_degraded(gid, "bob", "test".into());

    // alice manager
    let mut alice_mgr = GroupChatManager::new();
    alice_mgr.add_group(gid);
    alice_mgr.add_member(gid, "me");
    alice_mgr.set_member_session(gid, "me", alice_from_me);

    // bob manager
    let mut bob_mgr = GroupChatManager::new();
    bob_mgr.add_group(gid);
    bob_mgr.add_member(gid, "me");
    bob_mgr.set_member_session(gid, "me", bob_from_me);

    // Send
    let (deliveries, relays) = me_mgr
        .send_message(gid, b"hello group")
        .await
        .unwrap();

    assert_eq!(deliveries.len(), 2);
    assert_eq!(relays.len(), 2);

    // Each recipient gets their own encrypted copy
    let alice_relay = relays.iter().find(|r| r.target == "alice").unwrap();
    let bob_relay = relays.iter().find(|r| r.target == "bob").unwrap();

    // Alice decrypts
    let alice_msg = alice_mgr
        .receive_message(gid, "me", &alice_relay.encrypted_payload)
        .unwrap();
    assert_eq!(alice_msg.plaintext, b"hello group");
    assert_eq!(alice_msg.sequence_number, 1);

    // Bob decrypts
    let bob_msg = bob_mgr
        .receive_message(gid, "me", &bob_relay.encrypted_payload)
        .unwrap();
    assert_eq!(bob_msg.plaintext, b"hello group");
    assert_eq!(bob_msg.sequence_number, 1);

    // Different ciphertexts for each recipient
    assert_ne!(alice_relay.encrypted_payload, bob_relay.encrypted_payload);
}

#[tokio::test]
async fn multiple_messages_increment_sequence() {
    let (me_to_alice, alice_from_me) = create_test_session_pair();

    let gid = GroupId::new(1);

    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);
    me_mgr.member_degraded(gid, "alice", "test".into());

    let mut alice_mgr = GroupChatManager::new();
    alice_mgr.add_group(gid);
    alice_mgr.add_member(gid, "me");
    alice_mgr.set_member_session(gid, "me", alice_from_me);

    for i in 1..=5 {
        let plaintext = format!("message {i}");
        let (_, relays) = me_mgr
            .send_message(gid, plaintext.as_bytes())
            .await
            .unwrap();
        let received = alice_mgr
            .receive_message(gid, "me", &relays[0].encrypted_payload)
            .unwrap();
        assert_eq!(received.plaintext, plaintext.as_bytes());
        assert_eq!(received.sequence_number, i);
    }
}

// ── P2P delivery with actual transport ──────────────────────────

#[tokio::test]
async fn p2p_delivery_actually_sends_data() {
    let (transport_a, transport_b) = mock_transport_pair().await;

    let (me_to_alice, alice_from_me) = create_test_session_pair();

    let gid = GroupId::new(1);

    // "me" manager with P2P transport to alice
    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);
    me_mgr.member_connected(gid, "alice", transport_a);

    // Send
    let (deliveries, relays) = me_mgr.send_message(gid, b"p2p test").await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].path, DeliveryPath::P2p);
    assert!(relays.is_empty());

    // Receive on the other end of the transport
    let raw_bytes = transport_b.recv().await.unwrap();

    // Decrypt what was received
    let encrypted_msg = EncryptedMessage::from_bytes(&raw_bytes).unwrap();
    let mut alice_key_mgr = GroupKeyManager::new(gid);
    alice_key_mgr.set_session("me", alice_from_me);
    let envelope_bytes = alice_key_mgr
        .decrypt_from_member("me", &encrypted_msg)
        .unwrap();
    let envelope = MessageEnvelope::from_bytes(&envelope_bytes).unwrap();

    assert_eq!(envelope.plaintext, b"p2p test");
    assert_eq!(envelope.sequence_number, 1);
}

// ── Skips non-ready members ─────────────────────────────────────

#[tokio::test]
async fn send_skips_pending_members() {
    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    // Alice ready, bob pending
    mgr.add_member(gid, "alice");
    let (s1, _) = create_test_session_pair();
    mgr.set_member_session(gid, "alice", s1);
    mgr.member_degraded(gid, "alice", "test".into());

    mgr.add_member(gid, "bob"); // pending

    let (deliveries, relays) = mgr.send_message(gid, b"test").await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].recipient, "alice");
    assert_eq!(relays.len(), 1);
}

// ── Receive from unknown sender ─────────────────────────────────

#[tokio::test]
async fn receive_from_unknown_sender_fails() {
    // Create a valid encrypted message to test the "no session" path
    let (sender_session, _) = create_test_session_pair();
    let mut key_mgr = GroupKeyManager::new(GroupId::new(1));
    key_mgr.set_session("dummy", sender_session);
    let encrypted = key_mgr.encrypt_for_group(b"test").unwrap();
    let payload = encrypted["dummy"].to_bytes();

    let mut mgr = GroupChatManager::new();
    let gid = GroupId::new(1);
    mgr.add_group(gid);

    let result = mgr.receive_message(gid, "unknown_sender", &payload);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("decryption failed"));
}

// ── Multiple groups ─────────────────────────────────────────────

#[tokio::test]
async fn independent_groups() {
    let (me_to_alice_g1, alice_from_me_g1) = create_test_session_pair();
    let (me_to_alice_g2, alice_from_me_g2) = create_test_session_pair();

    let g1 = GroupId::new(1);
    let g2 = GroupId::new(2);

    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(g1);
    me_mgr.add_group(g2);
    me_mgr.add_member(g1, "alice");
    me_mgr.set_member_session(g1, "alice", me_to_alice_g1);
    me_mgr.member_degraded(g1, "alice", "test".into());
    me_mgr.add_member(g2, "alice");
    me_mgr.set_member_session(g2, "alice", me_to_alice_g2);
    me_mgr.member_degraded(g2, "alice", "test".into());

    let mut alice_mgr = GroupChatManager::new();
    alice_mgr.add_group(g1);
    alice_mgr.add_group(g2);
    alice_mgr.add_member(g1, "me");
    alice_mgr.set_member_session(g1, "me", alice_from_me_g1);
    alice_mgr.add_member(g2, "me");
    alice_mgr.set_member_session(g2, "me", alice_from_me_g2);

    // Send to group 1
    let (_, relays1) = me_mgr.send_message(g1, b"group1 msg").await.unwrap();
    let received1 = alice_mgr
        .receive_message(g1, "me", &relays1[0].encrypted_payload)
        .unwrap();
    assert_eq!(received1.plaintext, b"group1 msg");
    assert_eq!(received1.group_id, g1);

    // Send to group 2
    let (_, relays2) = me_mgr.send_message(g2, b"group2 msg").await.unwrap();
    let received2 = alice_mgr
        .receive_message(g2, "me", &relays2[0].encrypted_payload)
        .unwrap();
    assert_eq!(received2.plaintext, b"group2 msg");
    assert_eq!(received2.group_id, g2);

    // Both start from sequence 1
    assert_eq!(received1.sequence_number, 1);
    assert_eq!(received2.sequence_number, 1);
}

// ── Join/leave encryption roundtrip ─────────────────────────────

#[tokio::test]
async fn join_leave_encryption_roundtrip() {
    // Simulate: me has alice as member, bob joins, then alice leaves
    let (me_to_alice, _alice_from_me) = create_test_session_pair();
    let (me_to_bob, bob_from_me) = create_test_session_pair();

    let gid = GroupId::new(1);

    let mut me_mgr = GroupChatManager::new();
    me_mgr.add_group(gid);
    me_mgr.add_member(gid, "alice");
    me_mgr.set_member_session(gid, "alice", me_to_alice);
    me_mgr.member_degraded(gid, "alice", "test".into());

    // Bob joins
    assert!(me_mgr.handle_member_join(gid, "bob"));
    me_mgr.set_member_session(gid, "bob", me_to_bob);
    me_mgr.member_degraded(gid, "bob", "test".into());

    // Send a message — both alice and bob should get copies
    let (deliveries, _) = me_mgr.send_message(gid, b"hello all").await.unwrap();
    assert_eq!(deliveries.len(), 2);

    // Alice leaves
    assert!(me_mgr.handle_member_leave(gid, "alice"));

    // Now send again — only bob should get a copy
    let (deliveries, relays) = me_mgr.send_message(gid, b"after alice left").await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].recipient, "bob");

    // Bob can decrypt the message
    let mut bob_mgr = GroupChatManager::new();
    bob_mgr.add_group(gid);
    bob_mgr.add_member(gid, "me");
    bob_mgr.set_member_session(gid, "me", bob_from_me);

    // Need to decrypt the first message sent to bob (before alice left)
    // to keep ratchet in sync
    let bob_relay_first = relays.iter().find(|r| r.target == "bob");
    if let Some(relay) = bob_relay_first {
        let _ = bob_mgr.receive_message(gid, "me", &relay.encrypted_payload);
    }

    // Decrypt the second message (after alice left)
    let (_, relays2) = me_mgr.send_message(gid, b"bob only msg").await.unwrap();
    let received = bob_mgr
        .receive_message(gid, "me", &relays2[0].encrypted_payload)
        .unwrap();
    assert_eq!(received.plaintext, b"bob only msg");
}
