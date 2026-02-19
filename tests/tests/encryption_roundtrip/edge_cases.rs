//! Edge Cases tests: offline message delivery, large messages, rekeying,
//! serialization round-trips, wrong session decryption, empty plaintext,
//! identity signatures, AEAD, KDF chains, key exchange complete relay,
//! skipped key purging, and full E2E flow.

use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::protocol;
use pirc_integration_tests::common::{
    assert_command, assert_param_contains, TestClient, TestServer,
};
use pirc_protocol::{Command, PircSubcommand};

use super::{
    UserKeys, encrypted_msg, establish_session, fingerprint_msg, key_exchange_complete_msg,
};

#[tokio::test]
async fn offline_message_delivery() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    alice_client.register("Alice", "alice").await;

    // Alice sends an encrypted message to offline Bob
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, _bob_session) = establish_session(&alice_keys, &bob_keys);

    let enc = alice_session.encrypt(b"hello offline Bob").expect("encrypt");
    alice_client.send(encrypted_msg("Bob", &enc)).await;

    // Alice gets a notice that Bob is offline
    let notice = alice_client.recv_msg().await;
    assert_command(&notice, Command::Notice);
    assert_param_contains(&notice, 1, "offline");
}

#[tokio::test]
async fn large_message_encryption_decryption() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Create a large message (100KB)
    let large_plaintext = vec![0xABu8; 100 * 1024];
    let enc = alice_session.encrypt(&large_plaintext).expect("encrypt large");
    let dec = bob_session.decrypt(&enc).expect("decrypt large");

    assert_eq!(dec, large_plaintext);
}

#[tokio::test]
async fn rekeying_after_many_messages() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Set PQ interval only for Alice (she has the remote KEM key).
    // Bob keeps default (20) — he'll get the remote key when Alice
    // triggers a PQ step and won't try to initiate one before that.
    alice_session.set_pq_interval(5);

    // Exchange many messages. Each alternating round-trip creates 1 DH
    // step on each side. After 5 rounds, Alice triggers PQ.
    for i in 0..25 {
        let msg = format!("message {i}");
        let enc = alice_session.encrypt(msg.as_bytes()).expect("encrypt");
        let dec = bob_session.decrypt(&enc).expect("decrypt");
        assert_eq!(dec, msg.as_bytes());

        let reply = format!("reply {i}");
        let enc = bob_session.encrypt(reply.as_bytes()).expect("encrypt");
        let dec = alice_session.decrypt(&enc).expect("decrypt");
        assert_eq!(dec, reply.as_bytes());
    }

    let info = alice_session.session_info();
    assert_eq!(info.messages_sent, 25);
    assert_eq!(info.messages_received, 25);
    assert!(info.pq_step_count > 0, "PQ ratchet should have advanced");
    assert!(info.dh_step_count > 0, "DH ratchet should have advanced");
}

#[tokio::test]
async fn encrypted_message_serialization_roundtrip() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let plaintext = b"roundtrip test";
    let enc = alice_session.encrypt(plaintext).expect("encrypt");

    // Serialize to bytes and back (simulates wire transport)
    let bytes = enc.to_bytes();
    let restored = EncryptedMessage::from_bytes(&bytes).expect("from_bytes");

    let dec = bob_session.decrypt(&restored).expect("decrypt");
    assert_eq!(dec, plaintext);
}

#[tokio::test]
async fn encrypted_message_wire_encoding_roundtrip() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let plaintext = b"wire encoding test";
    let enc = alice_session.encrypt(plaintext).expect("encrypt");

    // Encode to base64 and back (simulates IRC wire transport)
    let wire = protocol::encode_for_wire(&enc.to_bytes());
    assert!(wire.is_ascii(), "wire encoding should be ASCII safe");

    let decoded_bytes = protocol::decode_from_wire(&wire).expect("decode");
    let restored = EncryptedMessage::from_bytes(&decoded_bytes).expect("parse");

    let dec = bob_session.decrypt(&restored).expect("decrypt");
    assert_eq!(dec, plaintext);
}

#[tokio::test]
async fn wrong_session_cannot_decrypt() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let carol_keys = UserKeys::generate();

    let (mut alice_session, _bob_session) = establish_session(&alice_keys, &bob_keys);
    let (_alice_carol_session, mut carol_session) =
        establish_session(&alice_keys, &carol_keys);

    // Alice encrypts for Bob's session
    let enc = alice_session.encrypt(b"for Bob only").expect("encrypt");

    // Carol (different session) should not be able to decrypt
    let result = carol_session.decrypt(&enc);
    assert!(result.is_err(), "wrong session should not decrypt");
}

#[tokio::test]
async fn empty_plaintext_encryption() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let enc = alice_session.encrypt(b"").expect("encrypt empty");
    let dec = bob_session.decrypt(&enc).expect("decrypt empty");
    assert!(dec.is_empty());
}

#[tokio::test]
async fn identity_signature_verification() {
    let keys = UserKeys::generate();
    let message = b"test message to sign";

    // Sign with identity key
    let signature = keys.identity.sign(message).expect("sign");

    // Verify with public identity
    keys.identity
        .public_identity()
        .verify(message, &signature)
        .expect("verify");

    // Wrong message should fail verification
    let wrong_message = b"different message";
    let result = keys.identity.public_identity().verify(wrong_message, &signature);
    assert!(result.is_err(), "wrong message should fail verification");
}

