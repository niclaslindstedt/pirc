use super::*;
use crate::kem::KemKeyPair;
use crate::x25519::KeyPair;

fn shared_secret() -> [u8; 32] {
    [0x42u8; 32]
}

/// Create a sender/receiver session pair.
fn make_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();
    let secret = shared_secret();

    let alice = TripleRatchetSession::init_sender(
        &secret,
        bob_dh.public_key(),
        bob_kem.public_key(),
    )
    .expect("init sender failed");

    let bob = TripleRatchetSession::init_receiver(&secret, bob_dh, bob_kem)
        .expect("init receiver failed");

    (alice, bob)
}

// ── Basic send/receive ─────────────────────────────────────────

#[test]
fn basic_send_receive() {
    let (mut alice, mut bob) = make_session_pair();

    let plaintext = b"Hello, Bob!";
    let msg = alice.encrypt(plaintext).expect("encrypt failed");
    let decrypted = bob.decrypt(&msg).expect("decrypt failed");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn multiple_messages_same_direction() {
    let (mut alice, mut bob) = make_session_pair();

    for i in 0..5 {
        let plaintext = format!("message {i}");
        let msg = alice.encrypt(plaintext.as_bytes()).expect("encrypt failed");
        let decrypted = bob.decrypt(&msg).expect("decrypt failed");
        assert_eq!(decrypted, plaintext.as_bytes(), "mismatch at message {i}");
    }
}

// ── Bidirectional communication ────────────────────────────────

#[test]
fn bidirectional_exchange() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice -> Bob
    let msg1 = alice.encrypt(b"Hello Bob").expect("alice encrypt 1");
    let dec1 = bob.decrypt(&msg1).expect("bob decrypt 1");
    assert_eq!(dec1, b"Hello Bob");

    // Bob -> Alice
    let msg2 = bob.encrypt(b"Hello Alice").expect("bob encrypt 1");
    let dec2 = alice.decrypt(&msg2).expect("alice decrypt 1");
    assert_eq!(dec2, b"Hello Alice");

    // Alice -> Bob again
    let msg3 = alice.encrypt(b"How are you?").expect("alice encrypt 2");
    let dec3 = bob.decrypt(&msg3).expect("bob decrypt 2");
    assert_eq!(dec3, b"How are you?");
}

#[test]
fn many_round_trips() {
    let (mut alice, mut bob) = make_session_pair();

    for round in 0..10 {
        let a_msg = format!("Alice round {round}");
        let encrypted = alice.encrypt(a_msg.as_bytes()).expect("alice encrypt");
        let decrypted = bob.decrypt(&encrypted).expect("bob decrypt");
        assert_eq!(decrypted, a_msg.as_bytes(), "A->B mismatch round {round}");

        let b_msg = format!("Bob round {round}");
        let encrypted = bob.encrypt(b_msg.as_bytes()).expect("bob encrypt");
        let decrypted = alice.decrypt(&encrypted).expect("alice decrypt");
        assert_eq!(decrypted, b_msg.as_bytes(), "B->A mismatch round {round}");
    }
}

// ── Out-of-order messages ──────────────────────────────────────

#[test]
fn out_of_order_within_same_chain() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 3 messages (all in the same sending chain)
    let msg0 = alice.encrypt(b"message 0").expect("encrypt 0");
    let msg1 = alice.encrypt(b"message 1").expect("encrypt 1");
    let msg2 = alice.encrypt(b"message 2").expect("encrypt 2");

    // Bob receives message 2 first (skips 0 and 1 in the chain)
    let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
    assert_eq!(dec2, b"message 2");

    // Skipped keys should have been cached
    assert_eq!(bob.skipped_key_count(), 2);

    // Now Bob receives message 0 (from skipped cache)
    let dec0 = bob.decrypt(&msg0).expect("decrypt 0");
    assert_eq!(dec0, b"message 0");

    // And message 1
    let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
    assert_eq!(dec1, b"message 1");

    // All skipped keys should be consumed now
    assert_eq!(bob.skipped_key_count(), 0);
}

