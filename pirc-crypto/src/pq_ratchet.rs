//! Post-quantum ratchet.
//!
//! Implements the third ratchet in the triple ratchet protocol, using
//! ML-KEM key encapsulation to periodically inject post-quantum keying
//! material into the KDF chain. This provides long-term resistance
//! against quantum computing attacks.
//!
//! Unlike the DH ratchet (which performs an interactive exchange), the
//! PQ ratchet is a one-shot operation: the initiator encapsulates a
//! shared secret under the responder's KEM public key, and the
//! responder decapsulates to recover the same secret. Both sides then
//! mix this secret into their PQ chain key via HKDF.

use crate::error::{CryptoError, Result};
use crate::kdf;
use crate::kem::{KemCiphertext, KemKeyPair, KemPublicKey};

/// Info string for PQ chain key derivation during a ratchet step.
const PQ_CHAIN_KEY_INFO: &[u8] = b"pirc-pq-ratchet-chain";

/// The post-quantum ratchet state for one party in a session.
///
/// Manages ML-KEM key pairs and uses KEM shared secrets to periodically
/// inject post-quantum entropy into a chain key. The chain key output
/// is mixed with the DH ratchet root key in the triple ratchet session.
pub struct PqRatchetState {
    /// Our current KEM key pair.
    kem_pair: KemKeyPair,
    /// The remote party's current KEM public key.
    remote_kem_public: Option<KemPublicKey>,
    /// Post-quantum chain key — evolves with each PQ ratchet step.
    pq_chain_key: [u8; kdf::KEY_SIZE],
    /// Number of completed PQ ratchet steps.
    step_counter: u32,
}

impl PqRatchetState {
    /// Create a new PQ ratchet state with an initial chain key.
    ///
    /// Generates a fresh KEM key pair. The remote public key is not yet
    /// known — it will be set when the first PQ ratchet step completes.
    #[must_use]
    pub fn new(initial_key: [u8; kdf::KEY_SIZE]) -> Self {
        Self {
            kem_pair: KemKeyPair::generate(),
            remote_kem_public: None,
            pq_chain_key: initial_key,
            step_counter: 0,
        }
    }

    /// Create a new PQ ratchet state with an existing KEM key pair.
    ///
    /// Used by the session receiver who shared their KEM public key
    /// out-of-band. The sender will encapsulate under this public key
    /// during the first PQ ratchet step.
    #[must_use]
    pub fn with_keypair(initial_key: [u8; kdf::KEY_SIZE], kem_pair: KemKeyPair) -> Self {
        Self {
            kem_pair,
            remote_kem_public: None,
            pq_chain_key: initial_key,
            step_counter: 0,
        }
    }

    /// Initiate a PQ ratchet step (sender side).
    ///
    /// Encapsulates a fresh shared secret under the remote party's KEM
    /// public key, mixes the shared secret into the PQ chain key via
    /// HKDF, and generates a new KEM key pair for the next round.
    ///
    /// Returns the ciphertext and the new KEM public key to send to the
    /// peer. The peer must call [`complete_step`](Self::complete_step)
    /// with these values.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Ratchet`] if the remote KEM public key is
    /// not set, or [`CryptoError::Kem`] if encapsulation fails.
    pub fn initiate_step(&mut self) -> Result<(KemCiphertext, KemPublicKey)> {
        let remote_pk = self.remote_kem_public.as_ref().ok_or_else(|| {
            CryptoError::Ratchet("remote KEM public key not set".into())
        })?;

        // Encapsulate a fresh shared secret under the remote's public key
        let (ciphertext, shared_secret) = remote_pk.encapsulate()?;

        // Mix shared secret into PQ chain key via HKDF (zero-allocation)
        let mut new_key = [0u8; kdf::KEY_SIZE];
        kdf::derive_key_into(
            &self.pq_chain_key,
            shared_secret.as_bytes(),
            PQ_CHAIN_KEY_INFO,
            &mut new_key,
        )?;
        self.pq_chain_key = new_key;

        // Generate new KEM key pair for forward secrecy
        self.kem_pair = KemKeyPair::generate();
        self.step_counter += 1;

        Ok((ciphertext, self.kem_pair.public_key()))
    }

