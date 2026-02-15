//! Symmetric-key ratchet (KDF chain).
//!
//! Implements the sending and receiving KDF chains that derive per-message
//! keys. Each chain step advances the chain key through HKDF and outputs
//! a message key for AES-256-GCM encryption.

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};
use crate::kdf;

/// Maximum number of message keys that can be skipped in a single
/// `skip_to` call. Prevents denial-of-service from an adversary
/// requesting arbitrarily large skip distances.
pub const MAX_SKIP: u32 = 1000;

/// A 32-byte chain key used to derive the next chain state and message key.
///
/// Implements [`Zeroize`] and [`ZeroizeOnDrop`] so that old chain keys
/// are securely erased from memory when replaced.
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct ChainKey(pub(crate) [u8; kdf::KEY_SIZE]);

impl ChainKey {
    /// Creates a new `ChainKey` from raw bytes.
    #[must_use]
    pub fn new(bytes: [u8; kdf::KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns a reference to the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; kdf::KEY_SIZE] {
        &self.0
    }
}

/// A 32-byte per-message encryption key.
///
/// Each message key is used exactly once for AES-256-GCM encryption and
/// then discarded. Implements [`Zeroize`] and [`ZeroizeOnDrop`] for
/// forward secrecy.
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct MessageKey(pub(crate) [u8; kdf::KEY_SIZE]);

impl MessageKey {
    /// Returns a reference to the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; kdf::KEY_SIZE] {
        &self.0
    }
}

/// A symmetric-key ratchet that derives per-message keys from a KDF chain.
///
/// Each call to [`advance`](Self::advance) steps the chain forward by one,
/// producing a unique [`MessageKey`] and replacing the internal
/// [`ChainKey`]. The old chain key is zeroized, providing forward secrecy.
pub struct SymmetricRatchet {
    chain_key: ChainKey,
    message_number: u32,
}

impl SymmetricRatchet {
    /// Creates a new ratchet initialised with the given chain key.
    ///
    /// The message counter starts at zero.
    #[must_use]
    pub fn new(chain_key: ChainKey) -> Self {
        Self {
            chain_key,
            message_number: 0,
        }
    }

    /// Returns the current message number (the index of the *next* message
    /// key that will be produced).
    #[must_use]
    pub fn message_number(&self) -> u32 {
        self.message_number
    }

    /// Returns a reference to the current chain key.
    #[must_use]
    pub fn chain_key(&self) -> &ChainKey {
        &self.chain_key
    }

    /// Advances the ratchet by one step, producing a message key.
    ///
    /// Internally calls [`kdf::kdf_chain`] with the current chain key and
    /// an empty input (the DH ratchet injects shared secrets at a higher
    /// layer). The old chain key is zeroized via [`ChainKey`]'s
    /// [`ZeroizeOnDrop`] when it is replaced.
    pub fn advance(&mut self) -> MessageKey {
        let (new_ck, mk) = kdf::kdf_chain(self.chain_key.as_bytes(), &[]);
        self.chain_key = ChainKey::new(new_ck);
        self.message_number += 1;
        MessageKey(mk)
    }

    /// Skips ahead to `target` message number, collecting all intermediate
    /// message keys for out-of-order decryption.
    ///
    /// Returns a vector of `(message_number, MessageKey)` pairs for every
    /// skipped position *up to and including* `target`. The ratchet's
    /// message counter is set to `target + 1` afterwards.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Ratchet`] if:
    /// - `target` is less than the current message number (cannot go back)
    /// - the skip distance exceeds [`MAX_SKIP`]
    pub fn skip_to(&mut self, target: u32) -> Result<Vec<(u32, MessageKey)>> {
        if target < self.message_number {
            return Err(CryptoError::Ratchet(format!(
                "cannot skip backwards: current {}, target {target}",
                self.message_number,
            )));
        }

        let skip_count = target - self.message_number + 1;
        if skip_count > MAX_SKIP {
            return Err(CryptoError::Ratchet(format!(
                "skip distance {skip_count} exceeds MAX_SKIP ({MAX_SKIP})",
            )));
        }

        let mut keys = Vec::with_capacity(skip_count as usize);
        for _ in 0..skip_count {
            let n = self.message_number;
            let mk = self.advance();
            keys.push((n, mk));
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_chain_key() -> ChainKey {
        ChainKey::new([0xAA; kdf::KEY_SIZE])
    }

    // ---------------------------------------------------------------
    // Basic advancement
    // ---------------------------------------------------------------

    #[test]
    fn advance_produces_message_key() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        let mk = ratchet.advance();
        // Message key should not be all zeros
        assert_ne!(mk.as_bytes(), &[0u8; kdf::KEY_SIZE]);
    }

    #[test]
    fn advance_increments_message_number() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        assert_eq!(ratchet.message_number(), 0);

        ratchet.advance();
        assert_eq!(ratchet.message_number(), 1);

        ratchet.advance();
        assert_eq!(ratchet.message_number(), 2);

        ratchet.advance();
        assert_eq!(ratchet.message_number(), 3);
    }

    #[test]
    fn advance_produces_unique_keys() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        let mk1 = ratchet.advance();
        let mk2 = ratchet.advance();
        let mk3 = ratchet.advance();