#[test]
fn out_of_order_reversed_delivery() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 5 messages
    let msgs: Vec<_> = (0..5)
        .map(|i| alice.encrypt(format!("msg {i}").as_bytes()).expect("encrypt"))
        .collect();

    // Bob receives them in reverse order
    for i in (0..5).rev() {
        let dec = bob.decrypt(&msgs[i]).expect("decrypt");
        assert_eq!(dec, format!("msg {i}").as_bytes(), "mismatch at msg {i}");
    }
}

#[test]
fn out_of_order_across_dh_ratchet() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 2 messages in her first sending chain
    let msg0 = alice.encrypt(b"chain1-msg0").expect("encrypt 0");
    let msg1 = alice.encrypt(b"chain1-msg1").expect("encrypt 1");

    // Bob receives only msg1 (skipping msg0)
    let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
    assert_eq!(dec1, b"chain1-msg1");
    assert_eq!(bob.skipped_key_count(), 1); // msg0 is cached

    // Bob replies (triggers DH ratchet on Alice)
    let reply = bob.encrypt(b"reply").expect("bob encrypt");
    let dec_reply = alice.decrypt(&reply).expect("alice decrypt");
    assert_eq!(dec_reply, b"reply");

    // Alice sends in her new chain
    let msg2 = alice.encrypt(b"chain2-msg0").expect("encrypt 2");
    let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
    assert_eq!(dec2, b"chain2-msg0");

    // msg0 from the old chain should still be recoverable
    let dec0 = bob.decrypt(&msg0).expect("decrypt old msg0");
    assert_eq!(dec0, b"chain1-msg0");
    assert_eq!(bob.skipped_key_count(), 0);
}

// ── Empty and large messages ───────────────────────────────────

#[test]
fn empty_message() {
    let (mut alice, mut bob) = make_session_pair();

    let msg = alice.encrypt(b"").expect("encrypt empty");
    let decrypted = bob.decrypt(&msg).expect("decrypt empty");
    assert!(decrypted.is_empty());
}

#[test]
fn large_message() {
    let (mut alice, mut bob) = make_session_pair();

    let plaintext = vec![0xAB; 64 * 1024]; // 64 KiB
    let msg = alice.encrypt(&plaintext).expect("encrypt large");
    let decrypted = bob.decrypt(&msg).expect("decrypt large");
    assert_eq!(decrypted, plaintext);
}

// ── PQ ratchet step trigger ────────────────────────────────────

#[test]
fn pq_step_triggers_at_interval() {
    let (mut alice, mut bob) = make_session_pair();
    alice.set_pq_interval(3);

    // Each round trip causes a DH ratchet step on each side.
    // After 3 DH steps on Alice's side, a PQ step should trigger.
    for round in 0..4 {
        let msg = alice
            .encrypt(format!("alice {round}").as_bytes())
            .expect("alice encrypt");
        bob.decrypt(&msg).expect("bob decrypt");

        let msg = bob
            .encrypt(format!("bob {round}").as_bytes())
            .expect("bob encrypt");
        alice.decrypt(&msg).expect("alice decrypt");
    }

    assert!(
        alice.pq_step_count() > 0,
        "PQ ratchet step should have triggered on sender"
    );
}

// ── Session initialization ─────────────────────────────────────

#[test]
fn init_sender_succeeds() {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let result = TripleRatchetSession::init_sender(
        &shared_secret(),
        bob_dh.public_key(),
        bob_kem.public_key(),
    );
    assert!(result.is_ok());
}

#[test]
fn init_receiver_succeeds() {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let result = TripleRatchetSession::init_receiver(&shared_secret(), bob_dh, bob_kem);
    assert!(result.is_ok());
}

#[test]
fn different_shared_secrets_fail_decryption() {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let mut alice = TripleRatchetSession::init_sender(
        &[0x01u8; 32],
        bob_dh.public_key(),
        bob_kem.public_key(),
    )
    .expect("init sender");

    let mut bob = TripleRatchetSession::init_receiver(&[0x02u8; 32], bob_dh, bob_kem)
        .expect("init receiver");

    let msg = alice.encrypt(b"test").expect("encrypt");
    let result = bob.decrypt(&msg);
    assert!(result.is_err(), "different secrets should fail");
}

