//! Identity key management for long-term user identity.
//!
//! An identity key pair is the long-term cryptographic identity for a user,
//! combining ML-DSA-65 (signing) with X25519 (Diffie-Hellman) keys. The
//! signing key authenticates protocol messages and pre-keys, while the DH
//! key serves as the "IK" in the X3DH-inspired key exchange.

use sha2::{Digest, Sha256};

use crate::error::{CryptoError, Result};
use crate::signing::{self, SigningKeyPair, Signature, VerifyingKey, FINGERPRINT_LEN};
use crate::x25519;

/// Size of a serialized [`IdentityPublicKey`] in bytes.
///
/// Composed of the ML-DSA-65 verifying key (1952 bytes) + X25519 public
/// key (32 bytes).
pub const IDENTITY_PUBLIC_KEY_LEN: usize = signing::VERIFYING_KEY_LEN + x25519::KEY_LEN;

/// Size of a serialized [`IdentityKeyPair`] (secret) in bytes.
///
/// Composed of: signing key pair (4032 + 1952) + X25519 secret key (32).
pub const IDENTITY_KEY_PAIR_LEN: usize =
    signing::SIGNING_KEY_LEN + signing::VERIFYING_KEY_LEN + x25519::KEY_LEN;

/// A user's long-term identity key pair.
///
/// Contains an ML-DSA-65 signing key pair (for signing pre-keys and protocol
/// messages) and an X25519 key pair (the "IK" in X3DH). The secret material
/// is zeroized on drop via the underlying crate implementations.
pub struct IdentityKeyPair {
    signing_key: SigningKeyPair,
    dh_key: x25519::KeyPair,
}

impl IdentityKeyPair {
    /// Generate a new random identity key pair.
    #[must_use]
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKeyPair::generate(),
            dh_key: x25519::KeyPair::generate(),
        }
    }

    /// Extract the public half of this identity.
    #[must_use]
    pub fn public_identity(&self) -> IdentityPublicKey {
        IdentityPublicKey {
            verifying_key: self.signing_key.verifying_key(),
            dh_public_key: self.dh_key.public_key(),
        }
    }

    /// Sign arbitrary data using the ML-DSA-65 signing key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if signing fails.
    pub fn sign(&self, message: &[u8]) -> Result<Signature> {
        self.signing_key.sign(message)
    }

    /// Return the X25519 DH public key.
    #[must_use]
    pub fn dh_public_key(&self) -> x25519::PublicKey {
        self.dh_key.public_key()
    }

    /// Return a reference to the X25519 key pair.
    ///
    /// Needed for performing DH operations during the key exchange protocol.
    #[must_use]
    pub fn dh_key_pair(&self) -> &x25519::KeyPair {
        &self.dh_key
    }

    /// Serialize the identity key pair to bytes.
    ///
    /// Format: `[signing_key_pair (5984) | dh_secret_key (32)]`
    ///
    /// # Security
    ///
    /// The returned bytes contain secret key material. The caller is
    /// responsible for zeroizing them when no longer needed.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(IDENTITY_KEY_PAIR_LEN);
        bytes.extend_from_slice(&self.signing_key.to_bytes());
        bytes.extend_from_slice(&self.dh_key.secret_key().to_bytes());
        bytes
    }

    /// Deserialize an identity key pair from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// [`IDENTITY_KEY_PAIR_LEN`] bytes or the embedded keys are malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != IDENTITY_KEY_PAIR_LEN {
            return Err(CryptoError::InvalidKey(format!(
                "IdentityKeyPair: expected {IDENTITY_KEY_PAIR_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let sk_len = signing::SIGNING_KEY_LEN + signing::VERIFYING_KEY_LEN;
        let signing_key = SigningKeyPair::from_bytes(&bytes[..sk_len])?;

        let mut dh_bytes = [0u8; x25519::KEY_LEN];
        dh_bytes.copy_from_slice(&bytes[sk_len..]);
        let dh_key = x25519::KeyPair::from_secret_bytes(dh_bytes);

        Ok(Self {
            signing_key,
            dh_key,
        })
    }
}

impl std::fmt::Debug for IdentityKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityKeyPair")
            .field("signing_key", &"[REDACTED]")
            .field("dh_public_key", &self.dh_key.public_key())
            .finish()
    }
}