    /// Complete a PQ ratchet step (receiver side).
    ///
    /// Decapsulates the ciphertext using our current KEM secret key to
    /// recover the shared secret, mixes it into the PQ chain key via
    /// HKDF, stores the new remote KEM public key, and generates a new
    /// KEM key pair.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Kem`] if decapsulation fails, or
    /// [`CryptoError::KeyDerivation`] if HKDF fails.
    pub fn complete_step(
        &mut self,
        ciphertext: &KemCiphertext,
        new_remote_public: &KemPublicKey,
    ) -> Result<()> {
        // Decapsulate using our current KEM secret key
        let shared_secret = self.kem_pair.decapsulate(ciphertext)?;

        // Mix shared secret into PQ chain key via HKDF (zero-allocation)
        let mut new_key = [0u8; kdf::KEY_SIZE];
        kdf::derive_key_into(
            &self.pq_chain_key,
            shared_secret.as_bytes(),
            PQ_CHAIN_KEY_INFO,
            &mut new_key,
        )?;
        self.pq_chain_key = new_key;

        // Store new remote KEM public key
        self.remote_kem_public = Some(new_remote_public.clone());

        // Generate new KEM key pair for forward secrecy
        self.kem_pair = KemKeyPair::generate();
        self.step_counter += 1;

        Ok(())
    }

    /// Return the current PQ chain key.
    ///
    /// This key is mixed with the DH ratchet root key in the triple
    /// ratchet session to provide post-quantum resistance.
    #[must_use]
    pub fn current_key(&self) -> &[u8; kdf::KEY_SIZE] {
        &self.pq_chain_key
    }

    /// Return the number of completed PQ ratchet steps.
    #[must_use]
    pub fn step_counter(&self) -> u32 {
        self.step_counter
    }

    /// Return our current KEM public key.
    ///
    /// The peer needs this to encapsulate shared secrets for the next
    /// PQ ratchet step.
    #[must_use]
    pub fn public_key(&self) -> KemPublicKey {
        self.kem_pair.public_key()
    }

    /// Set the remote party's KEM public key.
    ///
    /// This must be called before [`initiate_step`](Self::initiate_step)
    /// can be used.
    pub fn set_remote_public_key(&mut self, pk: KemPublicKey) {
        self.remote_kem_public = Some(pk);
    }
}

impl std::fmt::Debug for PqRatchetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PqRatchetState")
            .field("kem_pair", &"[REDACTED]")
            .field("remote_kem_public", &self.remote_kem_public.is_some())
            .field("pq_chain_key", &"[REDACTED]")
            .field("step_counter", &self.step_counter)
            .finish()
    }
}

impl Drop for PqRatchetState {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.pq_chain_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_initial_key() -> [u8; kdf::KEY_SIZE] {
        [0x42u8; kdf::KEY_SIZE]
    }

    // -----------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------

    #[test]
    fn new_creates_state_with_initial_key() {
        let state = PqRatchetState::new(test_initial_key());
        assert_eq!(*state.current_key(), test_initial_key());
        assert_eq!(state.step_counter(), 0);
        assert!(state.remote_kem_public.is_none());
    }

    #[test]
    fn new_generates_non_zero_public_key() {
        let state = PqRatchetState::new(test_initial_key());
        let pk_bytes = state.public_key().to_bytes();
        assert!(pk_bytes.iter().any(|&b| b != 0));
    }

    // -----------------------------------------------------------
    // Single step
    // -----------------------------------------------------------

    #[test]
    fn one_step_derives_matching_chain_keys() {
        let initial_key = test_initial_key();

        // Alice (initiator) and Bob (responder)
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        // Exchange public keys
        alice.set_remote_public_key(bob.public_key());
        bob.set_remote_public_key(alice.public_key());

        // Alice initiates a PQ ratchet step
        let (ciphertext, alice_new_pk) = alice.initiate_step().expect("initiate failed");

        // Bob completes the step
        bob.complete_step(&ciphertext, &alice_new_pk)
            .expect("complete failed");

        // Both should have the same chain key
        assert_eq!(alice.current_key(), bob.current_key());
        assert_eq!(alice.step_counter(), 1);
        assert_eq!(bob.step_counter(), 1);
    }

    #[test]
    fn chain_key_evolves_after_step() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        alice.set_remote_public_key(bob.public_key());

        let (ciphertext, alice_new_pk) = alice.initiate_step().expect("initiate failed");
        bob.complete_step(&ciphertext, &alice_new_pk)
            .expect("complete failed");