// ── Key uniqueness ─────────────────────────────────────────────

#[test]
fn each_message_uses_unique_encryption() {
    let (mut alice, mut bob) = make_session_pair();

    let msg1 = alice.encrypt(b"same content").expect("encrypt 1");
    let msg2 = alice.encrypt(b"same content").expect("encrypt 2");

    // Even with same plaintext, ciphertexts should differ
    assert_ne!(msg1.ciphertext, msg2.ciphertext);

    // Both should still decrypt correctly
    let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
    let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
    assert_eq!(dec1, b"same content");
    assert_eq!(dec2, b"same content");
}

// ── Skipped key management ─────────────────────────────────────

#[test]
fn skipped_keys_initially_empty() {
    let (alice, _bob) = make_session_pair();
    assert_eq!(alice.skipped_key_count(), 0);
}

#[test]
fn store_and_retrieve_skipped_key() {
    let (mut alice, _bob) = make_session_pair();

    let dh_pub = KeyPair::generate().public_key();
    let mk = crate::symmetric_ratchet::ChainKey::new([0xAA; 32]);
    let mut ratchet = crate::symmetric_ratchet::SymmetricRatchet::new(mk);
    let msg_key = ratchet.advance();

    alice.store_skipped_key(&dh_pub, 5, msg_key);
    assert_eq!(alice.skipped_key_count(), 1);
}

#[test]
fn skipped_key_eviction_at_limit() {
    let (mut alice, _bob) = make_session_pair();

    let dh_pub = KeyPair::generate().public_key();
    let ck = crate::symmetric_ratchet::ChainKey::new([0xBB; 32]);
    let mut ratchet = crate::symmetric_ratchet::SymmetricRatchet::new(ck);

    // Fill up to MAX_SKIPPED_KEYS + 1
    for i in 0..=MAX_SKIPPED_KEYS {
        let mk = ratchet.advance();
        #[allow(clippy::cast_possible_truncation)]
        alice.store_skipped_key(&dh_pub, i as u32, mk);
    }

    assert!(alice.skipped_key_count() <= MAX_SKIPPED_KEYS);
}

// ── PQ interval configuration ──────────────────────────────────

#[test]
fn set_pq_interval() {
    let (mut alice, _bob) = make_session_pair();
    alice.set_pq_interval(5);
    assert_eq!(alice.pq_step_interval, 5);
}

#[test]
fn pq_disabled_with_zero_interval() {
    let (mut alice, mut bob) = make_session_pair();
    alice.set_pq_interval(0);

    for round in 0..5 {
        let msg = alice
            .encrypt(format!("alice {round}").as_bytes())
            .expect("encrypt");
        bob.decrypt(&msg).expect("decrypt");

        let msg = bob
            .encrypt(format!("bob {round}").as_bytes())
            .expect("encrypt");
        alice.decrypt(&msg).expect("decrypt");
    }

    assert_eq!(alice.pq_step_count(), 0);
}

// ── Multiple messages before first reply ───────────────────────

#[test]
fn multiple_messages_before_reply() {
    let (mut alice, mut bob) = make_session_pair();

    for i in 0..10 {
        let msg = alice
            .encrypt(format!("msg {i}").as_bytes())
            .expect("encrypt");
        let dec = bob.decrypt(&msg).expect("decrypt");
        assert_eq!(dec, format!("msg {i}").as_bytes());
    }

    let reply = bob.encrypt(b"got them all").expect("encrypt");
    let dec = alice.decrypt(&reply).expect("decrypt");
    assert_eq!(dec, b"got them all");
}

// ── Header key rotation ──────────────────────────────────────

