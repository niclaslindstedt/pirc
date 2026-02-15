//! Diffie-Hellman ratchet.
//!
//! Manages the X25519 ratchet key pairs and performs DH exchanges on
//! each message turn. Each DH output is fed into the KDF chain to
//! derive new sending and receiving chain keys, providing forward secrecy.

use crate::error::{CryptoError, Result};
use crate::kdf;
use crate::symmetric_ratchet::{ChainKey, MessageKey, SymmetricRatchet};
use crate::x25519::{self, KeyPair, PublicKey};

/// Info string for root key derivation during a DH ratchet step.
const ROOT_KEY_INFO: &[u8] = b"pirc-dh-ratchet-root";

/// A 32-byte root key used to derive new chain keys during DH ratchet steps.
///
/// The root key evolves with each ratchet step and is never reused directly.
/// It is combined with a DH shared secret via HKDF to produce a new root
/// key and a chain key for the next sending or receiving chain.
struct RootKey([u8; kdf::KEY_SIZE]);

impl RootKey {
    fn new(bytes: [u8; kdf::KEY_SIZE]) -> Self {
        Self(bytes)
    }

    fn as_bytes(&self) -> &[u8; kdf::KEY_SIZE] {
        &self.0
    }

    /// Derive a new root key, chain key, and header key from a DH shared
    /// secret.
    ///
    /// Uses HKDF with the current root key as salt and the DH output as
    /// input key material. Produces 96 bytes: the first 32 become the new
    /// root key, the next 32 become a chain key for a symmetric ratchet,
    /// and the last 32 become a header key.
    fn kdf(
        &self,
        dh_output: &x25519::SharedSecret,
    ) -> Result<(RootKey, ChainKey, [u8; kdf::KEY_SIZE])> {
        let output = kdf::derive_key(self.as_bytes(), dh_output.as_bytes(), ROOT_KEY_INFO, 96)?;
        let mut new_root = [0u8; kdf::KEY_SIZE];
        let mut chain = [0u8; kdf::KEY_SIZE];
        let mut header = [0u8; kdf::KEY_SIZE];
        new_root.copy_from_slice(&output[..32]);
        chain.copy_from_slice(&output[32..64]);
        header.copy_from_slice(&output[64..96]);
        Ok((RootKey::new(new_root), ChainKey::new(chain), header))
    }
}

impl Drop for RootKey {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

/// The DH ratchet state for one party in a session.
///
/// Manages X25519 key pairs and uses DH shared secrets to periodically
/// reset the sending and receiving symmetric ratchet chains. This provides
/// break-in recovery: after a ratchet step, old keys cannot derive new ones.
pub struct DhRatchetState {
    /// Our current DH key pair.
    dh_pair: KeyPair,
    /// The remote party's current public key.
    remote_public: Option<PublicKey>,
    /// Root key used to derive chain keys on each DH ratchet step.
    root_key: RootKey,
    /// Sending chain — derives per-message encryption keys.
    sending_chain: Option<SymmetricRatchet>,
    /// Receiving chain — derives per-message decryption keys.
    receiving_chain: Option<SymmetricRatchet>,
}

impl DhRatchetState {
    /// Initialize as the session sender (Alice).
    ///
    /// Alice knows Bob's public key and performs an initial DH to set up
    /// her sending chain. The receiving chain is not yet available until
    /// Bob sends his first message with a new public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if the initial DH produces an invalid shared
    /// secret or the root key derivation fails.
    /// Returns `(Self, header_key)` where `header_key` is derived from
    /// the initial DH root-KDF step.  The caller uses this as the
    /// sender's *next* sending header key.
    pub fn init_sender(
        root_key: [u8; kdf::KEY_SIZE],
        remote_public: PublicKey,
    ) -> Result<(Self, [u8; kdf::KEY_SIZE])> {
        let dh_pair = KeyPair::generate();
        let dh_output = x25519::diffie_hellman_keypair(&dh_pair, &remote_public)?;

        let rk = RootKey::new(root_key);
        let (new_rk, sending_ck, header_key) = rk.kdf(&dh_output)?;

        Ok((
            Self {
                dh_pair,
                remote_public: Some(remote_public),
                root_key: new_rk,
                sending_chain: Some(SymmetricRatchet::new(sending_ck)),
                receiving_chain: None,
            },
            header_key,
        ))
    }