        assert_ne!(mk1.as_bytes(), mk2.as_bytes());
        assert_ne!(mk2.as_bytes(), mk3.as_bytes());
        assert_ne!(mk1.as_bytes(), mk3.as_bytes());
    }

    #[test]
    fn advance_changes_chain_key() {
        let initial = test_chain_key();
        let initial_bytes = *initial.as_bytes();
        let mut ratchet = SymmetricRatchet::new(initial);

        ratchet.advance();
        let after_one = *ratchet.chain_key().as_bytes();
        assert_ne!(initial_bytes, after_one);

        ratchet.advance();
        let after_two = *ratchet.chain_key().as_bytes();
        assert_ne!(after_one, after_two);
    }

    // ---------------------------------------------------------------
    // Determinism
    // ---------------------------------------------------------------

    #[test]
    fn ratchet_is_deterministic() {
        let mut r1 = SymmetricRatchet::new(test_chain_key());
        let mut r2 = SymmetricRatchet::new(test_chain_key());

        for _ in 0..5 {
            let mk1 = r1.advance();
            let mk2 = r2.advance();
            assert_eq!(mk1.as_bytes(), mk2.as_bytes());
        }
    }

    #[test]
    fn different_initial_keys_produce_different_sequences() {
        let mut r1 = SymmetricRatchet::new(ChainKey::new([0x01; kdf::KEY_SIZE]));
        let mut r2 = SymmetricRatchet::new(ChainKey::new([0x02; kdf::KEY_SIZE]));

        let mk1 = r1.advance();
        let mk2 = r2.advance();
        assert_ne!(mk1.as_bytes(), mk2.as_bytes());
    }

    // ---------------------------------------------------------------
    // skip_to
    // ---------------------------------------------------------------

    #[test]
    fn skip_to_current_position() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        // Skip to message 0 — should produce exactly one key
        let keys = ratchet.skip_to(0).expect("skip_to(0) should succeed");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, 0);
        assert_eq!(ratchet.message_number(), 1);
    }

    #[test]
    fn skip_to_collects_intermediate_keys() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        let keys = ratchet.skip_to(4).expect("skip_to(4) should succeed");

        assert_eq!(keys.len(), 5);
        for (i, (num, _)) in keys.iter().enumerate() {
            assert_eq!(*num, i as u32);
        }
        assert_eq!(ratchet.message_number(), 5);
    }

    #[test]
    fn skip_to_matches_sequential_advance() {
        // Advance sequentially and collect keys
        let mut sequential = SymmetricRatchet::new(test_chain_key());
        let mut seq_keys = Vec::new();
        for _ in 0..5 {
            seq_keys.push(sequential.advance());
        }

        // Skip to the same position
        let mut skipped = SymmetricRatchet::new(test_chain_key());
        let skip_keys = skipped.skip_to(4).expect("skip_to should succeed");

        // Keys must match
        for (i, (_, mk)) in skip_keys.iter().enumerate() {
            assert_eq!(
                mk.as_bytes(),
                seq_keys[i].as_bytes(),
                "mismatch at position {i}"
            );
        }
    }

    #[test]
    fn skip_to_rejects_backwards() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        ratchet.advance(); // now at message 1
        ratchet.advance(); // now at message 2

        let result = ratchet.skip_to(1);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot skip backwards"),
        );
    }

    #[test]
    fn skip_to_rejects_excessive_skip() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        let result = ratchet.skip_to(MAX_SKIP + 1);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds MAX_SKIP"),
        );
    }

    #[test]
    fn skip_to_at_max_skip_boundary() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        // Exactly MAX_SKIP is allowed (skip from 0 to MAX_SKIP - 1)
        let result = ratchet.skip_to(MAX_SKIP - 1);
        assert!(result.is_ok());
        let keys = result.unwrap();
        assert_eq!(keys.len(), MAX_SKIP as usize);
        assert_eq!(ratchet.message_number(), MAX_SKIP);
    }

    // ---------------------------------------------------------------
    // Zeroization
    // ---------------------------------------------------------------

    #[test]
    fn old_chain_key_is_replaced() {
        let mut ratchet = SymmetricRatchet::new(test_chain_key());
        let before = *ratchet.chain_key().as_bytes();
        ratchet.advance();
        let after = *ratchet.chain_key().as_bytes();
        // We cannot directly test that memory was zeroed (the old
        // ChainKey is dropped), but we can verify the chain key changed.
        assert_ne!(before, after);
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[test]
    fn zero_chain_key_still_produces_keys() {
        let mut ratchet = SymmetricRatchet::new(ChainKey::new([0x00; kdf::KEY_SIZE]));
        let mk = ratchet.advance();
        // Even a zero chain key should produce a non-zero message key
        // (HKDF guarantees useful output from any input).
        assert_ne!(mk.as_bytes(), &[0u8; kdf::KEY_SIZE]);
    }

    #[test]
    fn constructor_starts_at_zero() {
        let ratchet = SymmetricRatchet::new(test_chain_key());
        assert_eq!(ratchet.message_number(), 0);
    }
}
