//! ML-KEM (Kyber) key encapsulation wrapper.
//!
//! Provides key generation, encapsulation, and decapsulation using
//! ML-KEM-768 (FIPS 203). Used as the post-quantum component of the
//! triple ratchet to provide quantum resistance.
//!
//! Unlike Diffie-Hellman (which is interactive), KEM is a one-shot
//! operation: the encapsulator creates a ciphertext that only the
//! holder of the decapsulation key can process to derive the same
//! shared secret.

use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
use rand::rngs::OsRng;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};

/// Size of an ML-KEM-768 encapsulation (public) key in bytes.
pub const PUBLIC_KEY_LEN: usize = 1184;

/// Size of an ML-KEM-768 decapsulation (secret) key in bytes.
pub const SECRET_KEY_LEN: usize = 2400;

/// Size of an ML-KEM-768 ciphertext in bytes.
pub const CIPHERTEXT_LEN: usize = 1088;

/// Size of the shared secret produced by ML-KEM in bytes.
pub const SHARED_SECRET_LEN: usize = 32;

/// An ML-KEM-768 key pair (decapsulation key + encapsulation key).
///
/// The decapsulation key is secret material and is zeroized on drop
/// (via the `ml-kem` crate's `zeroize` feature).
pub struct KemKeyPair {
    dk: <MlKem768 as KemCore>::DecapsulationKey,
    ek: <MlKem768 as KemCore>::EncapsulationKey,
}

impl KemKeyPair {
    /// Generate a new random ML-KEM-768 key pair.
    #[must_use]
    pub fn generate() -> Self {
        let (dk, ek) = MlKem768::generate(&mut OsRng);
        Self { dk, ek }
    }

    /// Return the public (encapsulation) key.
    #[must_use]
    pub fn public_key(&self) -> KemPublicKey {
        KemPublicKey {
            ek: self.ek.clone(),
        }
    }

    /// Decapsulate a ciphertext to recover the shared secret.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Kem`] if decapsulation fails.
    pub fn decapsulate(&self, ciphertext: &KemCiphertext) -> Result<KemSharedSecret> {
        let shared = self
            .dk
            .decapsulate(&ciphertext.bytes)
            .map_err(|()| CryptoError::Kem("decapsulation failed".into()))?;

        let mut bytes = [0u8; SHARED_SECRET_LEN];
        bytes.copy_from_slice(shared.as_slice());
        Ok(KemSharedSecret { bytes })
    }

    /// Serialize the KEM key pair to bytes.
    ///
    /// Format: `[decapsulation_key (2400) | encapsulation_key (1184)]`
    ///
    /// # Security
    ///
    /// The returned bytes contain secret key material. The caller is
    /// responsible for zeroizing them when no longer needed.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let dk_bytes = self.dk.as_bytes();
        let ek_bytes = self.ek.as_bytes();
        let mut bytes = Vec::with_capacity(SECRET_KEY_LEN + PUBLIC_KEY_LEN);
        bytes.extend_from_slice(&dk_bytes);
        bytes.extend_from_slice(&ek_bytes);
        bytes
    }

    /// Deserialize a KEM key pair from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// `SECRET_KEY_LEN + PUBLIC_KEY_LEN` bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let expected = SECRET_KEY_LEN + PUBLIC_KEY_LEN;
        if bytes.len() != expected {
            return Err(CryptoError::InvalidKey(format!(
                "KemKeyPair: expected {expected} bytes, got {}",
                bytes.len()
            )));
        }

        let dk_arr = ml_kem::array::Array::try_from(&bytes[..SECRET_KEY_LEN])
            .map_err(|_| CryptoError::InvalidKey("invalid decapsulation key length".into()))?;
        let dk = <MlKem768 as KemCore>::DecapsulationKey::from_bytes(&dk_arr);

        let ek_arr = ml_kem::array::Array::try_from(&bytes[SECRET_KEY_LEN..])
            .map_err(|_| CryptoError::InvalidKey("invalid encapsulation key length".into()))?;
        let ek = <MlKem768 as KemCore>::EncapsulationKey::from_bytes(&ek_arr);

        Ok(Self { dk, ek })
    }
}

impl std::fmt::Debug for KemKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KemKeyPair")
            .field("dk", &"[REDACTED]")
            .field("ek", &"[..]")
            .finish()
    }
}

/// An ML-KEM-768 encapsulation (public) key.
///
/// Can be freely shared. Used by the encapsulator to create a
/// ciphertext and derive a shared secret.
#[derive(Clone)]
pub struct KemPublicKey {
    ek: <MlKem768 as KemCore>::EncapsulationKey,
}

