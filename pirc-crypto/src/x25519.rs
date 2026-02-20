//! X25519 Diffie-Hellman key agreement wrapper.
//!
//! Provides key pair generation and shared secret computation using the
//! X25519 elliptic-curve Diffie-Hellman function. Used as the classical
//! key exchange component of the DH ratchet.

use rand::rngs::OsRng;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};

/// Size of an X25519 public key or secret key in bytes.
pub const KEY_LEN: usize = 32;

/// An X25519 public key.
///
/// This is a newtype around the 32-byte Curve25519 point. It supports
/// constant-time equality comparison and serialization to/from a byte array.
#[derive(Clone, Copy)]
pub struct PublicKey(x25519_dalek::PublicKey);

impl PublicKey {
    /// Deserialize a public key from a 32-byte array.
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(x25519_dalek::PublicKey::from(bytes))
    }

    /// Serialize the public key to a 32-byte array.
    #[must_use]
    pub fn to_bytes(self) -> [u8; KEY_LEN] {
        self.0.to_bytes()
    }

    /// Return a reference to the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        self.0.as_bytes()
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes().ct_eq(other.as_bytes()).into()
    }
}

impl Eq for PublicKey {}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PublicKey({:02x?})", &self.as_bytes()[..8])
    }
}

impl From<x25519_dalek::PublicKey> for PublicKey {
    fn from(inner: x25519_dalek::PublicKey) -> Self {
        Self(inner)
    }
}

/// An X25519 secret key.
///
/// Wraps the raw 32-byte secret scalar. Implements [`Zeroize`] and
/// [`ZeroizeOnDrop`] so the key material is erased when no longer needed.
pub struct SecretKey {
    bytes: zeroize::Zeroizing<[u8; KEY_LEN]>,
}

impl SecretKey {
    /// Deserialize a secret key from a 32-byte array.
    ///
    /// The input bytes are copied into zeroizing storage. The caller is
    /// responsible for clearing their copy if needed.
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self {
            bytes: zeroize::Zeroizing::new(bytes),
        }
    }

    /// Serialize the secret key to a 32-byte array.
    ///
    /// # Security
    ///
    /// The caller is responsible for zeroizing the returned bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; KEY_LEN] {
        *self.bytes
    }

    /// Return a reference to the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }
}

/// An X25519 key pair (secret key + public key).
///
/// Generated from a CSPRNG. The secret key is zeroized on drop.
pub struct KeyPair {
    secret: x25519_dalek::StaticSecret,
    public: PublicKey,
}

impl KeyPair {
    /// Generate a new random X25519 key pair.
    #[must_use]
    pub fn generate() -> Self {
        let secret = x25519_dalek::StaticSecret::random_from_rng(OsRng);
        let public = PublicKey(x25519_dalek::PublicKey::from(&secret));
        Self { secret, public }
    }

    /// Return the public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        self.public
    }

    /// Return the secret key.
    ///
    /// The returned [`SecretKey`] wraps the raw bytes in zeroizing storage.
    #[must_use]
    pub fn secret_key(&self) -> SecretKey {
        SecretKey::from_bytes(self.secret.to_bytes())
    }

    /// Reconstruct a key pair from raw secret key bytes.
    ///
    /// Derives the public key from the secret key.
    #[must_use]
    pub fn from_secret_bytes(secret_bytes: [u8; KEY_LEN]) -> Self {
        let secret = x25519_dalek::StaticSecret::from(secret_bytes);
        let public = PublicKey(x25519_dalek::PublicKey::from(&secret));
        Self { secret, public }
    }
}

/// The shared secret resulting from an X25519 Diffie-Hellman key agreement.
///
/// This is a 32-byte value derived from one party's secret key and the other
/// party's public key. Implements [`Zeroize`] and [`ZeroizeOnDrop`] for
/// automatic key erasure.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SharedSecret {
    bytes: [u8; KEY_LEN],
}

impl std::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSecret")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl SharedSecret {
    /// Return a reference to the 32-byte shared secret.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }
}

/// Perform X25519 Diffie-Hellman key agreement.
///
/// Computes a shared secret from `our_secret` and `their_public`.
///
/// # Errors
///
/// Returns [`CryptoError::KeyExchange`] if the resulting shared secret is
/// all zeros (indicating a low-order public key contribution).
pub fn diffie_hellman(our_secret: &SecretKey, their_public: &PublicKey) -> Result<SharedSecret> {
    // Copy secret bytes into a Zeroizing wrapper so the intermediate
    // stack copy is erased even if StaticSecret::from() copies again.
    let mut secret_copy = *our_secret.as_bytes();
    let dalek_secret = x25519_dalek::StaticSecret::from(secret_copy);
    secret_copy.zeroize();
    let raw = dalek_secret.diffie_hellman(&their_public.0);
    let bytes: [u8; KEY_LEN] = raw.to_bytes();
    if bytes.ct_eq(&[0u8; KEY_LEN]).into() {
        return Err(CryptoError::KeyExchange(
            "DH produced all-zero shared secret (low-order point)".into(),
        ));
    }
    Ok(SharedSecret { bytes })
}

