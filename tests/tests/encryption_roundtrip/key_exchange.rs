//! Key Exchange Round-Trip tests: X3DH key exchange with and without
//! one-time pre-keys, pre-key bundle serialization, bundle requests,
//! identity fingerprints, and fingerprint relay via server.

use pirc_crypto::identity::IdentityKeyPair;
use pirc_crypto::prekey::{KemPreKey, PreKeyBundle, SignedPreKey};
use pirc_crypto::protocol::{self, KeyExchangeMessage};
use pirc_crypto::x3dh;
use pirc_integration_tests::common::{
    assert_command, assert_param_contains, TestClient, TestServer,
};
use pirc_protocol::{Command, PircSubcommand};

use super::{UserKeys, fingerprint_msg, request_bundle_msg};

#[tokio::test]
async fn x3dh_key_exchange_produces_matching_shared_secrets() {
    let alice = UserKeys::generate();
    let bob = UserKeys::generate();
    let bob_bundle = bob.public_bundle();
    bob_bundle.validate().expect("bob bundle validates");

    let (sender_result, init_msg) =
        x3dh::x3dh_sender(&alice.identity, &bob_bundle).expect("x3dh sender");

    let receiver_result = x3dh::x3dh_receiver(
        &bob.identity,
        &bob.signed_pre_key,
        &bob.kem_pre_key,
        Some(&bob.one_time_pre_key),
        &init_msg,
    )
    .expect("x3dh receiver");

    assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());
}

#[tokio::test]
async fn x3dh_key_exchange_without_one_time_prekey() {
    let alice_identity = IdentityKeyPair::generate();
    let bob_identity = IdentityKeyPair::generate();
    let bob_spk = SignedPreKey::generate(1, &bob_identity, 1_700_000_000).expect("spk");
    let bob_kpk = KemPreKey::generate(2, &bob_identity).expect("kpk");

    // Bundle with no one-time pre-key
    let bob_bundle = PreKeyBundle::new(
        bob_identity.public_identity(),
        bob_spk.to_public(),
        bob_kpk.to_public(),
        None,
    );
    bob_bundle.validate().expect("bundle validates");

    let (sender_result, init_msg) =
        x3dh::x3dh_sender(&alice_identity, &bob_bundle).expect("x3dh sender");

    assert!(init_msg.used_one_time_pre_key_id().is_none());

    let receiver_result = x3dh::x3dh_receiver(
        &bob_identity,
        &bob_spk,
        &bob_kpk,
        None,
        &init_msg,
    )
    .expect("x3dh receiver");

    assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());
}

#[tokio::test]
async fn prekey_bundle_serialization_and_x3dh_roundtrip() {
    // Pre-key bundles exceed the IRC 512-byte wire limit due to PQ key
    // material, so they cannot be sent as a single IRC message. This
    // test verifies the full bundle lifecycle at the crypto API level:
    // generation → serialization → deserialization → X3DH exchange.
    let bob_keys = UserKeys::generate();
    let bundle = bob_keys.public_bundle();
    bundle.validate().expect("original bundle validates");

    // Serialize to bytes (as would be stored on server)
    let bundle_bytes = bundle.to_bytes();

    // Wrap in a KeyExchangeMessage::Bundle and serialize/deserialize
    let ke_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
    let ke_bytes = ke_msg.to_bytes();
    let restored_ke = KeyExchangeMessage::from_bytes(&ke_bytes).expect("parse ke msg");

    let restored_bundle = match restored_ke {
        KeyExchangeMessage::Bundle(b) => {
            b.validate().expect("restored bundle validates");
            *b
        }
        other => panic!("expected Bundle, got {other:?}"),
    };

    // Perform X3DH with the restored bundle
    let alice_keys = UserKeys::generate();
    let (sender_result, init_msg) =
        x3dh::x3dh_sender(&alice_keys.identity, &restored_bundle).expect("x3dh sender");

    // Bob can complete the exchange
    let otpk = if init_msg.used_one_time_pre_key_id().is_some() {
        Some(&bob_keys.one_time_pre_key)
    } else {
        None
    };
    let receiver_result = x3dh::x3dh_receiver(
        &bob_keys.identity,
        &bob_keys.signed_pre_key,
        &bob_keys.kem_pre_key,
        otpk,
        &init_msg,
    )
    .expect("x3dh receiver");

    assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());

    // Verify the bundle data can also be round-tripped via the PreKeyBundleStore format
    let restored_from_bytes = PreKeyBundle::from_bytes(&bundle_bytes).expect("from_bytes");
    restored_from_bytes.validate().expect("validates after bytes roundtrip");
}

#[tokio::test]
async fn request_bundle_for_nonexistent_user() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Request bundle for a user who never uploaded one
    alice.send(request_bundle_msg("Ghost")).await;
    let reply = alice.recv_msg().await;

    // Server should respond with a notice about missing bundle
    assert_command(&reply, Command::Notice);
    assert_param_contains(&reply, 1, "No pre-key bundle available");
}

#[tokio::test]
async fn identity_fingerprint_comparison() {
    let alice = UserKeys::generate();
    let bob = UserKeys::generate();

    let alice_fp = alice.identity.public_identity().fingerprint();
    let bob_fp = bob.identity.public_identity().fingerprint();

    // Fingerprints should be unique
    assert_ne!(alice_fp, bob_fp);

    // Same identity always produces the same fingerprint
    assert_eq!(
        alice.identity.public_identity().fingerprint(),
        alice_fp
    );
}

#[tokio::test]
async fn fingerprint_relay_via_server() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    let mut bob = TestClient::connect(server.addr).await;

    alice.register("Alice", "alice").await;
    bob.register("Bob", "bob").await;

    let alice_keys = UserKeys::generate();
    let fp = alice_keys.identity.public_identity().fingerprint();

    // Alice sends her fingerprint to Bob
    alice.send(fingerprint_msg("Bob", &fp)).await;

    // Bob should receive the fingerprint message
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Pirc(PircSubcommand::Fingerprint));

    // Decode and verify the fingerprint
    let received_fp = protocol::decode_from_wire(&msg.params[1]).expect("decode fp");
    assert_eq!(received_fp.len(), 32);
    assert_eq!(&received_fp[..], &fp[..]);
}