    /// Initialize as the session receiver (Bob).
    ///
    /// Bob starts with his own key pair (whose public key was shared with
    /// Alice out-of-band). He does not yet have a sending or receiving
    /// chain — these are created on the first ratchet step when Alice's
    /// message arrives.
    #[must_use]
    pub fn init_receiver(root_key: [u8; kdf::KEY_SIZE], dh_pair: KeyPair) -> Self {
        Self {
            dh_pair,
            remote_public: None,
            root_key: RootKey::new(root_key),
            sending_chain: None,
            receiving_chain: None,
        }
    }

    /// Perform a DH ratchet step upon receiving a new remote public key.
    ///
    /// This is the core of the DH ratchet — a "double ratchet step":
    ///
    /// 1. DH with `new_remote_public` and our current secret key to derive
    ///    input for the receiving chain.
    /// 2. KDF(root\_key, dh\_output) to get a new root key + receiving chain key.
    /// 3. Generate a fresh DH key pair.
    /// 4. DH with `new_remote_public` and the *new* secret key to derive
    ///    input for the sending chain.
    /// 5. KDF(root\_key, dh\_output) to get another root key + sending chain key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if any DH operation or key derivation fails.
    /// Perform a DH ratchet step and return the derived header keys.
    ///
    /// Returns `(receiving_header_key, sending_header_key)` so the
    /// caller (e.g. the triple ratchet session) can rotate its header
    /// encryption state.
    pub fn ratchet_step(
        &mut self,
        new_remote_public: PublicKey,
    ) -> Result<([u8; kdf::KEY_SIZE], [u8; kdf::KEY_SIZE])> {
        // Step 1–2: derive receiving chain from current key pair
        let dh_output_recv =
            x25519::diffie_hellman_keypair(&self.dh_pair, &new_remote_public)?;
        let (new_rk, recv_chain_key, recv_header_key) =
            self.root_key.kdf(&dh_output_recv)?;
        self.root_key = new_rk;
        self.receiving_chain = Some(SymmetricRatchet::new(recv_chain_key));

        // Step 3: generate new DH key pair
        self.dh_pair = KeyPair::generate();

        // Step 4–5: derive sending chain from new key pair
        let dh_output_send =
            x25519::diffie_hellman_keypair(&self.dh_pair, &new_remote_public)?;
        let (new_rk, send_chain_key, send_header_key) =
            self.root_key.kdf(&dh_output_send)?;
        self.root_key = new_rk;
        self.sending_chain = Some(SymmetricRatchet::new(send_chain_key));

        // Update remote public key
        self.remote_public = Some(new_remote_public);

        Ok((recv_header_key, send_header_key))
    }