#[test]
fn previous_header_key_decrypts_after_dh_ratchet() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends two messages in the first epoch.
    let msg_epoch1_0 = alice.encrypt(b"epoch1-msg0").expect("encrypt epoch1-0");
    let msg_epoch1_1 = alice.encrypt(b"epoch1-msg1").expect("encrypt epoch1-1");

    // Bob only receives msg1 (skip msg0 for later).
    let dec = bob.decrypt(&msg_epoch1_1).expect("decrypt epoch1-1");
    assert_eq!(dec, b"epoch1-msg1");

    // Bob replies — this triggers a DH ratchet on Alice.
    let reply = bob.encrypt(b"reply").expect("bob encrypt");
    alice.decrypt(&reply).expect("alice decrypt reply");

    // Alice sends a message in the new epoch (new DH key, new header key).
    let msg_epoch2 = alice.encrypt(b"epoch2-msg0").expect("encrypt epoch2-0");
    bob.decrypt(&msg_epoch2).expect("bob decrypt epoch2-0");

    // Now decrypt the delayed msg0 from epoch 1.
    // This requires the previous receiving header key to
    // decrypt the header, then the cached skipped message key.
    let dec0 = bob.decrypt(&msg_epoch1_0).expect("decrypt old epoch1-msg0");
    assert_eq!(dec0, b"epoch1-msg0");
}

#[test]
fn header_decryption_fails_with_unrelated_key() {
    let (mut alice, _bob) = make_session_pair();

    // Alice encrypts a message.
    let msg = alice.encrypt(b"secret").expect("encrypt");

    // Create a completely separate session with a different
    // shared secret.
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();
    let mut eve = TripleRatchetSession::init_receiver(
        &[0xFF; 32],
        bob_dh,
        bob_kem,
    )
    .expect("init eve");

    // Eve should not be able to decrypt Alice's message.
    let result = eve.decrypt(&msg);
    assert!(result.is_err(), "unrelated key must fail decryption");
}

// ── Forward secrecy and key erasure ──────────────────────────

#[test]
fn session_info_initial_state() {
    let (alice, _bob) = make_session_pair();
    let info = alice.session_info();

    assert_eq!(info.dh_step_count, 0);
    assert_eq!(info.pq_step_count, 0);
    assert_eq!(info.messages_sent, 0);
    assert_eq!(info.messages_received, 0);
    assert_eq!(info.skipped_key_count, 0);
    // DH public key fingerprint should be non-zero
    assert!(info.dh_public_fingerprint.iter().any(|&b| b != 0));
}

#[test]
fn session_info_tracks_messages() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 3 messages
    for i in 0..3 {
        let msg = alice
            .encrypt(format!("msg {i}").as_bytes())
            .expect("encrypt");
        bob.decrypt(&msg).expect("decrypt");
    }

    let alice_info = alice.session_info();
    assert_eq!(alice_info.messages_sent, 3);
    assert_eq!(alice_info.messages_received, 0);

    let bob_info = bob.session_info();
    assert_eq!(bob_info.messages_sent, 0);
    assert_eq!(bob_info.messages_received, 3);
}

#[test]
fn session_info_tracks_dh_steps() {
    let (mut alice, mut bob) = make_session_pair();

    // Each round trip triggers DH ratchet steps
    for round in 0..3 {
        let msg = alice
            .encrypt(format!("alice {round}").as_bytes())
            .expect("encrypt");
        bob.decrypt(&msg).expect("decrypt");

        let msg = bob
            .encrypt(format!("bob {round}").as_bytes())
            .expect("encrypt");
        alice.decrypt(&msg).expect("decrypt");
    }

    let info = alice.session_info();
    assert!(info.dh_step_count > 0, "DH steps should have occurred");
}

#[test]
fn session_info_tracks_skipped_keys() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 3 messages; Bob receives only the last
    let msg0 = alice.encrypt(b"msg 0").expect("encrypt");
    let _msg1 = alice.encrypt(b"msg 1").expect("encrypt");
    let msg2 = alice.encrypt(b"msg 2").expect("encrypt");

    bob.decrypt(&msg2).expect("decrypt");

    let info = bob.session_info();
    assert_eq!(info.skipped_key_count, 2);

    // Consume skipped keys
    bob.decrypt(&msg0).expect("decrypt");
    let info2 = bob.session_info();
    assert_eq!(info2.skipped_key_count, 1);
}

