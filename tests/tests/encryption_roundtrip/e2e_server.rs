//! E2E Encrypted Private Messages via Server tests: encrypted message relay,
//! server opacity, multiple messages, and X3DH init message serialization.

use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::protocol::{self, KeyExchangeMessage};
use pirc_crypto::x3dh::{self, X3DHInitMessage};
use pirc_integration_tests::common::{assert_command, TestClient, TestServer};
use pirc_protocol::{Command, PircSubcommand};

use super::{UserKeys, encrypted_msg, establish_session};

#[tokio::test]
async fn e2e_encrypted_message_relayed_through_server() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    let mut bob_client = TestClient::connect(server.addr).await;

    alice_client.register("Alice", "alice").await;
    bob_client.register("Bob", "bob").await;

    // Establish crypto sessions locally (in a real client this goes through the server)
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    // Alice encrypts a message and sends it through the server
    let plaintext = b"This is a top-secret E2E message!";
    let enc = alice_session.encrypt(plaintext).expect("encrypt");
    alice_client.send(encrypted_msg("Bob", &enc)).await;

    // Bob receives the encrypted message from the server
    let relayed = bob_client.recv_msg().await;
    assert_command(&relayed, Command::Pirc(PircSubcommand::Encrypted));

    // Decode and decrypt
    let ciphertext_bytes =
        protocol::decode_from_wire(&relayed.params[1]).expect("decode ciphertext");
    let received_enc = EncryptedMessage::from_bytes(&ciphertext_bytes).expect("parse encrypted");
    let decrypted = bob_session.decrypt(&received_enc).expect("decrypt");

    assert_eq!(decrypted, plaintext);
}

#[tokio::test]
async fn server_cannot_read_encrypted_content() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    let mut bob_client = TestClient::connect(server.addr).await;

    alice_client.register("Alice", "alice").await;
    bob_client.register("Bob", "bob").await;

    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let plaintext = b"Server should not see this!";
    let enc = alice_session.encrypt(plaintext).expect("encrypt");
    let wire_payload = protocol::encode_for_wire(&enc.to_bytes());

    // The wire payload is base64 — it should NOT contain the plaintext
    assert!(
        !wire_payload.contains("Server should not see this"),
        "plaintext should not appear in wire payload"
    );

    // Send through server
    alice_client.send(encrypted_msg("Bob", &enc)).await;
    let relayed = bob_client.recv_msg().await;

    // The relayed message params should not contain plaintext
    for param in &relayed.params {
        assert!(
            !param.contains("Server should not see this"),
            "plaintext should not appear in relayed message"
        );
    }

    // But Bob can still decrypt
    let ciphertext_bytes =
        protocol::decode_from_wire(&relayed.params[1]).expect("decode");
    let received_enc = EncryptedMessage::from_bytes(&ciphertext_bytes).expect("parse");
    let decrypted = bob_session.decrypt(&received_enc).expect("decrypt");
    assert_eq!(decrypted, plaintext);
}

#[tokio::test]
async fn e2e_multiple_messages_through_server() {
    let server = TestServer::start().await;
    let mut alice_client = TestClient::connect(server.addr).await;
    let mut bob_client = TestClient::connect(server.addr).await;

    alice_client.register("Alice", "alice").await;
    bob_client.register("Bob", "bob").await;

    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let (mut alice_session, mut bob_session) = establish_session(&alice_keys, &bob_keys);

    let messages = vec!["First message", "Second message", "Third message"];

    for msg_text in &messages {
        let enc = alice_session
            .encrypt(msg_text.as_bytes())
            .expect("encrypt");
        alice_client.send(encrypted_msg("Bob", &enc)).await;
    }

    // Bob receives and decrypts all messages
    for msg_text in &messages {
        let relayed = bob_client.recv_msg().await;
        let data = protocol::decode_from_wire(&relayed.params[1]).expect("decode");
        let received_enc = EncryptedMessage::from_bytes(&data).expect("parse");
        let decrypted = bob_session.decrypt(&received_enc).expect("decrypt");
        assert_eq!(decrypted, msg_text.as_bytes());
    }
}

#[tokio::test]
async fn x3dh_init_message_serialization_roundtrip() {
    // X3DH init messages contain PQ KEM ciphertext and exceed the 512-byte
    // IRC wire limit. This test verifies the full init message lifecycle
    // at the crypto API level.
    let alice_keys = UserKeys::generate();
    let bob_keys = UserKeys::generate();
    let bob_bundle = bob_keys.public_bundle();

    let (sender_result, init_msg) =
        x3dh::x3dh_sender(&alice_keys.identity, &bob_bundle).expect("x3dh sender");

    // Serialize → wire encode → wire decode → deserialize
    let ke_msg = KeyExchangeMessage::InitMessage(Box::new(
        X3DHInitMessage::from_bytes(&init_msg.to_bytes()).expect("clone"),
    ));
    let wire = protocol::encode_for_wire(&ke_msg.to_bytes());
    let decoded = protocol::decode_from_wire(&wire).expect("decode");
    let restored_ke = KeyExchangeMessage::from_bytes(&decoded).expect("parse");

    let restored_init = match restored_ke {
        KeyExchangeMessage::InitMessage(i) => *i,
        _ => panic!("expected InitMessage"),
    };

    // Verify Bob can complete X3DH with the restored init message
    let otpk = if restored_init.used_one_time_pre_key_id().is_some() {
        Some(&bob_keys.one_time_pre_key)
    } else {
        None
    };
    let receiver_result = x3dh::x3dh_receiver(
        &bob_keys.identity,
        &bob_keys.signed_pre_key,
        &bob_keys.kem_pre_key,
        otpk,
        &restored_init,
    )
    .expect("x3dh receiver");

    assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());
}