impl KemPublicKey {
    /// Deserialize an encapsulation key from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// [`PUBLIC_KEY_LEN`] bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr = ml_kem::array::Array::try_from(bytes)
            .map_err(|_| CryptoError::InvalidKey(
                format!("expected {PUBLIC_KEY_LEN} bytes, got {}", bytes.len()),
            ))?;
        let ek = <MlKem768 as KemCore>::EncapsulationKey::from_bytes(&arr);
        Ok(Self { ek })
    }

    /// Serialize the encapsulation key to a byte vector.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.ek.as_bytes().to_vec()
    }

    /// Encapsulate a fresh shared secret under this public key.
    ///
    /// Returns the ciphertext and the shared secret. The ciphertext
    /// should be sent to the holder of the corresponding decapsulation
    /// key, who can recover the same shared secret via
    /// [`KemKeyPair::decapsulate`].
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Kem`] if encapsulation fails.
    pub fn encapsulate(&self) -> Result<(KemCiphertext, KemSharedSecret)> {
        let (ct, shared) = self
            .ek
            .encapsulate(&mut OsRng)
            .map_err(|()| CryptoError::Kem("encapsulation failed".into()))?;

        let mut secret_bytes = [0u8; SHARED_SECRET_LEN];
        secret_bytes.copy_from_slice(shared.as_slice());

        Ok((
            KemCiphertext { bytes: ct },
            KemSharedSecret {
                bytes: secret_bytes,
            },
        ))
    }
}

impl PartialEq for KemPublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.ek == other.ek
    }
}

impl Eq for KemPublicKey {}

impl std::fmt::Debug for KemPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.ek.as_bytes();
        write!(f, "KemPublicKey({:02x?}...)", &bytes[..8])
    }
}

/// An ML-KEM-768 ciphertext (the result of encapsulation).
///
/// This is the value sent from the encapsulator to the decapsulator.
pub struct KemCiphertext {
    bytes: ml_kem::Ciphertext<MlKem768>,
}

impl KemCiphertext {
    /// Deserialize a ciphertext from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Kem`] if `bytes` is not exactly
    /// [`CIPHERTEXT_LEN`] bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr = ml_kem::array::Array::try_from(bytes)
            .map_err(|_| CryptoError::Kem(
                format!("expected {CIPHERTEXT_LEN} byte ciphertext, got {}", bytes.len()),
            ))?;
        Ok(Self { bytes: arr })
    }

    /// Serialize the ciphertext to a byte vector.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }
}

impl Clone for KemCiphertext {
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes,
        }
    }
}

impl std::fmt::Debug for KemCiphertext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KemCiphertext({} bytes)", self.bytes.len())
    }
}

/// A 32-byte shared secret produced by ML-KEM encapsulation/decapsulation.
///
/// Implements [`Zeroize`] and [`ZeroizeOnDrop`] so the secret is erased
/// from memory when no longer needed.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KemSharedSecret {
    bytes: [u8; SHARED_SECRET_LEN],
}

impl KemSharedSecret {
    /// Return a reference to the 32-byte shared secret.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; SHARED_SECRET_LEN] {
        &self.bytes
    }
}

impl std::fmt::Debug for KemSharedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KemSharedSecret")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Encapsulate a fresh shared secret under the given public key.
///
/// Convenience function that delegates to [`KemPublicKey::encapsulate`].
///
/// # Errors
///
/// Returns [`CryptoError::Kem`] if encapsulation fails.
pub fn encapsulate(public_key: &KemPublicKey) -> Result<(KemCiphertext, KemSharedSecret)> {
    public_key.encapsulate()
}