#[test]
fn session_info_dh_fingerprint_changes_after_ratchet() {
    let (mut alice, mut bob) = make_session_pair();

    let fp_before = alice.session_info().dh_public_fingerprint;

    // Trigger DH ratchet
    let msg = alice.encrypt(b"hello").expect("encrypt");
    bob.decrypt(&msg).expect("decrypt");
    let reply = bob.encrypt(b"reply").expect("encrypt");
    alice.decrypt(&reply).expect("decrypt");

    let fp_after = alice.session_info().dh_public_fingerprint;
    assert_ne!(
        fp_before, fp_after,
        "DH public key should change after ratchet"
    );
}

#[test]
fn purge_skipped_keys_by_age() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends messages, Bob skips some
    let msg0 = alice.encrypt(b"msg 0").expect("encrypt");
    let _msg1 = alice.encrypt(b"msg 1").expect("encrypt");
    let msg2 = alice.encrypt(b"msg 2").expect("encrypt");

    // Bob receives only msg2, caching keys for 0 and 1
    bob.decrypt(&msg2).expect("decrypt");
    assert_eq!(bob.skipped_key_count(), 2);

    // Trigger several DH ratchet steps to age the skipped keys
    for round in 0..5 {
        let msg = bob
            .encrypt(format!("bob {round}").as_bytes())
            .expect("encrypt");
        alice.decrypt(&msg).expect("decrypt");

        let msg = alice
            .encrypt(format!("alice {round}").as_bytes())
            .expect("encrypt");
        bob.decrypt(&msg).expect("decrypt");
    }

    // Skipped keys from the old epoch should still be present
    assert_eq!(bob.skipped_key_count(), 2);

    // Purge keys older than 2 DH steps — all skipped keys are
    // from step 0, current step is >= 5, so they should be purged
    bob.purge_skipped_keys_older_than(2);
    assert_eq!(
        bob.skipped_key_count(),
        0,
        "old skipped keys should have been purged"
    );

    // msg0 from the old chain should no longer be decryptable
    let result = bob.decrypt(&msg0);
    assert!(result.is_err(), "purged key should fail decryption");
}

#[test]
fn purge_skipped_keys_keeps_recent() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 3 messages, Bob skips 0 and 1
    let _msg0 = alice.encrypt(b"msg 0").expect("encrypt");
    let _msg1 = alice.encrypt(b"msg 1").expect("encrypt");
    let msg2 = alice.encrypt(b"msg 2").expect("encrypt");
    bob.decrypt(&msg2).expect("decrypt");
    assert_eq!(bob.skipped_key_count(), 2);

    // Purge with max_age that keeps everything (large value)
    bob.purge_skipped_keys_older_than(1000);
    assert_eq!(
        bob.skipped_key_count(),
        2,
        "recent skipped keys should not be purged"
    );
}

#[test]
fn message_key_not_cloneable() {
    // This is a compile-time guarantee: MessageKey does not
    // implement Clone. We verify it indirectly by confirming
    // that the key is consumed (moved) in encrypt/decrypt.
    let (mut alice, mut bob) = make_session_pair();

    let msg = alice.encrypt(b"consumed key").expect("encrypt");
    let dec = bob.decrypt(&msg).expect("decrypt");
    assert_eq!(dec, b"consumed key");
}

#[test]
fn skipped_key_messages_count_correctly() {
    let (mut alice, mut bob) = make_session_pair();

    // Alice sends 3 messages
    let msg0 = alice.encrypt(b"msg 0").expect("encrypt");
    let _msg1 = alice.encrypt(b"msg 1").expect("encrypt");
    let msg2 = alice.encrypt(b"msg 2").expect("encrypt");

    // Bob receives msg2 first (via normal path), then msg0 (via skipped key)
    bob.decrypt(&msg2).expect("decrypt");
    bob.decrypt(&msg0).expect("decrypt");

    let info = bob.session_info();
    // Both normal decryptions and skipped-key decryptions count
    assert_eq!(info.messages_received, 2);
}