    /// Advance the sending chain and return a message key for encryption.
    ///
    /// Returns the message key, the message number (for the receiver to
    /// locate the correct chain position), and our current DH public key
    /// (so the receiver can detect when a ratchet step is needed).
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Ratchet`] if the sending chain has not been
    /// initialized (e.g. receiver before first ratchet step).
    pub fn encrypt_message_key(&mut self) -> Result<(MessageKey, u32, PublicKey)> {
        let chain = self.sending_chain.as_mut().ok_or_else(|| {
            CryptoError::Ratchet("sending chain not initialized".into())
        })?;
        let msg_num = chain.message_number();
        let mk = chain.advance();
        Ok((mk, msg_num, self.dh_pair.public_key()))
    }

    /// Advance the receiving chain to `msg_num` and return the message key.
    ///
    /// Also returns intermediate (skipped) keys for out-of-order caching.
    /// The caller must ensure a ratchet step has been performed if needed.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if:
    /// - the receiving chain has not been initialized
    /// - `msg_num` requires a backwards skip or exceeds [`MAX_SKIP`](crate::symmetric_ratchet::MAX_SKIP)
    pub fn receive_message_key(
        &mut self,
        msg_num: u32,
    ) -> Result<(MessageKey, Vec<(u32, MessageKey)>)> {
        let chain = self.receiving_chain.as_mut().ok_or_else(|| {
            CryptoError::Ratchet("receiving chain not initialized".into())
        })?;

        let current = chain.message_number();
        if msg_num < current {
            return Err(CryptoError::Ratchet(format!(
                "message number {msg_num} already consumed (current: {current})"
            )));
        }

        if msg_num == current {
            Ok((chain.advance(), Vec::new()))
        } else {
            // Skip to the target message number; skip_to returns all
            // keys up to and including target.
            let mut keys = chain.skip_to(msg_num)?;
            // The last key is the target; the rest are skipped intermediates.
            let (_, target_mk) = keys
                .pop()
                .expect("skip_to always returns at least one key");
            Ok((target_mk, keys))
        }
    }

    /// Skip remaining keys in the current receiving chain and return them.
    ///
    /// Called before a DH ratchet step to preserve intermediate keys from
    /// the old receiving chain. Returns the skipped `(msg_num, key)` pairs.
    /// Returns an empty vec if there is no receiving chain.
    pub fn skip_remaining_receiving_keys(
        &mut self,
        up_to: u32,
    ) -> Result<Vec<(u32, MessageKey)>> {
        let Some(chain) = self.receiving_chain.as_mut() else {
            return Ok(Vec::new());
        };

        let current = chain.message_number();
        if up_to <= current {
            return Ok(Vec::new());
        }

        // Skip to up_to - 1 so that we collect keys [current .. up_to)
        // (all the keys the remote sent but we haven't consumed yet)
        chain.skip_to(up_to - 1)
    }

    /// Obtain a message key for decryption (convenience wrapper).
    ///
    /// Performs a ratchet step if the remote public key has changed, then
    /// advances the receiving chain to `msg_num`. Returns the target key
    /// and any intermediate skipped keys.
    ///
    /// For callers that need to store skipped keys before the ratchet step
    /// (e.g. the triple ratchet session), use
    /// [`skip_remaining_receiving_keys`](Self::skip_remaining_receiving_keys),
    /// [`ratchet_step`](Self::ratchet_step), and
    /// [`receive_message_key`](Self::receive_message_key) separately.
    pub fn decrypt_message_key(
        &mut self,
        remote_public: &PublicKey,
        msg_num: u32,
    ) -> Result<(MessageKey, Vec<(u32, MessageKey)>)> {
        let needs_ratchet = match &self.remote_public {
            Some(stored) => stored != remote_public,
            None => true,
        };

        if needs_ratchet {
            let _header_keys = self.ratchet_step(*remote_public)?;
        }

        self.receive_message_key(msg_num)
    }

    /// Returns our current DH public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        self.dh_pair.public_key()
    }

    /// Returns the remote party's current public key, if known.
    #[must_use]
    pub fn remote_public_key(&self) -> Option<PublicKey> {
        self.remote_public
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared root key for tests.
    fn test_root_key() -> [u8; kdf::KEY_SIZE] {
        [0x42u8; kdf::KEY_SIZE]
    }

    // -----------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------

    #[test]
    fn init_sender_creates_sending_chain() {
        let bob = KeyPair::generate();
        let alice =
            DhRatchetState::init_sender(test_root_key(), bob.public_key()).expect("init failed").0;

        // Alice should have a sending chain and know Bob's public key
        assert!(alice.sending_chain.is_some());
        assert!(alice.receiving_chain.is_none());
        assert_eq!(alice.remote_public_key(), Some(bob.public_key()));
    }

    #[test]
    fn init_receiver_has_no_chains() {
        let bob_pair = KeyPair::generate();
        let bob = DhRatchetState::init_receiver(test_root_key(), bob_pair);

        assert!(bob.sending_chain.is_none());
        assert!(bob.receiving_chain.is_none());
        assert!(bob.remote_public_key().is_none());
    }

    // -----------------------------------------------------------
    // Basic send/receive
    // -----------------------------------------------------------

    #[test]
    fn sender_can_encrypt_message_key() {
        let bob = KeyPair::generate();
        let mut alice =
            DhRatchetState::init_sender(test_root_key(), bob.public_key()).expect("init failed").0;

        let (mk, num, pk) = alice.encrypt_message_key().expect("encrypt failed");
        assert_eq!(num, 0);
        assert_ne!(mk.as_bytes(), &[0u8; kdf::KEY_SIZE]);
        assert_eq!(pk, alice.public_key());
    }

    #[test]
    fn receiver_before_ratchet_cannot_encrypt() {
        let bob_pair = KeyPair::generate();
        let mut bob = DhRatchetState::init_receiver(test_root_key(), bob_pair);

        let result = bob.encrypt_message_key();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sending chain"));
    }

    // -----------------------------------------------------------
    // Bidirectional communication
    // -----------------------------------------------------------

    #[test]
    fn alice_sends_bob_receives() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice encrypts
        let (alice_mk, msg_num, alice_pub) =
            alice.encrypt_message_key().expect("encrypt failed");

        // Bob decrypts — this triggers a ratchet step on Bob's side
        let (bob_mk, _) = bob
            .decrypt_message_key(&alice_pub, msg_num)
            .expect("decrypt failed");

        assert_eq!(alice_mk.as_bytes(), bob_mk.as_bytes());
    }

    #[test]
    fn multiple_messages_same_direction() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice sends 3 messages
        let mut alice_keys = Vec::new();
        let mut alice_pub = alice.public_key();
        for _ in 0..3 {
            let (mk, _num, pk) = alice.encrypt_message_key().expect("encrypt failed");
            alice_pub = pk;
            alice_keys.push(mk);
        }

        // Bob decrypts all 3
        for (i, a_mk) in alice_keys.iter().enumerate() {
            let (bob_mk, _) = bob
                .decrypt_message_key(&alice_pub, i as u32)
                .expect("decrypt failed");
            assert_eq!(a_mk.as_bytes(), bob_mk.as_bytes(), "mismatch at msg {i}");
        }
    }

    #[test]
    fn bidirectional_conversation() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice -> Bob (message 0)
        let (a_mk0, a_num0, a_pub0) = alice.encrypt_message_key().expect("encrypt failed");
        let (b_mk0, _) = bob
            .decrypt_message_key(&a_pub0, a_num0)
            .expect("decrypt failed");
        assert_eq!(a_mk0.as_bytes(), b_mk0.as_bytes());

        // Bob -> Alice (message 0 on Bob's sending chain)
        let (b_mk1, b_num1, b_pub1) = bob.encrypt_message_key().expect("encrypt failed");
        let (a_mk1, _) = alice
            .decrypt_message_key(&b_pub1, b_num1)
            .expect("decrypt failed");
        assert_eq!(b_mk1.as_bytes(), a_mk1.as_bytes());

        // Alice -> Bob again (message 0 on Alice's new sending chain)
        let (a_mk2, a_num2, a_pub2) = alice.encrypt_message_key().expect("encrypt failed");
        let (b_mk2, _) = bob
            .decrypt_message_key(&a_pub2, a_num2)
            .expect("decrypt failed");
        assert_eq!(a_mk2.as_bytes(), b_mk2.as_bytes());
    }

    // -----------------------------------------------------------
    // Ratchet step behaviour
    // -----------------------------------------------------------

    #[test]
    fn ratchet_step_generates_new_key_pair() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let pub_before = alice.public_key();

        // Simulate receiving a message from Bob (triggers ratchet step)
        let remote_pk = KeyPair::generate().public_key();
        alice.ratchet_step(remote_pk).expect("ratchet step failed");

        let pub_after = alice.public_key();
        assert_ne!(
            pub_before.to_bytes(),
            pub_after.to_bytes(),
            "ratchet step must generate a new key pair"
        );
    }

    #[test]
    fn root_key_evolves_each_step() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let rk_after_init = *alice.root_key.as_bytes();

        let remote_pk = KeyPair::generate().public_key();
        alice.ratchet_step(remote_pk).expect("ratchet step failed");
        let rk_after_step1 = *alice.root_key.as_bytes();

        assert_ne!(rk_after_init, rk_after_step1, "root key must evolve");

        let remote_pk2 = KeyPair::generate().public_key();
        alice.ratchet_step(remote_pk2).expect("ratchet step failed");
        let rk_after_step2 = *alice.root_key.as_bytes();

        assert_ne!(rk_after_step1, rk_after_step2, "root key must evolve again");
        assert_ne!(rk_after_init, rk_after_step2, "root key must be unique");
    }

    #[test]
    fn ratchet_step_resets_chains() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;

        // Advance sending chain a few times
        alice.encrypt_message_key().expect("encrypt failed");
        alice.encrypt_message_key().expect("encrypt failed");
        let send_num = alice
            .sending_chain
            .as_ref()
            .map(SymmetricRatchet::message_number)
            .unwrap();
        assert_eq!(send_num, 2);

        // Ratchet step resets both chains
        let remote_pk = KeyPair::generate().public_key();
        alice.ratchet_step(remote_pk).expect("ratchet step failed");

        // Both chains start fresh at message number 0
        assert_eq!(
            alice
                .sending_chain
                .as_ref()
                .map(SymmetricRatchet::message_number),
            Some(0)
        );
        assert_eq!(
            alice
                .receiving_chain
                .as_ref()
                .map(SymmetricRatchet::message_number),
            Some(0)
        );
    }

    // -----------------------------------------------------------
    // Break-in recovery
    // -----------------------------------------------------------

    #[test]
    fn break_in_recovery() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice sends message 0
        let (mk0, num0, pub0) = alice.encrypt_message_key().expect("encrypt failed");
        bob.decrypt_message_key(&pub0, num0)
            .expect("decrypt failed");

        // Capture Alice's state "snapshot" — in a real attack, the adversary
        // would have compromised alice's current root key + chain keys.
        let compromised_root = *alice.root_key.as_bytes();

        // Bob replies, triggering ratchet steps on both sides
        let (b_mk, b_num, b_pub) = bob.encrypt_message_key().expect("encrypt failed");
        alice
            .decrypt_message_key(&b_pub, b_num)
            .expect("decrypt failed");

        // Alice sends again after ratchet step — new keys
        let (mk_new, _, _) = alice.encrypt_message_key().expect("encrypt failed");

        // The compromised root key should differ from Alice's current root key
        assert_ne!(
            compromised_root,
            *alice.root_key.as_bytes(),
            "root key must have evolved (break-in recovery)"
        );

        // And the message keys are different
        assert_ne!(
            mk0.as_bytes(),
            mk_new.as_bytes(),
            "new messages use fresh keys"
        );

        // Suppress unused variable warnings
        let _ = b_mk;
    }

    // -----------------------------------------------------------
    // Multiple rounds
    // -----------------------------------------------------------

    #[test]
    fn multiple_round_trips() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        for round in 0..5 {
            // Alice -> Bob
            let (a_mk, a_num, a_pub) =
                alice.encrypt_message_key().expect("alice encrypt failed");
            let (b_mk, _) = bob
                .decrypt_message_key(&a_pub, a_num)
                .expect("bob decrypt failed");
            assert_eq!(
                a_mk.as_bytes(),
                b_mk.as_bytes(),
                "round {round} A->B mismatch"
            );

            // Bob -> Alice
            let (b_mk2, b_num, b_pub) =
                bob.encrypt_message_key().expect("bob encrypt failed");
            let (a_mk2, _) = alice
                .decrypt_message_key(&b_pub, b_num)
                .expect("alice decrypt failed");
            assert_eq!(
                b_mk2.as_bytes(),
                a_mk2.as_bytes(),
                "round {round} B->A mismatch"
            );
        }
    }

    // -----------------------------------------------------------
    // Out-of-order message handling
    // -----------------------------------------------------------

    #[test]
    fn skip_message_in_receiving_chain() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice sends messages 0, 1, 2
        let (_mk0, _, pub0) = alice.encrypt_message_key().expect("encrypt 0");
        let (_mk1, _, _) = alice.encrypt_message_key().expect("encrypt 1");
        let (mk2, _, _) = alice.encrypt_message_key().expect("encrypt 2");

        // Bob receives message 2 first (skipping 0 and 1)
        let (bob_mk2, skipped) = bob
            .decrypt_message_key(&pub0, 2)
            .expect("decrypt msg 2 failed");
        assert_eq!(mk2.as_bytes(), bob_mk2.as_bytes());
        assert_eq!(skipped.len(), 2, "should return 2 skipped keys");
    }

    // -----------------------------------------------------------
    // Edge cases & errors
    // -----------------------------------------------------------

    #[test]
    fn decrypt_backwards_message_number_is_error() {
        let bob_pair = KeyPair::generate();
        let root = test_root_key();

        let mut alice =
            DhRatchetState::init_sender(root, bob_pair.public_key()).expect("init failed").0;
        let mut bob = DhRatchetState::init_receiver(root, bob_pair);

        // Alice sends messages 0 and 1
        let (_, _, pub0) = alice.encrypt_message_key().expect("encrypt 0");
        alice.encrypt_message_key().expect("encrypt 1");

        // Bob receives message 1
        bob.decrypt_message_key(&pub0, 1)
            .expect("decrypt 1");

        // Bob tries to receive message 0 (already past it)
        let result = bob.decrypt_message_key(&pub0, 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already consumed"));
    }

    #[test]
    fn unique_message_keys_per_message() {
        let bob = KeyPair::generate();
        let mut alice =
            DhRatchetState::init_sender(test_root_key(), bob.public_key()).expect("init failed").0;

        let (mk0, _, _) = alice.encrypt_message_key().expect("encrypt 0");
        let (mk1, _, _) = alice.encrypt_message_key().expect("encrypt 1");
        let (mk2, _, _) = alice.encrypt_message_key().expect("encrypt 2");

        assert_ne!(mk0.as_bytes(), mk1.as_bytes());
        assert_ne!(mk1.as_bytes(), mk2.as_bytes());
        assert_ne!(mk0.as_bytes(), mk2.as_bytes());
    }

    #[test]
    fn public_key_accessor() {
        let bob = KeyPair::generate();
        let alice =
            DhRatchetState::init_sender(test_root_key(), bob.public_key()).expect("init failed").0;

        let pk = alice.public_key();
        // Should be a valid non-zero public key
        assert!(pk.to_bytes().iter().any(|&b| b != 0));
    }
}