/// The public half of a user's identity.
///
/// Contains the ML-DSA-65 verifying key and the X25519 DH public key.
/// Can be freely shared with other users.
#[derive(Clone)]
pub struct IdentityPublicKey {
    verifying_key: VerifyingKey,
    dh_public_key: x25519::PublicKey,
}

impl IdentityPublicKey {
    /// Verify a signature on a message using the ML-DSA-65 verifying key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if verification fails.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        self.verifying_key.verify(message, signature)
    }

    /// Compute a SHA-256 fingerprint of this identity.
    ///
    /// The fingerprint is the SHA-256 hash of the concatenated verifying key
    /// and DH public key bytes. It is deterministic: the same identity always
    /// produces the same fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; FINGERPRINT_LEN] {
        let mut hasher = Sha256::new();
        hasher.update(self.verifying_key.to_bytes());
        hasher.update(self.dh_public_key.as_bytes());
        let hash = hasher.finalize();
        let mut fp = [0u8; FINGERPRINT_LEN];
        fp.copy_from_slice(&hash);
        fp
    }

    /// Return the X25519 DH public key.
    #[must_use]
    pub fn dh_public_key(&self) -> x25519::PublicKey {
        self.dh_public_key
    }

    /// Return a reference to the ML-DSA-65 verifying key.
    #[must_use]
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Serialize the identity public key to a byte vector.
    ///
    /// Format: `[verifying_key_bytes | dh_public_key_bytes]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(IDENTITY_PUBLIC_KEY_LEN);
        bytes.extend_from_slice(&self.verifying_key.to_bytes());
        bytes.extend_from_slice(self.dh_public_key.as_bytes());
        bytes
    }

    /// Deserialize an identity public key from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::InvalidKey`] if `bytes` is not exactly
    /// [`IDENTITY_PUBLIC_KEY_LEN`] bytes, or if the embedded keys are
    /// malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != IDENTITY_PUBLIC_KEY_LEN {
            return Err(CryptoError::InvalidKey(format!(
                "expected {IDENTITY_PUBLIC_KEY_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let verifying_key = VerifyingKey::from_bytes(&bytes[..signing::VERIFYING_KEY_LEN])?;

        let mut dh_bytes = [0u8; x25519::KEY_LEN];
        dh_bytes.copy_from_slice(&bytes[signing::VERIFYING_KEY_LEN..]);
        let dh_public_key = x25519::PublicKey::from_bytes(dh_bytes);

        Ok(Self {
            verifying_key,
            dh_public_key,
        })
    }
}

impl PartialEq for IdentityPublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.verifying_key == other.verifying_key && self.dh_public_key == other.dh_public_key
    }
}

impl Eq for IdentityPublicKey {}