        // Chain key must have changed from the initial value
        assert_ne!(*alice.current_key(), initial_key);
        assert_ne!(*bob.current_key(), initial_key);
    }

    // -----------------------------------------------------------
    // Multiple steps
    // -----------------------------------------------------------

    #[test]
    fn multiple_steps_derive_matching_keys() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        alice.set_remote_public_key(bob.public_key());
        bob.set_remote_public_key(alice.public_key());

        let mut prev_key = initial_key;

        for step in 0..5 {
            // Alternate who initiates
            if step % 2 == 0 {
                // Alice initiates
                let (ct, new_pk) = alice.initiate_step().expect("alice initiate failed");
                bob.complete_step(&ct, &new_pk).expect("bob complete failed");
                // Bob now knows Alice's new key; Alice needs Bob's
                alice.set_remote_public_key(bob.public_key());
            } else {
                // Bob initiates
                let (ct, new_pk) = bob.initiate_step().expect("bob initiate failed");
                alice.complete_step(&ct, &new_pk).expect("alice complete failed");
                // Alice now knows Bob's new key; Bob needs Alice's
                bob.set_remote_public_key(alice.public_key());
            }

            // Keys must match after each step
            assert_eq!(
                alice.current_key(),
                bob.current_key(),
                "mismatch at step {step}"
            );

            // Key must differ from previous step
            assert_ne!(
                *alice.current_key(),
                prev_key,
                "chain key didn't evolve at step {step}"
            );

            prev_key = *alice.current_key();
        }

        assert_eq!(alice.step_counter(), 5);
        assert_eq!(bob.step_counter(), 5);
    }

    #[test]
    fn step_counter_increments() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        alice.set_remote_public_key(bob.public_key());

        for i in 0..3 {
            let (ct, new_pk) = alice.initiate_step().expect("initiate failed");
            bob.complete_step(&ct, &new_pk).expect("complete failed");
            alice.set_remote_public_key(bob.public_key());
            assert_eq!(alice.step_counter(), i + 1);
            assert_eq!(bob.step_counter(), i + 1);
        }
    }

    // -----------------------------------------------------------
    // Key replacement
    // -----------------------------------------------------------

    #[test]
    fn kem_keys_replaced_after_step() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let bob = PqRatchetState::new(initial_key);

        let alice_pk_before = alice.public_key().to_bytes();
        alice.set_remote_public_key(bob.public_key());

        let _ = alice.initiate_step().expect("initiate failed");

        let alice_pk_after = alice.public_key().to_bytes();
        assert_ne!(
            alice_pk_before, alice_pk_after,
            "KEM key pair must be replaced after step"
        );
    }

    #[test]
    fn responder_kem_keys_replaced_after_step() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        alice.set_remote_public_key(bob.public_key());
        let bob_pk_before = bob.public_key().to_bytes();

        let (ct, new_pk) = alice.initiate_step().expect("initiate failed");
        bob.complete_step(&ct, &new_pk).expect("complete failed");

        let bob_pk_after = bob.public_key().to_bytes();
        assert_ne!(
            bob_pk_before, bob_pk_after,
            "responder KEM key pair must be replaced after step"
        );
    }

    // -----------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------

    #[test]
    fn initiate_without_remote_key_fails() {
        let mut state = PqRatchetState::new(test_initial_key());
        let result = state.initiate_step();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("remote KEM public key not set")
        );
    }

    #[test]
    fn decapsulate_with_wrong_key_produces_different_chain_key() {
        // ML-KEM decapsulation with wrong key returns a pseudorandom
        // secret (implicit rejection), so both parties end up with
        // different chain keys — the step "succeeds" but produces
        // a mismatch.
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);
        let eve = PqRatchetState::new(initial_key);

        // Alice encapsulates under Eve's key instead of Bob's
        alice.set_remote_public_key(eve.public_key());
        let (ct, new_pk) = alice.initiate_step().expect("initiate failed");

        // Bob tries to decapsulate — wrong key, so he gets a different secret
        bob.complete_step(&ct, &new_pk).expect("complete should not error");

        assert_ne!(
            alice.current_key(),
            bob.current_key(),
            "mismatched keys must produce different chain keys"
        );
    }

    // -----------------------------------------------------------
    // Debug output
    // -----------------------------------------------------------

    #[test]
    fn debug_redacts_secrets() {
        let state = PqRatchetState::new(test_initial_key());
        let debug = format!("{state:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("step_counter: 0"));
        assert!(!debug.contains("42")); // initial key byte shouldn't leak
    }

    // -----------------------------------------------------------
    // Forward secrecy
    // -----------------------------------------------------------

    #[test]
    fn old_keys_cannot_derive_new_chain_key() {
        let initial_key = test_initial_key();
        let mut alice = PqRatchetState::new(initial_key);
        let mut bob = PqRatchetState::new(initial_key);

        alice.set_remote_public_key(bob.public_key());
        bob.set_remote_public_key(alice.public_key());

        // Do one step
        let (ct, new_pk) = alice.initiate_step().expect("initiate failed");
        bob.complete_step(&ct, &new_pk).expect("complete failed");
        let key_after_step1 = *alice.current_key();

        // Do another step
        alice.set_remote_public_key(bob.public_key());
        let (ct2, new_pk2) = alice.initiate_step().expect("initiate failed");
        bob.complete_step(&ct2, &new_pk2).expect("complete failed");
        let key_after_step2 = *alice.current_key();

        // All keys must be distinct — knowing one doesn't reveal others
        assert_ne!(initial_key, key_after_step1);
        assert_ne!(key_after_step1, key_after_step2);
        assert_ne!(initial_key, key_after_step2);

        // Both parties agree
        assert_eq!(*bob.current_key(), key_after_step2);
    }
}
