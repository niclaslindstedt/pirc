//! Group messaging integration tests.
//!
//! Exercises encrypted message broadcast, E2E encryption verification,
//! message ordering, and large message handling across group chats.

use pirc_common::types::GroupId;
use pirc_crypto::group_key::GroupKeyManager;

use super::{create_test_session_pair, create_test_session_pair_unique, make_encrypted_transport_pair, XorCipher};

// ── Broadcast to all peers ───────────────────────────────────────────

#[tokio::test]
async fn message_encrypted_for_each_group_member() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    // Add two members with pairwise sessions
    let (sender_to_alice, _alice_from_sender) = create_test_session_pair_unique(0x10);
    let (sender_to_bob, _bob_from_sender) = create_test_session_pair_unique(0x20);

    key_mgr.add_member("alice");
    key_mgr.set_session("alice", sender_to_alice);

    key_mgr.add_member("bob");
    key_mgr.set_session("bob", sender_to_bob);

    assert!(key_mgr.all_ready());

    // Encrypt for the group
    let plaintext = b"hello group!";
    let encrypted_map = key_mgr.encrypt_for_group(plaintext).expect("encrypt");

    // Each member gets an individually encrypted message
    assert_eq!(encrypted_map.len(), 2);
    assert!(encrypted_map.contains_key("alice"));
    assert!(encrypted_map.contains_key("bob"));

    // The encrypted payloads for alice and bob should differ
    let alice_bytes = encrypted_map["alice"].to_bytes();
    let bob_bytes = encrypted_map["bob"].to_bytes();
    assert_ne!(alice_bytes, bob_bytes, "each member gets a unique ciphertext");
}

#[tokio::test]
async fn each_member_can_decrypt_their_message() {
    let group_id = GroupId::new(1);

    // Sender's key manager
    let mut sender_mgr = GroupKeyManager::new(group_id);

    // Alice's key manager (from Alice's perspective, sender is a member)
    let mut alice_mgr = GroupKeyManager::new(group_id);

    // Bob's key manager
    let mut bob_mgr = GroupKeyManager::new(group_id);

    // Create pairwise sessions: sender <-> alice
    let (s_to_a, a_from_s) = create_test_session_pair_unique(0x10);
    sender_mgr.add_member("alice");
    sender_mgr.set_session("alice", s_to_a);
    alice_mgr.add_member("sender");
    alice_mgr.set_session("sender", a_from_s);

    // Create pairwise sessions: sender <-> bob
    let (s_to_b, b_from_s) = create_test_session_pair_unique(0x20);
    sender_mgr.add_member("bob");
    sender_mgr.set_session("bob", s_to_b);
    bob_mgr.add_member("sender");
    bob_mgr.set_session("sender", b_from_s);

    // Sender encrypts for the group
    let plaintext = b"secret group message";
    let encrypted_map = sender_mgr.encrypt_for_group(plaintext).expect("encrypt");

    // Alice decrypts her copy
    let alice_decrypted = alice_mgr
        .decrypt_from_member("sender", &encrypted_map["alice"])
        .expect("alice decrypt");
    assert_eq!(alice_decrypted, plaintext);

    // Bob decrypts his copy
    let bob_decrypted = bob_mgr
        .decrypt_from_member("sender", &encrypted_map["bob"])
        .expect("bob decrypt");
    assert_eq!(bob_decrypted, plaintext);
}

// ── E2E encryption verified ─────────────────────────────────────────

#[tokio::test]
async fn encrypted_payload_differs_from_plaintext() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    let (session, _) = create_test_session_pair();
    key_mgr.add_member("alice");
    key_mgr.set_session("alice", session);

    let plaintext = b"this should be encrypted";
    let encrypted_map = key_mgr.encrypt_for_group(plaintext).expect("encrypt");

    let ciphertext = encrypted_map["alice"].to_bytes();

    // Plaintext should not appear in the ciphertext
    assert!(
        !ciphertext
            .windows(plaintext.len())
            .any(|w| w == plaintext),
        "plaintext must not appear in ciphertext"
    );
}

#[tokio::test]
async fn intermediary_cannot_decrypt_message() {
    let group_id = GroupId::new(1);

    // Sender encrypts for alice
    let mut sender_mgr = GroupKeyManager::new(group_id);
    let (s_to_a, _a_from_s) = create_test_session_pair_unique(0x10);
    sender_mgr.add_member("alice");
    sender_mgr.set_session("alice", s_to_a);

    let plaintext = b"private group data";
    let encrypted_map = sender_mgr.encrypt_for_group(plaintext).expect("encrypt");

    // An intermediary (eve) with a different session cannot decrypt
    let mut eve_mgr = GroupKeyManager::new(group_id);
    let (_, eve_session) = create_test_session_pair_unique(0x99);
    eve_mgr.add_member("sender");
    eve_mgr.set_session("sender", eve_session);

    let result = eve_mgr.decrypt_from_member("sender", &encrypted_map["alice"]);
    assert!(result.is_err(), "intermediary should not be able to decrypt");
}

// ── Message ordering preserved within sender ─────────────────────────

