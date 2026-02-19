//! E2E encryption round-trip integration tests.
//!
//! Exercises the full pirc-crypto API through real session lifecycles:
//! key exchange, triple ratchet sessions, forward secrecy, E2E encrypted
//! private messages via server, key storage, and edge cases.
//!
//! Test modules are organized by scenario category:
//! - `key_exchange` — X3DH key exchange and pre-key bundle round-trips
//! - `triple_ratchet` — triple ratchet session lifecycle and counters
//! - `forward_secrecy` — replay resistance and key rotation
//! - `e2e_server` — E2E encrypted messages relayed through a server
//! - `key_storage` — key store persistence, rotation, and one-time pre-keys
//! - `edge_cases` — boundary conditions, serialization, and error paths

mod e2e_server;
mod edge_cases;
mod forward_secrecy;
mod key_exchange;
mod key_storage;
mod triple_ratchet;

use pirc_crypto::identity::IdentityKeyPair;
use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::prekey::{KemPreKey, OneTimePreKey, PreKeyBundle, SignedPreKey};
use pirc_crypto::protocol::{self, KeyExchangeMessage};
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_crypto::x3dh;
use pirc_protocol::{Command, Message, PircSubcommand};

// =========================================================================
// Helpers
// =========================================================================

/// Generate a full key set for a user: identity + pre-keys.
pub struct UserKeys {
    pub identity: IdentityKeyPair,
    pub signed_pre_key: SignedPreKey,
    pub kem_pre_key: KemPreKey,
    pub one_time_pre_key: OneTimePreKey,
}

impl UserKeys {
    pub fn generate() -> Self {
        let identity = IdentityKeyPair::generate();
        let signed_pre_key =
            SignedPreKey::generate(1, &identity, 1_700_000_000).expect("spk gen");
        let kem_pre_key = KemPreKey::generate(2, &identity).expect("kpk gen");
        let one_time_pre_key = OneTimePreKey::generate(3);
        Self {
            identity,
            signed_pre_key,
            kem_pre_key,
            one_time_pre_key,
        }
    }

    pub fn public_bundle(&self) -> PreKeyBundle {
        PreKeyBundle::new(
            self.identity.public_identity(),
            self.signed_pre_key.to_public(),
            self.kem_pre_key.to_public(),
            Some(self.one_time_pre_key.to_public()),
        )
    }
}

/// Perform X3DH key exchange and establish triple ratchet sessions for
/// both Alice (sender) and Bob (receiver).
pub fn establish_session(
    alice: &UserKeys,
    bob: &UserKeys,
) -> (TripleRatchetSession, TripleRatchetSession) {
    let bob_bundle = bob.public_bundle();
    bob_bundle.validate().expect("bob bundle validates");

    // Alice performs sender side of X3DH
    let (sender_result, init_msg) =
        x3dh::x3dh_sender(&alice.identity, &bob_bundle).expect("x3dh sender");

    // Bob performs receiver side of X3DH
    let one_time = if init_msg.used_one_time_pre_key_id().is_some() {
        Some(&bob.one_time_pre_key)
    } else {
        None
    };
    let receiver_result = x3dh::x3dh_receiver(
        &bob.identity,
        &bob.signed_pre_key,
        &bob.kem_pre_key,
        one_time,
        &init_msg,
    )
    .expect("x3dh receiver");

    assert_eq!(
        sender_result.shared_secret(),
        receiver_result.shared_secret(),
        "shared secrets must match"
    );

    // Establish triple ratchet sessions
    let alice_session = TripleRatchetSession::init_sender(
        sender_result.shared_secret(),
        bob.signed_pre_key.public_key(),
        bob.kem_pre_key.to_public().public_key(),
    )
    .expect("alice session init");

    let bob_session = TripleRatchetSession::init_receiver(
        receiver_result.shared_secret(),
        x25519::KeyPair::from_secret_bytes(bob.signed_pre_key.key_pair().secret_key().to_bytes()),
        KemKeyPair::from_bytes(&bob.kem_pre_key.kem_pair().to_bytes()).expect("kem pair clone"),
    )
    .expect("bob session init");

    (alice_session, bob_session)
}

/// Build a `PIRC KEYEXCHANGE <target>` request-bundle message.
pub fn request_bundle_msg(target: &str) -> Message {
    let ke_msg = KeyExchangeMessage::RequestBundle;
    let encoded = protocol::encode_for_wire(&ke_msg.to_bytes());
    Message::new(
        Command::Pirc(PircSubcommand::KeyExchange),
        vec![target.to_owned(), encoded],
    )
}

/// Build a `PIRC ENCRYPTED <target> <base64-payload>` message.
pub fn encrypted_msg(target: &str, encrypted: &EncryptedMessage) -> Message {
    let encoded = protocol::encode_for_wire(&encrypted.to_bytes());
    Message::new(
        Command::Pirc(PircSubcommand::Encrypted),
        vec![target.to_owned(), encoded],
    )
}

/// Build a `PIRC KEYEXCHANGE-COMPLETE <target>` message.
pub fn key_exchange_complete_msg(target: &str) -> Message {
    Message::new(
        Command::Pirc(PircSubcommand::KeyExchangeComplete),
        vec![target.to_owned()],
    )
}

/// Build a `PIRC FINGERPRINT <target> <fingerprint>` message.
pub fn fingerprint_msg(target: &str, fingerprint: &[u8; 32]) -> Message {
    let encoded = protocol::encode_for_wire(fingerprint);
    Message::new(
        Command::Pirc(PircSubcommand::Fingerprint),
        vec![target.to_owned(), encoded],
    )
}
