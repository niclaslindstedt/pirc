//! ML-DSA (Dilithium) digital signature wrapper.
//!
//! Provides key generation, signing, and verification using ML-DSA-65
//! (FIPS 204). Used for identity key signing and verifying key exchange
//! messages to prevent MITM attacks.

use ml_dsa::signature::Verifier;
use ml_dsa::{KeyGen, MlDsa65};
use sha2::{Digest, Sha256};

use crate::error::{CryptoError, Result};

/// Size of an ML-DSA-65 verifying (public) key in bytes.
pub const VERIFYING_KEY_LEN: usize = 1952;

/// Size of an ML-DSA-65 signing (secret) key in bytes.
pub const SIGNING_KEY_LEN: usize = 4032;

/// Size of an ML-DSA-65 signature in bytes.
pub const SIGNATURE_LEN: usize = 3309;

/// Size of a key fingerprint (SHA-256 hash) in bytes.
pub const FINGERPRINT_LEN: usize = 32;

/// An ML-DSA-65 signing key pair.
///
/// The signing key is secret material and is zeroized on drop
/// (via the `ml-dsa` crate's `zeroize` feature).
pub struct SigningKeyPair {
    sk: ml_dsa::SigningKey<MlDsa65>,
    vk: ml_dsa::VerifyingKey<MlDsa65>,
}

impl SigningKeyPair {
    /// Generate a new random ML-DSA-65 signing key pair.
    #[must_use]
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng;
        let kp = MlDsa65::key_gen(&mut rng);
        Self {
            sk: kp.signing_key().clone(),
            vk: kp.verifying_key().clone(),
        }
    }

    /// Return the public (verifying) key.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey {
            vk: self.vk.clone(),
        }
    }

    /// Sign a message, returning an ML-DSA-65 signature.
    ///
    /// Uses the deterministic signing variant with an empty context string.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if signing fails.
    pub fn sign(&self, message: &[u8]) -> Result<Signature> {
        let sig = self
            .sk
            .sign_deterministic(message, &[])
            .map_err(|e| CryptoError::Signature(format!("signing failed: {e}")))?;
        Ok(Signature { sig })
    }

    /// Serialize the signing key pair to bytes.
    ///
    /// Format: `[signing_key (4032) | verifying_key (1952)]`
    ///
    /// # Security
    ///
    /// The returned bytes contain secret key material. The caller is
    /// responsible for zeroizing them when no longer needed.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let sk = self.sk.encode();
        let vk = self.vk.encode();
        let mut bytes = Vec::with_capacity(SIGNING_KEY_LEN + VERIFYING_KEY_LEN);
        bytes.extend_from_slice(&sk);
        bytes.extend_from_slice(&vk);
        bytes
    }

    /// Deserialize a signing key pair from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// `SIGNING_KEY_LEN + VERIFYING_KEY_LEN` bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let expected = SIGNING_KEY_LEN + VERIFYING_KEY_LEN;
        if bytes.len() != expected {
            return Err(CryptoError::InvalidKey(format!(
                "SigningKeyPair: expected {expected} bytes, got {}",
                bytes.len()
            )));
        }

        let sk_enc = <ml_dsa::EncodedSigningKey<MlDsa65>>::try_from(&bytes[..SIGNING_KEY_LEN])
            .map_err(|_| CryptoError::InvalidKey("invalid signing key length".into()))?;
        let sk = ml_dsa::SigningKey::<MlDsa65>::decode(&sk_enc);

        let vk_enc = <ml_dsa::EncodedVerifyingKey<MlDsa65>>::try_from(
            &bytes[SIGNING_KEY_LEN..],
        )
        .map_err(|_| CryptoError::InvalidKey("invalid verifying key length".into()))?;
        let vk = ml_dsa::VerifyingKey::<MlDsa65>::decode(&vk_enc);

        Ok(Self { sk, vk })
    }
}

impl std::fmt::Debug for SigningKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKeyPair")
            .field("sk", &"[REDACTED]")
            .field("vk", &VerifyingKey { vk: self.vk.clone() })
            .finish()
    }
}

/// An ML-DSA-65 verifying (public) key.
///
/// Can be freely shared. Used to verify signatures produced by the
/// corresponding [`SigningKeyPair`].
#[derive(Clone)]
pub struct VerifyingKey {
    vk: ml_dsa::VerifyingKey<MlDsa65>,
}