/// Perform X25519 Diffie-Hellman key agreement using a [`KeyPair`] directly.
///
/// Convenience wrapper that extracts the secret key from the key pair.
///
/// # Errors
///
/// Returns [`CryptoError::KeyExchange`] if the resulting shared secret is
/// all zeros.
pub fn diffie_hellman_keypair(our_keys: &KeyPair, their_public: &PublicKey) -> Result<SharedSecret> {
    let raw = our_keys.secret.diffie_hellman(&their_public.0);
    let bytes: [u8; KEY_LEN] = raw.to_bytes();
    if bytes.ct_eq(&[0u8; KEY_LEN]).into() {
        return Err(CryptoError::KeyExchange(
            "DH produced all-zero shared secret (low-order point)".into(),
        ));
    }
    Ok(SharedSecret { bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_pair_generation() {
        let kp = KeyPair::generate();
        // Public key should be 32 bytes and non-zero
        let pk_bytes = kp.public_key().to_bytes();
        assert_eq!(pk_bytes.len(), KEY_LEN);
        assert!(pk_bytes.iter().any(|&b| b != 0));

        // Secret key should be 32 bytes and non-zero
        let sk_bytes = kp.secret_key().to_bytes();
        assert_eq!(sk_bytes.len(), KEY_LEN);
        assert!(sk_bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn two_key_pairs_differ() {
        let kp1 = KeyPair::generate();
        let kp2 = KeyPair::generate();
        assert_ne!(kp1.public_key().to_bytes(), kp2.public_key().to_bytes());
    }

    #[test]
    fn dh_agreement_produces_matching_secrets() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let alice_shared =
            diffie_hellman(&alice.secret_key(), &bob.public_key()).expect("DH failed");
        let bob_shared =
            diffie_hellman(&bob.secret_key(), &alice.public_key()).expect("DH failed");

        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }

    #[test]
    fn dh_agreement_via_keypair() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let alice_shared =
            diffie_hellman_keypair(&alice, &bob.public_key()).expect("DH failed");
        let bob_shared =
            diffie_hellman_keypair(&bob, &alice.public_key()).expect("DH failed");

        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }

    #[test]
    fn different_pairs_produce_different_secrets() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let charlie = KeyPair::generate();

        let ab = diffie_hellman(&alice.secret_key(), &bob.public_key()).expect("DH failed");
        let ac = diffie_hellman(&alice.secret_key(), &charlie.public_key()).expect("DH failed");

        assert_ne!(ab.as_bytes(), ac.as_bytes());
    }

    #[test]
    fn public_key_serialization_roundtrip() {
        let kp = KeyPair::generate();
        let pk = kp.public_key();

        let bytes = pk.to_bytes();
        let pk2 = PublicKey::from_bytes(bytes);

        assert_eq!(pk, pk2);
        assert_eq!(pk.to_bytes(), pk2.to_bytes());
    }

    #[test]
    fn secret_key_serialization_roundtrip() {
        let kp = KeyPair::generate();
        let sk = kp.secret_key();

        let bytes = sk.to_bytes();
        let sk2 = SecretKey::from_bytes(bytes);

        // Re-derive the public key from the deserialized secret to verify
        // it produces the same result
        let dalek_secret = x25519_dalek::StaticSecret::from(*sk2.as_bytes());
        let pk_from_sk = PublicKey(x25519_dalek::PublicKey::from(&dalek_secret));
        assert_eq!(kp.public_key(), pk_from_sk);
    }

    #[test]
    fn public_key_constant_time_equality() {
        let kp = KeyPair::generate();
        let pk = kp.public_key();

        // Same key should be equal
        let pk_copy = PublicKey::from_bytes(pk.to_bytes());
        assert_eq!(pk, pk_copy);

        // Different key should not be equal
        let kp2 = KeyPair::generate();
        assert_ne!(pk, kp2.public_key());
    }

    #[test]
    fn public_key_debug_does_not_leak_full_key() {
        let kp = KeyPair::generate();
        let debug_str = format!("{:?}", kp.public_key());
        assert!(debug_str.starts_with("PublicKey("));
        // Only shows first 8 bytes, not all 32
        assert!(debug_str.len() < 100);
    }

    #[test]
    fn shared_secret_as_bytes() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let shared = diffie_hellman(&alice.secret_key(), &bob.public_key()).expect("DH failed");
        let bytes = shared.as_bytes();
        assert_eq!(bytes.len(), KEY_LEN);
        // Shared secret should not be all zeros for well-formed keys
        assert!(bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn low_order_point_produces_error() {
        // The all-zeros public key is a low-order point on Curve25519.
        // DH with it should produce an all-zero shared secret.
        let kp = KeyPair::generate();
        let low_order = PublicKey::from_bytes([0u8; KEY_LEN]);

        let result = diffie_hellman(&kp.secret_key(), &low_order);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("all-zero"),
            "unexpected error: {err}"
        );
    }
}