impl std::fmt::Debug for IdentityPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityPublicKey")
            .field("verifying_key", &self.verifying_key)
            .field("dh_public_key", &self.dh_public_key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Key generation ──────────────────────────────────────────────

    #[test]
    fn generate_identity_key_pair() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();
        let bytes = public.to_bytes();
        assert_eq!(bytes.len(), IDENTITY_PUBLIC_KEY_LEN);
    }

    #[test]
    fn two_identities_differ() {
        let ikp1 = IdentityKeyPair::generate();
        let ikp2 = IdentityKeyPair::generate();
        assert_ne!(ikp1.public_identity(), ikp2.public_identity());
    }

    // ── Public key serialization round-trip ─────────────────────────

    #[test]
    fn public_key_serialization_roundtrip() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let bytes = public.to_bytes();
        let restored = IdentityPublicKey::from_bytes(&bytes).expect("deserialization failed");

        assert_eq!(public, restored);
    }

    #[test]
    fn public_key_from_invalid_length_fails() {
        let too_short = vec![0u8; 100];
        let result = IdentityPublicKey::from_bytes(&too_short);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("expected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn public_key_from_empty_fails() {
        let result = IdentityPublicKey::from_bytes(&[]);
        assert!(result.is_err());
    }

    // ── Sign / verify ───────────────────────────────────────────────

    #[test]
    fn sign_verify_roundtrip() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let msg = b"Hello, identity!";
        let sig = ikp.sign(msg).expect("signing failed");
        public.verify(msg, &sig).expect("verification failed");
    }

    #[test]
    fn sign_verify_empty_message() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let sig = ikp.sign(b"").expect("signing failed");
        public.verify(b"", &sig).expect("verification failed");
    }

    #[test]
    fn verification_fails_with_wrong_identity() {
        let ikp1 = IdentityKeyPair::generate();
        let ikp2 = IdentityKeyPair::generate();

        let msg = b"signed by ikp1";
        let sig = ikp1.sign(msg).expect("signing failed");

        let result = ikp2.public_identity().verify(msg, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn verification_fails_with_tampered_message() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let sig = ikp.sign(b"original").expect("signing failed");
        let result = public.verify(b"tampered", &sig);
        assert!(result.is_err());
    }

    // ── Fingerprint ─────────────────────────────────────────────────

    #[test]
    fn fingerprint_is_deterministic() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let fp1 = public.fingerprint();
        let fp2 = public.fingerprint();
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_differs_for_different_identities() {
        let ikp1 = IdentityKeyPair::generate();
        let ikp2 = IdentityKeyPair::generate();

        assert_ne!(
            ikp1.public_identity().fingerprint(),
            ikp2.public_identity().fingerprint()
        );
    }

    #[test]
    fn fingerprint_is_32_bytes() {
        let ikp = IdentityKeyPair::generate();
        let fp = ikp.public_identity().fingerprint();
        assert_eq!(fp.len(), FINGERPRINT_LEN);
        assert!(fp.iter().any(|&b| b != 0));
    }

    #[test]
    fn fingerprint_includes_both_keys() {
        // The fingerprint should depend on both the verifying key and the DH
        // key. We verify this indirectly: since generate() produces random
        // key pairs, two identities with different keys should always have
        // different fingerprints.
        let fps: Vec<_> = (0..5)
            .map(|_| IdentityKeyPair::generate().public_identity().fingerprint())
            .collect();

        for i in 0..fps.len() {
            for j in (i + 1)..fps.len() {
                assert_ne!(fps[i], fps[j], "fingerprints {i} and {j} collided");
            }
        }
    }

    // ── Serialized round-trip preserves functionality ────────────────

    #[test]
    fn deserialized_public_key_can_verify() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let msg = b"roundtrip verify test";
        let sig = ikp.sign(msg).expect("signing failed");

        let bytes = public.to_bytes();
        let restored = IdentityPublicKey::from_bytes(&bytes).expect("deserialization failed");

        restored.verify(msg, &sig).expect("verification with deserialized key failed");
    }

    #[test]
    fn deserialized_public_key_same_fingerprint() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        let bytes = public.to_bytes();
        let restored = IdentityPublicKey::from_bytes(&bytes).expect("deserialization failed");

        assert_eq!(public.fingerprint(), restored.fingerprint());
    }

    // ── DH key accessors ────────────────────────────────────────────

    #[test]
    fn dh_public_key_matches() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();

        assert_eq!(
            ikp.dh_public_key().to_bytes(),
            public.dh_public_key().to_bytes()
        );
    }

    #[test]
    fn dh_key_pair_accessible() {
        let ikp = IdentityKeyPair::generate();
        let pk_from_pair = ikp.dh_key_pair().public_key();
        assert_eq!(ikp.dh_public_key().to_bytes(), pk_from_pair.to_bytes());
    }

    // ── Debug output ────────────────────────────────────────────────

    #[test]
    fn identity_keypair_debug_redacts_secret() {
        let ikp = IdentityKeyPair::generate();
        let debug = format!("{ikp:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("IdentityKeyPair"));
    }

    #[test]
    fn identity_public_key_debug_shows_fields() {
        let ikp = IdentityKeyPair::generate();
        let debug = format!("{:?}", ikp.public_identity());
        assert!(debug.contains("IdentityPublicKey"));
        assert!(debug.contains("VerifyingKey"));
        assert!(debug.contains("PublicKey"));
    }

    // ── Clone and equality ──────────────────────────────────────────

    #[test]
    fn public_key_clone_is_equal() {
        let ikp = IdentityKeyPair::generate();
        let public = ikp.public_identity();
        let cloned = public.clone();
        assert_eq!(public, cloned);
    }
}