impl VerifyingKey {
    /// Deserialize a verifying key from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// [`VERIFYING_KEY_LEN`] bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr = <ml_dsa::EncodedVerifyingKey<MlDsa65>>::try_from(bytes)
            .map_err(|_| {
                CryptoError::InvalidKey(format!(
                    "expected {VERIFYING_KEY_LEN} bytes, got {}",
                    bytes.len()
                ))
            })?;
        let vk = ml_dsa::VerifyingKey::<MlDsa65>::decode(&arr);
        Ok(Self { vk })
    }

    /// Serialize the verifying key to a byte vector.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.vk.encode().to_vec()
    }

    /// Verify a signature on a message.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if verification fails.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        self.vk
            .verify(message, &signature.sig)
            .map_err(|e| CryptoError::Signature(format!("verification failed: {e}")))
    }

    /// Compute a SHA-256 fingerprint of this verifying key.
    ///
    /// The fingerprint is deterministic: the same key always produces the
    /// same fingerprint. Useful for identity verification without comparing
    /// full key bytes.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; FINGERPRINT_LEN] {
        let key_bytes = self.vk.encode();
        let hash = Sha256::digest(key_bytes);
        let mut fp = [0u8; FINGERPRINT_LEN];
        fp.copy_from_slice(&hash);
        fp
    }
}

impl PartialEq for VerifyingKey {
    fn eq(&self, other: &Self) -> bool {
        self.vk == other.vk
    }
}

impl Eq for VerifyingKey {}

impl std::fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.vk.encode();
        write!(f, "VerifyingKey({:02x?}...)", &bytes[..8])
    }
}

/// An ML-DSA-65 signature.
#[derive(Clone)]
pub struct Signature {
    sig: ml_dsa::Signature<MlDsa65>,
}

impl Signature {
    /// Deserialize a signature from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if `bytes` is not a valid
    /// ML-DSA-65 signature.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let sig = ml_dsa::Signature::<MlDsa65>::try_from(bytes)
            .map_err(|e| CryptoError::Signature(format!("invalid signature: {e}")))?;
        Ok(Self { sig })
    }

    /// Serialize the signature to a byte vector.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.sig.encode().to_vec()
    }
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signature({SIGNATURE_LEN} bytes)")
    }
}

/// Sign a message using the given key pair.
///
/// Convenience function that delegates to [`SigningKeyPair::sign`].
///
/// # Errors
///
/// Returns [`CryptoError::Signature`] if signing fails.
pub fn sign(key_pair: &SigningKeyPair, message: &[u8]) -> Result<Signature> {
    key_pair.sign(message)
}

