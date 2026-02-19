//! Triple Ratchet Session tests: encrypt/decrypt round-trips, bidirectional
//! messaging, same-chain delivery, out-of-order delivery, DH ratchet
//! advancement, PQ ratchet intervals, and session info counters.

use super::{UserKeys, establish_session};

#[tokio::test]
async fn triple_ratchet_encrypt_decrypt_roundtrip() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let plaintext = b"Hello, Bob! This is a secret message.";
    let encrypted = alice_session.encrypt(plaintext).expect("encrypt");
    let decrypted = bob_session.decrypt(&encrypted).expect("decrypt");

    assert_eq!(decrypted, plaintext);
}

#[tokio::test]
async fn triple_ratchet_bidirectional_messages() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Alice -> Bob
    let enc1 = alice_session.encrypt(b"Hello Bob").expect("encrypt 1");
    assert_eq!(bob_session.decrypt(&enc1).expect("decrypt 1"), b"Hello Bob");

    // Bob -> Alice
    let enc2 = bob_session.encrypt(b"Hello Alice").expect("encrypt 2");
    assert_eq!(
        alice_session.decrypt(&enc2).expect("decrypt 2"),
        b"Hello Alice"
    );

    // Alice -> Bob again (DH ratchet advances)
    let enc3 = alice_session.encrypt(b"How are you?").expect("encrypt 3");
    assert_eq!(
        bob_session.decrypt(&enc3).expect("decrypt 3"),
        b"How are you?"
    );
}

#[tokio::test]
async fn triple_ratchet_multiple_messages_in_same_chain() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Multiple messages from Alice without Bob responding (same sending chain)
    let messages = vec![
        b"Message 1".to_vec(),
        b"Message 2".to_vec(),
        b"Message 3".to_vec(),
        b"Message 4".to_vec(),
        b"Message 5".to_vec(),
    ];

    let encrypted: Vec<_> = messages
        .iter()
        .map(|m| alice_session.encrypt(m).expect("encrypt"))
        .collect();

    // Decrypt in order
    for (i, enc) in encrypted.iter().enumerate() {
        let decrypted = bob_session.decrypt(enc).expect("decrypt");
        assert_eq!(decrypted, messages[i]);
    }
}

#[tokio::test]
async fn triple_ratchet_out_of_order_delivery() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Alice sends multiple messages
    let enc1 = alice_session.encrypt(b"First").expect("encrypt 1");
    let enc2 = alice_session.encrypt(b"Second").expect("encrypt 2");
    let enc3 = alice_session.encrypt(b"Third").expect("encrypt 3");

    // Bob receives them out of order
    assert_eq!(bob_session.decrypt(&enc3).expect("decrypt 3"), b"Third");
    assert_eq!(bob_session.decrypt(&enc1).expect("decrypt 1"), b"First");
    assert_eq!(bob_session.decrypt(&enc2).expect("decrypt 2"), b"Second");
}

#[tokio::test]
async fn triple_ratchet_ratchet_step_advances() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let info_before = alice_session.session_info();

    // Send some messages back and forth to trigger DH ratchet
    let enc = alice_session.encrypt(b"msg1").expect("encrypt");
    bob_session.decrypt(&enc).expect("decrypt");

    let enc = bob_session.encrypt(b"reply1").expect("encrypt");
    alice_session.decrypt(&enc).expect("decrypt");

    let info_after = alice_session.session_info();

    assert!(
        info_after.dh_step_count > info_before.dh_step_count,
        "DH ratchet should advance"
    );
    assert_eq!(info_after.messages_sent, 1);
    assert_eq!(info_after.messages_received, 1);
}

#[tokio::test]
async fn triple_ratchet_pq_ratchet_advances_at_interval() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Only Alice has the remote KEM public key initially (set in init_sender).
    // Bob's PQ ratchet gets the remote key when he receives Alice's first PQ
    // step data. To ensure Alice triggers PQ first, set a low interval for
    // Alice and leave Bob at the default (20).
    alice_session.set_pq_interval(3);
    // Bob keeps default interval (20) — he won't trigger PQ before Alice

    let pq_before = alice_session.session_info().pq_step_count;

    // Alternate messages: each round-trip adds 1 DH step to each side.
    // After 3 rounds, Alice's dh_steps_since_pq reaches 3, and her
    // next encrypt triggers the PQ step.
    for i in 0..3 {
        let enc = alice_session
            .encrypt(format!("msg {i}").as_bytes())
            .expect("encrypt");
        bob_session.decrypt(&enc).expect("decrypt");

        let enc = bob_session
            .encrypt(format!("reply {i}").as_bytes())
            .expect("encrypt");
        alice_session.decrypt(&enc).expect("decrypt");
    }

    // Alice's next encrypt should trigger PQ step (dh_steps_since_pq >= 3)
    let enc = alice_session
        .encrypt(b"pq-trigger")
        .expect("encrypt with PQ step");
    bob_session
        .decrypt(&enc)
        .expect("decrypt with PQ data");

    let pq_after = alice_session.session_info().pq_step_count;
    assert!(
        pq_after > pq_before,
        "PQ ratchet should have advanced: before={pq_before}, after={pq_after}"
    );

    // Bob now has Alice's remote KEM public key from the PQ step.
    // Verify continued messaging works after PQ ratchet.
    let enc = bob_session.encrypt(b"after pq").expect("encrypt after pq");
    let dec = alice_session.decrypt(&enc).expect("decrypt after pq");
    assert_eq!(dec, b"after pq");
}

#[tokio::test]
async fn triple_ratchet_session_info_tracks_counters() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Send 5 messages Alice -> Bob
    for _ in 0..5 {
        let enc = alice_session.encrypt(b"hello").expect("encrypt");
        bob_session.decrypt(&enc).expect("decrypt");
    }

    let alice_info = alice_session.session_info();
    let bob_info = bob_session.session_info();

    assert_eq!(alice_info.messages_sent, 5);
    assert_eq!(alice_info.messages_received, 0);
    assert_eq!(bob_info.messages_sent, 0);
    assert_eq!(bob_info.messages_received, 5);
}