#[tokio::test]
async fn message_ordering_preserved_for_single_sender() {
    let group_id = GroupId::new(1);

    let mut sender_mgr = GroupKeyManager::new(group_id);
    let mut receiver_mgr = GroupKeyManager::new(group_id);

    let (s_to_r, r_from_s) = create_test_session_pair_unique(0x30);
    sender_mgr.add_member("receiver");
    sender_mgr.set_session("receiver", s_to_r);
    receiver_mgr.add_member("sender");
    receiver_mgr.set_session("sender", r_from_s);

    // Send multiple messages in order
    let messages: Vec<Vec<u8>> = (0..5).map(|i| format!("msg-{i}").into_bytes()).collect();

    let mut encrypted_msgs = Vec::new();
    for msg in &messages {
        let encrypted_map = sender_mgr.encrypt_for_group(msg).expect("encrypt");
        encrypted_msgs.push(encrypted_map["receiver"].clone());
    }

    // Decrypt in the same order — should succeed
    for (i, enc_msg) in encrypted_msgs.iter().enumerate() {
        let decrypted = receiver_mgr
            .decrypt_from_member("sender", enc_msg)
            .expect("decrypt");
        assert_eq!(
            decrypted, messages[i],
            "message {i} should match after decrypt"
        );
    }
}

// ── Large messages ───────────────────────────────────────────────────

#[tokio::test]
async fn large_message_encrypt_decrypt() {
    let group_id = GroupId::new(1);

    let mut sender_mgr = GroupKeyManager::new(group_id);
    let mut receiver_mgr = GroupKeyManager::new(group_id);

    let (s_to_r, r_from_s) = create_test_session_pair_unique(0x40);
    sender_mgr.add_member("receiver");
    sender_mgr.set_session("receiver", s_to_r);
    receiver_mgr.add_member("sender");
    receiver_mgr.set_session("sender", r_from_s);

    // Large message (4KB)
    let plaintext: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
    let encrypted_map = sender_mgr.encrypt_for_group(&plaintext).expect("encrypt");

    let decrypted = receiver_mgr
        .decrypt_from_member("sender", &encrypted_map["receiver"])
        .expect("decrypt");
    assert_eq!(decrypted, plaintext);
}

// ── Encrypted transport round-trip ───────────────────────────────────

#[tokio::test]
async fn encrypted_transport_group_broadcast_simulation() {
    // Simulate sending a message from "sender" to "alice" and "bob" via
    // separate encrypted transports.

    // sender -> alice transport
    let (enc_s_a, enc_a_s) = make_encrypted_transport_pair(
        Box::new(XorCipher::new(0x42)),
        Box::new(XorCipher::new(0x42)),
    )
    .await;

    // sender -> bob transport
    let (enc_s_b, enc_b_s) = make_encrypted_transport_pair(
        Box::new(XorCipher::new(0x55)),
        Box::new(XorCipher::new(0x55)),
    )
    .await;

    let msg = b"group broadcast message";

    // Send to both alice and bob
    enc_s_a.send(msg).await.unwrap();
    enc_s_b.send(msg).await.unwrap();

    // Both receive the same plaintext
    let from_alice = enc_a_s.recv().await.unwrap();
    let from_bob = enc_b_s.recv().await.unwrap();

    assert_eq!(from_alice, msg.to_vec());
    assert_eq!(from_bob, msg.to_vec());
}

#[tokio::test]
async fn encrypted_transport_data_on_wire_is_encrypted() {
    use std::sync::Arc;
    use tokio::net::UdpSocket;
    use pirc_p2p::transport::{P2pTransport, UdpTransport};

    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    let raw_sock_b = Arc::new(sock_b);

    let enc_a = pirc_p2p::EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a))),
        Box::new(XorCipher::new(0xFF)),
    );

    let plaintext = b"group secret message";
    enc_a.send(plaintext).await.unwrap();

    // Read raw bytes from the wire
    let mut buf = [0u8; 2048];
    let n = raw_sock_b.recv(&mut buf).await.unwrap();
    let wire_data = &buf[..n];

    // Plaintext should NOT appear on the wire
    assert!(
        !wire_data.windows(plaintext.len()).any(|w| w == plaintext),
        "plaintext should not appear on the wire"
    );
}

// ── Multiple messages via transport ──────────────────────────────────

#[tokio::test]
async fn multiple_messages_via_encrypted_transport() {
    let (enc_a, enc_b) = make_encrypted_transport_pair(
        Box::new(XorCipher::new(0x77)),
        Box::new(XorCipher::new(0x77)),
    )
    .await;

    for i in 0..10 {
        let msg = format!("group msg #{i}");
        enc_a.send(msg.as_bytes()).await.unwrap();
        let received = enc_b.recv().await.unwrap();
        assert_eq!(received, msg.as_bytes());
    }
}

// ── Bidirectional group transport ────────────────────────────────────

#[tokio::test]
async fn bidirectional_group_messaging() {
    let (enc_a, enc_b) = make_encrypted_transport_pair(
        Box::new(XorCipher::new(0x33)),
        Box::new(XorCipher::new(0x33)),
    )
    .await;

    // alice -> bob
    enc_a.send(b"hello from alice").await.unwrap();
    assert_eq!(enc_b.recv().await.unwrap(), b"hello from alice");

    // bob -> alice
    enc_b.send(b"hello from bob").await.unwrap();
    assert_eq!(enc_a.recv().await.unwrap(), b"hello from bob");

    // alice -> bob again
    enc_a.send(b"another message").await.unwrap();
    assert_eq!(enc_b.recv().await.unwrap(), b"another message");
}