/// Verify a signature on a message using the given verifying key.
///
/// Convenience function that delegates to [`VerifyingKey::verify`].
///
/// # Errors
///
/// Returns [`CryptoError::Signature`] if verification fails.
pub fn verify(verifying_key: &VerifyingKey, message: &[u8], signature: &Signature) -> Result<()> {
    verifying_key.verify(message, signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Key generation ──────────────────────────────────────────────

    #[test]
    fn key_pair_generation() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();
        let vk_bytes = vk.to_bytes();
        assert_eq!(vk_bytes.len(), VERIFYING_KEY_LEN);
        // Verifying key should not be all zeros
        assert!(vk_bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn two_key_pairs_differ() {
        let kp1 = SigningKeyPair::generate();
        let kp2 = SigningKeyPair::generate();
        assert_ne!(kp1.verifying_key().to_bytes(), kp2.verifying_key().to_bytes());
    }

    // ── Sign / verify round-trip ────────────────────────────────────

    #[test]
    fn sign_verify_roundtrip() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let msg = b"Hello, post-quantum world!";
        let sig = kp.sign(msg).expect("signing failed");
        vk.verify(msg, &sig).expect("verification failed");
    }

    #[test]
    fn convenience_functions_roundtrip() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let msg = b"convenience test";
        let sig = sign(&kp, msg).expect("signing failed");
        verify(&vk, msg, &sig).expect("verification failed");
    }

    #[test]
    fn sign_verify_empty_message() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let sig = kp.sign(b"").expect("signing failed");
        vk.verify(b"", &sig).expect("verification failed");
    }

    #[test]
    fn sign_verify_large_message() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let msg = vec![0xABu8; 10_000];
        let sig = kp.sign(&msg).expect("signing failed");
        vk.verify(&msg, &sig).expect("verification failed");
    }

    #[test]
    fn signature_is_correct_length() {
        let kp = SigningKeyPair::generate();
        let sig = kp.sign(b"test").expect("signing failed");
        assert_eq!(sig.to_bytes().len(), SIGNATURE_LEN);
    }

    // ── Wrong key ───────────────────────────────────────────────────

    #[test]
    fn verification_fails_with_wrong_key() {
        let kp1 = SigningKeyPair::generate();
        let kp2 = SigningKeyPair::generate();

        let msg = b"signed by kp1";
        let sig = kp1.sign(msg).expect("signing failed");

        let result = kp2.verifying_key().verify(msg, &sig);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("verification failed"),
            "unexpected error: {err}"
        );
    }

    // ── Tampered message ────────────────────────────────────────────

    #[test]
    fn verification_fails_with_tampered_message() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let msg = b"original message";
        let sig = kp.sign(msg).expect("signing failed");

        let result = vk.verify(b"tampered message", &sig);
        assert!(result.is_err());
    }

    // ── Tampered signature ──────────────────────────────────────────

    #[test]
    fn verification_fails_with_tampered_signature() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let msg = b"test message";
        let sig = kp.sign(msg).expect("signing failed");

        let mut sig_bytes = sig.to_bytes();
        // Flip a bit in the signature
        sig_bytes[0] ^= 0xFF;

        // Tampered signature may fail to deserialize or fail verification
        if let Ok(tampered_sig) = Signature::from_bytes(&sig_bytes) {
            let result = vk.verify(msg, &tampered_sig);
            assert!(result.is_err());
        }
        // If deserialization fails, that's also correct behavior
    }

    // ── Fingerprint ─────────────────────────────────────────────────

    #[test]
    fn fingerprint_is_deterministic() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let fp1 = vk.fingerprint();
        let fp2 = vk.fingerprint();
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_differs_for_different_keys() {
        let kp1 = SigningKeyPair::generate();
        let kp2 = SigningKeyPair::generate();

        assert_ne!(
            kp1.verifying_key().fingerprint(),
            kp2.verifying_key().fingerprint()
        );
    }

    #[test]
    fn fingerprint_is_32_bytes() {
        let kp = SigningKeyPair::generate();
        let fp = kp.verifying_key().fingerprint();
        assert_eq!(fp.len(), FINGERPRINT_LEN);
        // Should not be all zeros
        assert!(fp.iter().any(|&b| b != 0));
    }

    // ── Serialization round-trips ───────────────────────────────────

    #[test]
    fn verifying_key_serialization_roundtrip() {
        let kp = SigningKeyPair::generate();
        let vk = kp.verifying_key();

        let bytes = vk.to_bytes();
        let vk2 = VerifyingKey::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(vk, vk2);
        assert_eq!(vk.to_bytes(), vk2.to_bytes());
    }

    #[test]
    fn signature_serialization_roundtrip() {
        let kp = SigningKeyPair::generate();
        let msg = b"roundtrip test";
        let sig = kp.sign(msg).expect("signing failed");

        let sig_bytes = sig.to_bytes();
        assert_eq!(sig_bytes.len(), SIGNATURE_LEN);

        let sig2 = Signature::from_bytes(&sig_bytes).expect("deserialize failed");
        kp.verifying_key()
            .verify(msg, &sig2)
            .expect("verification with deserialized sig failed");
    }

    #[test]
    fn verifying_key_from_invalid_length_fails() {
        let too_short = vec![0u8; 100];
        let result = VerifyingKey::from_bytes(&too_short);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expected"), "unexpected error: {err}");
    }

    #[test]
    fn signature_from_invalid_length_fails() {
        let too_short = vec![0u8; 100];
        let result = Signature::from_bytes(&too_short);
        assert!(result.is_err());
    }

    // ── Debug output ────────────────────────────────────────────────

    #[test]
    fn keypair_debug_redacts_secret() {
        let kp = SigningKeyPair::generate();
        let debug = format!("{kp:?}");
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn verifying_key_debug_shows_prefix() {
        let kp = SigningKeyPair::generate();
        let debug = format!("{:?}", kp.verifying_key());
        assert!(debug.starts_with("VerifyingKey("));
        assert!(debug.contains("..."));
    }

    #[test]
    fn signature_debug_shows_length() {
        let kp = SigningKeyPair::generate();
        let sig = kp.sign(b"test").expect("signing failed");
        let debug = format!("{sig:?}");
        assert!(debug.contains("3309 bytes"));
    }
}
