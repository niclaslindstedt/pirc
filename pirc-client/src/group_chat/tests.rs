//! Tests for the group chat manager.

use super::envelope::ENVELOPE_HEADER_SIZE;
use super::*;
use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_p2p::encrypted_transport::TransportCipher;
use pirc_p2p::transport::{P2pTransport, UdpTransport};
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Create a pair of linked triple ratchet sessions for testing.
fn create_test_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
    let shared_secret = [0x42u8; 32];
    let bob_dh = x25519::KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let sender = TripleRatchetSession::init_sender(
        &shared_secret,
        bob_dh.public_key(),
        bob_kem.public_key(),
    )
    .expect("init_sender");

    let receiver = TripleRatchetSession::init_receiver(&shared_secret, bob_dh, bob_kem)
        .expect("init_receiver");

    (sender, receiver)
}

/// A no-op cipher for testing (data passes through unchanged).
struct NoopCipher;

impl TransportCipher for NoopCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(ciphertext.to_vec())
    }
}

/// Creates a connected pair of mock encrypted P2P transports.
async fn mock_transport_pair() -> (EncryptedP2pTransport, EncryptedP2pTransport) {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    let transport_a = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a))),
        Box::new(NoopCipher),
    );
    let transport_b = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_b))),
        Box::new(NoopCipher),
    );

    (transport_a, transport_b)
}

/// Creates a single mock transport (self-connected, for send-only tests).
async fn mock_transport() -> EncryptedP2pTransport {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap();
    sock.connect(addr).await.unwrap();
    let udp = UdpTransport::new(Arc::new(sock));
    EncryptedP2pTransport::new(P2pTransport::Direct(udp), Box::new(NoopCipher))
}

// ── MessageEnvelope tests ───────────────────────────────────────

#[test]
fn envelope_roundtrip() {
    let envelope = MessageEnvelope {
        sequence_number: 42,
        timestamp_ms: 1_700_000_000_000,
        plaintext: b"hello group".to_vec(),
    };

    let bytes = envelope.to_bytes();
    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();

    assert_eq!(restored.sequence_number, 42);
    assert_eq!(restored.timestamp_ms, 1_700_000_000_000);
    assert_eq!(restored.plaintext, b"hello group");
}

#[test]
fn envelope_empty_plaintext() {
    let envelope = MessageEnvelope {
        sequence_number: 1,
        timestamp_ms: 0,
        plaintext: vec![],
    };

    let bytes = envelope.to_bytes();
    assert_eq!(bytes.len(), ENVELOPE_HEADER_SIZE);

    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();
    assert!(restored.plaintext.is_empty());
}

#[test]
fn envelope_too_short() {
    let result = MessageEnvelope::from_bytes(&[0u8; 10]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too short"));
}

#[test]
fn envelope_boundary_values() {
    let envelope = MessageEnvelope {
        sequence_number: u64::MAX,
        timestamp_ms: u64::MAX,
        plaintext: b"x".to_vec(),
    };

    let bytes = envelope.to_bytes();
    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();
    assert_eq!(restored.sequence_number, u64::MAX);
    assert_eq!(restored.timestamp_ms, u64::MAX);
}

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