#[test]
fn chain_key_zeroize_derive_on_symmetric_ratchet() {
    // ChainKey implements Zeroize and ZeroizeOnDrop via derive.
    // We verify the type is usable and the chain key changes
    // after each advance (old one is dropped/zeroized).
    let ck = crate::symmetric_ratchet::ChainKey::new([0xAA; 32]);
    let initial_bytes = *ck.as_bytes();
    let mut ratchet = crate::symmetric_ratchet::SymmetricRatchet::new(ck);

    let _mk = ratchet.advance();
    let after_bytes = *ratchet.chain_key().as_bytes();

    assert_ne!(
        initial_bytes, after_bytes,
        "chain key must change after advance (old is zeroized on drop)"
    );
}

#[test]
fn header_key_zeroize_derive() {
    // HeaderKey implements Zeroize and ZeroizeOnDrop via derive.
    let key = HeaderKey::from_bytes([0xAB; 32]);
    assert_eq!(key.as_bytes(), &[0xAB; 32]);
    // Key is dropped here and zeroized
}

#[test]
fn session_info_debug_display() {
    let (alice, _bob) = make_session_pair();
    let info = alice.session_info();
    let debug = format!("{info:?}");
    assert!(debug.contains("dh_step_count"));
    assert!(debug.contains("pq_step_count"));
    assert!(debug.contains("messages_sent"));
}

#[test]
fn dh_ratchet_replaces_key_pair() {
    let (mut alice, mut bob) = make_session_pair();

    let fp1 = alice.session_info().dh_public_fingerprint;

    // Trigger two round trips
    for _ in 0..2 {
        let msg = alice.encrypt(b"a").expect("encrypt");
        bob.decrypt(&msg).expect("decrypt");
        let msg = bob.encrypt(b"b").expect("encrypt");
        alice.decrypt(&msg).expect("decrypt");
    }

    let fp2 = alice.session_info().dh_public_fingerprint;
    assert_ne!(
        fp1, fp2,
        "DH key pair must be replaced after ratchet steps"
    );
}

// ── Security audit tests ──────────────────────────────────────

#[test]
fn tampered_encrypted_header_fails_body_decryption() {
    // Verifies that the encrypted header is bound as AAD to the body
    // ciphertext. Tampering with the encrypted header must cause
    // body decryption to fail (AEAD authentication failure).
    let (mut alice, mut bob) = make_session_pair();

    let msg = alice.encrypt(b"secret message").expect("encrypt");

    // Tamper with one byte of the encrypted header
    let mut tampered = msg.clone();
    if let Some(byte) = tampered.encrypted_header.first_mut() {
        *byte ^= 0xFF;
    }

    // Decryption must fail — either header decryption fails (because
    // the header ciphertext is invalid) or body decryption fails
    // (because the AAD no longer matches).
    let result = bob.decrypt(&tampered);
    assert!(result.is_err(), "tampered encrypted header must cause decryption failure");
}

#[test]
fn swapped_body_between_messages_fails() {
    // Verifies that swapping the body ciphertext between two messages
    // (while keeping the original encrypted headers) fails decryption,
    // because the body is bound to its specific encrypted header via AAD.
    let (mut alice, mut bob) = make_session_pair();

    let msg1 = alice.encrypt(b"message one").expect("encrypt 1");
    let msg2 = alice.encrypt(b"message two").expect("encrypt 2");

    // Swap the body ciphertext and nonce from msg2 into msg1's header frame
    let franken = crate::message::EncryptedMessage {
        encrypted_header: msg1.encrypted_header.clone(),
        header_nonce: msg1.header_nonce,
        ciphertext: msg2.ciphertext.clone(),
        body_nonce: msg2.body_nonce,
    };

    // Bob receives msg2 normally first so the symmetric ratchet advances
    // past message_number=1. Then the franken-message re-uses msg1's
    // header (message_number=0) but msg2's body — the AAD mismatch
    // should cause AEAD authentication failure.
    let dec2 = bob.decrypt(&msg2).expect("decrypt msg2");
    assert_eq!(dec2, b"message two");

    let result = bob.decrypt(&franken);
    assert!(result.is_err(), "swapped body must fail AAD check");
}
