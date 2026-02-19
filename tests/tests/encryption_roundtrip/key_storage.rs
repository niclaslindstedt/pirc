//! Key Storage tests: create/save/open round-trip, wrong passphrase,
//! encrypt/decrypt after reload, pre-key rotation, one-time pre-key
//! management, and serialized bytes round-trip.

use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::key_storage::KeyStore;
use pirc_crypto::prekey::{OneTimePreKey, PreKeyBundle};
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_crypto::x3dh;

#[tokio::test]
async fn key_store_create_save_open_roundtrip() {
    let store = KeyStore::create().expect("create");
    let original_pub = store.identity().public_identity();

    let encrypted = store.save(b"test passphrase").expect("save");
    let restored = KeyStore::open(&encrypted, b"test passphrase").expect("open");

    assert_eq!(original_pub, restored.identity().public_identity());
}

#[tokio::test]
async fn key_store_wrong_passphrase_fails() {
    let store = KeyStore::create().expect("create");
    let encrypted = store.save(b"correct password").expect("save");

    let result = KeyStore::open(&encrypted, b"wrong password");
    assert!(result.is_err());
}

#[tokio::test]
async fn key_store_encrypt_decrypt_message_after_reload() {
    // Create key stores for Alice and Bob
    let mut alice_store = KeyStore::create().expect("create alice store");
    alice_store.generate_one_time_pre_keys(5);
    let mut bob_store = KeyStore::create().expect("create bob store");
    bob_store.generate_one_time_pre_keys(5);

    // Save and reload both stores
    let alice_encrypted = alice_store.save(b"alice_pass").expect("save alice");
    let bob_encrypted = bob_store.save(b"bob_pass").expect("save bob");

    let alice_restored = KeyStore::open(&alice_encrypted, b"alice_pass").expect("open alice");
    let bob_restored = KeyStore::open(&bob_encrypted, b"bob_pass").expect("open bob");

    // Get components for key exchange
    let (bob_identity, bob_spk, bob_kpk, bob_otpks) =
        bob_restored.into_parts().expect("bob parts");

    let bob_bundle = PreKeyBundle::new(
        bob_identity.public_identity(),
        bob_spk.to_public(),
        bob_kpk.to_public(),
        bob_otpks.first().map(OneTimePreKey::to_public),
    );
    bob_bundle.validate().expect("bob bundle validates");

    // Perform X3DH with restored keys
    let (sender_result, init_msg) =
        x3dh::x3dh_sender(alice_restored.identity(), &bob_bundle).expect("x3dh sender");

    let one_time = if init_msg.used_one_time_pre_key_id().is_some() {
        bob_otpks.first()
    } else {
        None
    };
    let receiver_result = x3dh::x3dh_receiver(
        &bob_identity,
        &bob_spk,
        &bob_kpk,
        one_time,
        &init_msg,
    )
    .expect("x3dh receiver");

    assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());

    // Establish sessions
    let mut alice_session = TripleRatchetSession::init_sender(
        sender_result.shared_secret(),
        bob_spk.public_key(),
        bob_kpk.to_public().public_key(),
    )
    .expect("alice session");

    let mut bob_session = TripleRatchetSession::init_receiver(
        receiver_result.shared_secret(),
        x25519::KeyPair::from_secret_bytes(bob_spk.key_pair().secret_key().to_bytes()),
        KemKeyPair::from_bytes(&bob_kpk.kem_pair().to_bytes()).expect("kem clone"),
    )
    .expect("bob session");

    // Verify encryption works with restored keys
    let enc = alice_session.encrypt(b"message after reload").expect("encrypt");
    let dec = bob_session.decrypt(&enc).expect("decrypt");
    assert_eq!(dec, b"message after reload");
}

#[tokio::test]
async fn key_store_prekey_rotation() {
    let mut store = KeyStore::create().expect("create");
    let original_spk_id = store.public_bundle().expect("bundle").signed_pre_key().id();
    let original_kpk_id = store.public_bundle().expect("bundle").kem_pre_key().id();

    // Rotate keys
    let new_spk = store.rotate_signed_pre_key(1000).expect("rotate spk");
    let new_kpk = store.rotate_kem_pre_key(2000).expect("rotate kpk");

    // New keys are current
    assert_ne!(new_spk.id(), original_spk_id);
    assert_ne!(new_kpk.id(), original_kpk_id);

    // Old keys still retained (for in-flight key exchanges)
    assert!(store.find_signed_pre_key(original_spk_id).is_some());
    assert!(store.find_kem_pre_key(original_kpk_id).is_some());

    // Cleanup expired keys
    let removed = store.cleanup_expired_pre_keys(500);
    assert!(removed > 0, "should have removed expired keys");

    // After cleanup, old keys gone
    assert!(store.find_signed_pre_key(original_spk_id).is_none());
}

#[tokio::test]
async fn key_store_one_time_prekey_management() {
    let mut store = KeyStore::create().expect("create");
    assert_eq!(store.one_time_pre_key_count(), 0);

    // Generate one-time pre-keys
    let pks = store.generate_one_time_pre_keys(10);
    assert_eq!(pks.len(), 10);
    assert_eq!(store.one_time_pre_key_count(), 10);

    // Consume some
    store.consume_one_time_pre_key(pks[0].id());
    assert_eq!(store.one_time_pre_key_count(), 9);

    // Replenish
    let new_pks = store.replenish_one_time_pre_keys(15);
    assert_eq!(new_pks.len(), 6);
    assert_eq!(store.one_time_pre_key_count(), 15);
}

#[tokio::test]
async fn key_store_serialized_bytes_roundtrip() {
    let store = KeyStore::create().expect("create");
    let encrypted = store.save(b"passphrase").expect("save");

    // Serialize to bytes and restore
    let bytes = encrypted.to_bytes();
    let restored_enc =
        pirc_crypto::key_storage::EncryptedKeyStore::from_bytes(&bytes).expect("deserialize");
    let restored = KeyStore::open(&restored_enc, b"passphrase").expect("open");

    assert_eq!(
        store.identity().public_identity(),
        restored.identity().public_identity()
    );
}