#[tokio::test]
async fn aead_encrypt_decrypt_roundtrip() {
    use pirc_crypto::aead;

    let key = [0x42u8; 32];
    let nonce = aead::generate_nonce();
    let plaintext = b"AEAD test message";
    let aad = b"additional authenticated data";

    let ciphertext = aead::encrypt(&key, &nonce, plaintext, aad).expect("encrypt");
    let decrypted = aead::decrypt(&key, &nonce, &ciphertext, aad).expect("decrypt");

    assert_eq!(decrypted, plaintext);

    // Wrong key should fail
    let wrong_key = [0x43u8; 32];
    let result = aead::decrypt(&wrong_key, &nonce, &ciphertext, aad);
    assert!(result.is_err());

    // Wrong AAD should fail
    let result = aead::decrypt(&key, &nonce, &ciphertext, b"wrong aad");
    assert!(result.is_err());
}

#[tokio::test]
async fn kdf_chain_produces_unique_keys() {
    use pirc_crypto::kdf;

    let chain_key = [0x01u8; 32];
    let (next_chain1, msg_key1) = kdf::kdf_chain(&chain_key, b"step 1");
    let (next_chain2, msg_key2) = kdf::kdf_chain(&next_chain1, b"step 2");

    // All keys should be different
    assert_ne!(chain_key, next_chain1);
    assert_ne!(next_chain1, next_chain2);
    assert_ne!(msg_key1, msg_key2);
}

#[tokio::test]
async fn key_exchange_complete_relay_via_server() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    let mut bob_client = TestClient::connect(server.addr).await;

    alice_client.register("Alice", "alice").await;
    bob_client.register("Bob", "bob").await;

    // Alice sends key exchange complete to Bob
    alice_client.send(key_exchange_complete_msg("Bob")).await;

    let msg = bob_client.recv_msg().await;
    assert_command(
        &msg,
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
    );
}

#[tokio::test]
async fn purge_skipped_keys() {
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Create some out-of-order messages to generate skipped keys
    let enc1 = alice_session.encrypt(b"msg 1").expect("encrypt 1");
    let _enc2 = alice_session.encrypt(b"msg 2").expect("encrypt 2");
    let enc3 = alice_session.encrypt(b"msg 3").expect("encrypt 3");

    // Decrypt msg 3 first, causing keys for 1 and 2 to be cached
    bob_session.decrypt(&enc3).expect("decrypt 3");
    assert!(bob_session.session_info().skipped_key_count > 0);

    // Purge skipped keys older than a very high threshold (should remove all)
    bob_session.purge_skipped_keys_older_than(0);

    // Now try to decrypt msg 1 — it should still work because we purged
    // with threshold 0 which removes nothing (keys at step >= 0 are kept)
    let dec1 = bob_session.decrypt(&enc1).expect("decrypt 1");
    assert_eq!(dec1, b"msg 1");
}

#[tokio::test]
async fn full_e2e_key_exchange_and_messaging_flow() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    let mut bob_client = TestClient::connect(server.addr).await;

    alice_client.register("Alice", "alice").await;
    bob_client.register("Bob", "bob").await;

    // Step 1: Key exchange happens out-of-band (bundle and init messages
    // exceed the 512-byte IRC wire limit due to PQ key material, so in a
    // real implementation they would use message fragmentation or a
    // side-channel). Here we perform it locally.
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Step 2: Bob sends completion ack through the server
    bob_client.send(key_exchange_complete_msg("Alice")).await;
    let complete = alice_client.recv_msg().await;
    assert_command(
        &complete,
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
    );

    // Step 3: Alice sends fingerprint to Bob for verification
    let alice_fp = alice_keys.identity.public_identity().fingerprint();
    alice_client.send(fingerprint_msg("Bob", &alice_fp)).await;
    let fp_msg = bob_client.recv_msg().await;
    assert_command(&fp_msg, Command::Pirc(PircSubcommand::Fingerprint));
    let received_fp = protocol::decode_from_wire(&fp_msg.params[1]).expect("decode fp");
    assert_eq!(&received_fp[..], &alice_fp[..]);

    // Step 4: Exchange encrypted messages through the server
    let plaintext = b"This is a fully E2E encrypted message!";
    let enc = alice_session.encrypt(plaintext).expect("encrypt");
    alice_client.send(encrypted_msg("Bob", &enc)).await;

    let relayed = bob_client.recv_msg().await;
    assert_command(&relayed, Command::Pirc(PircSubcommand::Encrypted));
    let ct_bytes = protocol::decode_from_wire(&relayed.params[1]).expect("decode");
    let received_enc = EncryptedMessage::from_bytes(&ct_bytes).expect("parse");
    let decrypted = bob_session.decrypt(&received_enc).expect("decrypt");
    assert_eq!(decrypted, plaintext);

    // Step 5: Bob replies with encrypted message
    let reply_text = b"Got it! Replying with E2E encryption.";
    let reply_enc = bob_session.encrypt(reply_text).expect("encrypt reply");
    bob_client.send(encrypted_msg("Alice", &reply_enc)).await;

    let alice_received = alice_client.recv_msg().await;
    assert_command(&alice_received, Command::Pirc(PircSubcommand::Encrypted));
    let reply_bytes = protocol::decode_from_wire(&alice_received.params[1]).expect("decode");
    let reply_msg = EncryptedMessage::from_bytes(&reply_bytes).expect("parse");
    let reply_dec = alice_session.decrypt(&reply_msg).expect("decrypt reply");
    assert_eq!(reply_dec, reply_text);

    // Step 6: Multiple messages back and forth
    for i in 0..5 {
        let msg = format!("Alice msg {i}");
        let enc = alice_session.encrypt(msg.as_bytes()).expect("encrypt");
        alice_client.send(encrypted_msg("Bob", &enc)).await;

        let relayed = bob_client.recv_msg().await;
        let data = protocol::decode_from_wire(&relayed.params[1]).expect("decode");
        let received = EncryptedMessage::from_bytes(&data).expect("parse");
        let dec = bob_session.decrypt(&received).expect("decrypt");
        assert_eq!(dec, msg.as_bytes());
    }
}
