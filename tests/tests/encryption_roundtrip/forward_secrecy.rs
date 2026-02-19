//! Forward Secrecy tests: replay resistance, unique ciphertexts for
//! identical plaintexts, and DH key rotation after ratchet steps.

use super::{UserKeys, establish_session};

#[tokio::test]
async fn forward_secrecy_old_ciphertext_not_replayable() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Alice sends a message
    let enc1 = alice_session.encrypt(b"secret message").expect("encrypt");
    bob_session.decrypt(&enc1).expect("decrypt first time");

    // Attempting to decrypt the same ciphertext again should fail
    // (message key already consumed)
    let result = bob_session.decrypt(&enc1);
    assert!(result.is_err(), "replaying same ciphertext should fail");
}

#[tokio::test]
async fn forward_secrecy_ratchet_produces_unique_ciphertexts() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, _bob_session) = establish_session(&alice_keys, &bob_keys);

    // Encrypt the same plaintext twice
    let enc1 = alice_session.encrypt(b"same message").expect("encrypt 1");
    let enc2 = alice_session.encrypt(b"same message").expect("encrypt 2");

    // Ciphertexts should differ (different message keys each time)
    assert_ne!(enc1.ciphertext, enc2.ciphertext);
}

#[tokio::test]
async fn forward_secrecy_new_keys_after_ratchet_step() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Capture initial DH fingerprint
    let fp_before = alice_session.session_info().dh_public_fingerprint;

    // Exchange messages to trigger ratchet
    let enc = alice_session.encrypt(b"trigger").expect("encrypt");
    bob_session.decrypt(&enc).expect("decrypt");
    let enc = bob_session.encrypt(b"response").expect("encrypt");
    alice_session.decrypt(&enc).expect("decrypt");

    // DH public key should have rotated
    let fp_after = alice_session.session_info().dh_public_fingerprint;
    assert_ne!(fp_before, fp_after, "DH public key should rotate after ratchet step");
}