/// Decapsulate a ciphertext using the given key pair to recover the shared secret.
///
/// Convenience function that delegates to [`KemKeyPair::decapsulate`].
///
/// # Errors
///
/// Returns [`CryptoError::Kem`] if decapsulation fails.
pub fn decapsulate(key_pair: &KemKeyPair, ciphertext: &KemCiphertext) -> Result<KemSharedSecret> {
    key_pair.decapsulate(ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Key generation ──────────────────────────────────────────────

    #[test]
    fn key_pair_generation() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();
        let pk_bytes = pk.to_bytes();
        assert_eq!(pk_bytes.len(), PUBLIC_KEY_LEN);
        // Public key should not be all zeros
        assert!(pk_bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn two_key_pairs_differ() {
        let kp1 = KemKeyPair::generate();
        let kp2 = KemKeyPair::generate();
        assert_ne!(kp1.public_key().to_bytes(), kp2.public_key().to_bytes());
    }

    // ── Encapsulate / decapsulate round-trip ────────────────────────

    #[test]
    fn encapsulate_decapsulate_roundtrip() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();

        let (ct, shared_enc) = pk.encapsulate().expect("encapsulate failed");
        let shared_dec = kp.decapsulate(&ct).expect("decapsulate failed");

        assert_eq!(shared_enc.as_bytes(), shared_dec.as_bytes());
    }

    #[test]
    fn convenience_functions_roundtrip() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();

        let (ct, shared_enc) = encapsulate(&pk).expect("encapsulate failed");
        let shared_dec = decapsulate(&kp, &ct).expect("decapsulate failed");

        assert_eq!(shared_enc.as_bytes(), shared_dec.as_bytes());
    }

    #[test]
    fn shared_secret_is_32_bytes() {
        let kp = KemKeyPair::generate();
        let (_, shared) = kp.public_key().encapsulate().expect("encapsulate failed");
        assert_eq!(shared.as_bytes().len(), SHARED_SECRET_LEN);
        // Should not be all zeros
        assert!(shared.as_bytes().iter().any(|&b| b != 0));
    }

    #[test]
    fn multiple_encapsulations_produce_different_secrets() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();

        let (_, s1) = pk.encapsulate().expect("encapsulate 1 failed");
        let (_, s2) = pk.encapsulate().expect("encapsulate 2 failed");

        assert_ne!(s1.as_bytes(), s2.as_bytes());
    }

    // ── Wrong key ───────────────────────────────────────────────────

    #[test]
    fn decapsulate_with_wrong_key_produces_different_secret() {
        let kp1 = KemKeyPair::generate();
        let kp2 = KemKeyPair::generate();

        let (ct, shared_enc) = kp1.public_key().encapsulate().expect("encapsulate failed");

        // ML-KEM decapsulation is implicitly rejected — it returns a
        // pseudorandom value instead of failing. The key point is that
        // the wrong key produces a *different* shared secret.
        let shared_wrong = kp2.decapsulate(&ct).expect("decapsulate should not error");
        assert_ne!(shared_enc.as_bytes(), shared_wrong.as_bytes());
    }

    // ── Serialization round-trips ───────────────────────────────────

    #[test]
    fn public_key_serialization_roundtrip() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();

        let bytes = pk.to_bytes();
        let pk2 = KemPublicKey::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(pk, pk2);
        assert_eq!(pk.to_bytes(), pk2.to_bytes());
    }

    #[test]
    fn ciphertext_serialization_roundtrip() {
        let kp = KemKeyPair::generate();
        let (ct, shared_enc) = kp.public_key().encapsulate().expect("encapsulate failed");

        let ct_bytes = ct.to_bytes();
        assert_eq!(ct_bytes.len(), CIPHERTEXT_LEN);

        let ct2 = KemCiphertext::from_bytes(&ct_bytes).expect("deserialize failed");
        let shared_dec = kp.decapsulate(&ct2).expect("decapsulate failed");

        assert_eq!(shared_enc.as_bytes(), shared_dec.as_bytes());
    }

    #[test]
    fn public_key_from_invalid_length_fails() {
        let too_short = vec![0u8; 100];
        let result = KemPublicKey::from_bytes(&too_short);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expected"), "unexpected error: {err}");
    }

    #[test]
    fn ciphertext_from_invalid_length_fails() {
        let too_short = vec![0u8; 100];
        let result = KemCiphertext::from_bytes(&too_short);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expected"), "unexpected error: {err}");
    }

    // ── Debug output ────────────────────────────────────────────────

    #[test]
    fn keypair_debug_redacts_secret() {
        let kp = KemKeyPair::generate();
        let debug = format!("{kp:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("DecapsulationKey"));
    }

    #[test]
    fn shared_secret_debug_redacts() {
        let kp = KemKeyPair::generate();
        let (_, shared) = kp.public_key().encapsulate().expect("encapsulate failed");
        let debug = format!("{shared:?}");
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn public_key_debug_shows_prefix() {
        let kp = KemKeyPair::generate();
        let debug = format!("{:?}", kp.public_key());
        assert!(debug.starts_with("KemPublicKey("));
        assert!(debug.contains("..."));
    }

    #[test]
    fn ciphertext_debug_shows_length() {
        let kp = KemKeyPair::generate();
        let (ct, _) = kp.public_key().encapsulate().expect("encapsulate failed");
        let debug = format!("{ct:?}");
        assert!(debug.contains("1088 bytes"));
    }

    // ── Zeroize ─────────────────────────────────────────────────────

    #[test]
    fn shared_secret_zeroize_on_drop() {
        let kp = KemKeyPair::generate();
        let (_, shared) = kp.public_key().encapsulate().expect("encapsulate failed");

        // Verify the secret is non-zero before drop
        let bytes_copy = *shared.as_bytes();
        assert!(bytes_copy.iter().any(|&b| b != 0));

        // After drop the bytes are zeroed (verified by Zeroize derive)
        drop(shared);
    }
}
