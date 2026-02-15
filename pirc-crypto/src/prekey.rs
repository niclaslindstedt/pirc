//! Pre-key bundle types for X3DH-inspired key exchange with PQ extension.
//!
//! A pre-key bundle is what a user publishes so that others can initiate
//! key exchanges asynchronously (even when the user is offline). The bundle
//! contains:
//!
//! - **Signed pre-key** — a medium-term X25519 key pair signed by the
//!   identity key, rotated periodically
//! - **One-time pre-key** — an ephemeral X25519 key pair used exactly once
//! - **KEM pre-key** — a post-quantum ML-KEM key pair signed by the
//!   identity key, providing hybrid key exchange

use crate::error::{CryptoError, Result};
use crate::identity::{IdentityKeyPair, IdentityPublicKey, IDENTITY_PUBLIC_KEY_LEN};
use crate::kem::{self, KemKeyPair, KemPublicKey};
use crate::signing::{self, Signature};
use crate::x25519;

/// Size of a serialized [`SignedPreKeyPublic`] in bytes.
///
/// Composed of: id (4) + public key (32) + signature (3309) + timestamp (8).
pub const SIGNED_PRE_KEY_PUBLIC_LEN: usize = 4 + x25519::KEY_LEN + signing::SIGNATURE_LEN + 8;

/// Size of a serialized [`OneTimePreKeyPublic`] in bytes.
///
/// Composed of: id (4) + public key (32).
pub const ONE_TIME_PRE_KEY_PUBLIC_LEN: usize = 4 + x25519::KEY_LEN;

/// Size of a serialized [`KemPreKeyPublic`] in bytes.
///
/// Composed of: id (4) + public key (1184) + signature (3309).
pub const KEM_PRE_KEY_PUBLIC_LEN: usize = 4 + kem::PUBLIC_KEY_LEN + signing::SIGNATURE_LEN;

// ── Signed pre-key (full, secret) ──────────────────────────────────────

/// A medium-term X25519 key pair, signed by the identity key.
///
/// The signature covers the public key bytes, binding this pre-key to
/// the identity. Rotated periodically (e.g. weekly). The secret key is
/// zeroized on drop via the underlying X25519 key pair.
pub struct SignedPreKey {
    id: u32,
    key_pair: x25519::KeyPair,
    signature: Signature,
    timestamp: u64,
}

impl SignedPreKey {
    /// Generate a new signed pre-key.
    ///
    /// Signs the X25519 public key bytes with the identity signing key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if signing fails.
    pub fn generate(id: u32, identity: &IdentityKeyPair, timestamp: u64) -> Result<Self> {
        let key_pair = x25519::KeyPair::generate();
        let signature = identity.sign(key_pair.public_key().as_bytes())?;
        Ok(Self {
            id,
            key_pair,
            signature,
            timestamp,
        })
    }

    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the generation timestamp.
    #[must_use]
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Return the X25519 public key.
    #[must_use]
    pub fn public_key(&self) -> x25519::PublicKey {
        self.key_pair.public_key()
    }

    /// Return a reference to the X25519 key pair.
    #[must_use]
    pub fn key_pair(&self) -> &x25519::KeyPair {
        &self.key_pair
    }

    /// Verify this pre-key's signature against an identity public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if verification fails.
    pub fn verify(&self, identity_public: &IdentityPublicKey) -> Result<()> {
        identity_public.verify(self.key_pair.public_key().as_bytes(), &self.signature)
    }

    /// Extract the public portion of this signed pre-key.
    #[must_use]
    pub fn to_public(&self) -> SignedPreKeyPublic {
        SignedPreKeyPublic {
            id: self.id,
            public_key: self.key_pair.public_key(),
            signature: self.signature.clone(),
            timestamp: self.timestamp,
        }
    }

    /// Serialize the signed pre-key (including secret key) to bytes.
    ///
    /// Format: `[id (4 LE) | secret_key (32) | signature (3309) | timestamp (8 LE)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + x25519::KEY_LEN + signing::SIGNATURE_LEN + 8);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&self.key_pair.secret_key().to_bytes());
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes
    }

    /// Deserialize a signed pre-key from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let expected = 4 + x25519::KEY_LEN + signing::SIGNATURE_LEN + 8;
        if bytes.len() != expected {
            return Err(CryptoError::Serialization(format!(
                "SignedPreKey: expected {expected} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let id = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let mut sk_bytes = [0u8; x25519::KEY_LEN];
        sk_bytes.copy_from_slice(&bytes[offset..offset + x25519::KEY_LEN]);
        let key_pair = x25519::KeyPair::from_secret_bytes(sk_bytes);
        offset += x25519::KEY_LEN;

        let signature = Signature::from_bytes(&bytes[offset..offset + signing::SIGNATURE_LEN])?;
        offset += signing::SIGNATURE_LEN;

        let timestamp = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());

        Ok(Self {
            id,
            key_pair,
            signature,
            timestamp,
        })
    }
}

impl std::fmt::Debug for SignedPreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedPreKey")
            .field("id", &self.id)
            .field("key_pair", &"[REDACTED]")
            .field("signature", &self.signature)
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

// ── Signed pre-key (public) ────────────────────────────────────────────

/// The public portion of a signed pre-key.
///
/// Contains the X25519 public key, the ML-DSA signature binding it to
/// an identity, and a timestamp for rotation tracking.
#[derive(Clone)]
pub struct SignedPreKeyPublic {
    id: u32,
    public_key: x25519::PublicKey,
    signature: Signature,
    timestamp: u64,
}

impl SignedPreKeyPublic {
    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the X25519 public key.
    #[must_use]
    pub fn public_key(&self) -> x25519::PublicKey {
        self.public_key
    }

    /// Return the generation timestamp.
    #[must_use]
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Verify this pre-key's signature against an identity public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if verification fails.
    pub fn verify(&self, identity_public: &IdentityPublicKey) -> Result<()> {
        identity_public.verify(self.public_key.as_bytes(), &self.signature)
    }

    /// Serialize to a byte vector.
    ///
    /// Format: `[id (4 LE) | public_key (32) | signature (3309) | timestamp (8 LE)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(SIGNED_PRE_KEY_PUBLIC_LEN);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(self.public_key.as_bytes());
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes
    }

    /// Deserialize from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the slice length is wrong
    /// or the embedded signature is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != SIGNED_PRE_KEY_PUBLIC_LEN {
            return Err(CryptoError::Serialization(format!(
                "SignedPreKeyPublic: expected {SIGNED_PRE_KEY_PUBLIC_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let id = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let mut pk_bytes = [0u8; x25519::KEY_LEN];
        pk_bytes.copy_from_slice(&bytes[offset..offset + x25519::KEY_LEN]);
        let public_key = x25519::PublicKey::from_bytes(pk_bytes);
        offset += x25519::KEY_LEN;

        let signature = Signature::from_bytes(&bytes[offset..offset + signing::SIGNATURE_LEN])?;
        offset += signing::SIGNATURE_LEN;

        let timestamp = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());

        Ok(Self {
            id,
            public_key,
            signature,
            timestamp,
        })
    }
}

impl std::fmt::Debug for SignedPreKeyPublic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedPreKeyPublic")
            .field("id", &self.id)
            .field("public_key", &self.public_key)
            .field("signature", &self.signature)
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

// ── One-time pre-key (full, secret) ────────────────────────────────────

/// An ephemeral X25519 key pair used exactly once.
///
/// One-time pre-keys provide forward secrecy for the initial key exchange.
/// After use, the key pair should be discarded. The secret key is zeroized
/// on drop via the underlying X25519 key pair.
pub struct OneTimePreKey {
    id: u32,
    key_pair: x25519::KeyPair,
}

impl OneTimePreKey {
    /// Generate a new one-time pre-key.
    #[must_use]
    pub fn generate(id: u32) -> Self {
        Self {
            id,
            key_pair: x25519::KeyPair::generate(),
        }
    }

    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the X25519 public key.
    #[must_use]
    pub fn public_key(&self) -> x25519::PublicKey {
        self.key_pair.public_key()
    }

    /// Return a reference to the X25519 key pair.
    #[must_use]
    pub fn key_pair(&self) -> &x25519::KeyPair {
        &self.key_pair
    }

    /// Extract the public portion.
    #[must_use]
    pub fn to_public(&self) -> OneTimePreKeyPublic {
        OneTimePreKeyPublic {
            id: self.id,
            public_key: self.key_pair.public_key(),
        }
    }

    /// Serialize the one-time pre-key (including secret key) to bytes.
    ///
    /// Format: `[id (4 LE) | secret_key (32)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + x25519::KEY_LEN);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&self.key_pair.secret_key().to_bytes());
        bytes
    }

    /// Deserialize a one-time pre-key from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let expected = 4 + x25519::KEY_LEN;
        if bytes.len() != expected {
            return Err(CryptoError::Serialization(format!(
                "OneTimePreKey: expected {expected} bytes, got {}",
                bytes.len()
            )));
        }

        let id = u32::from_le_bytes(bytes[..4].try_into().unwrap());

        let mut sk_bytes = [0u8; x25519::KEY_LEN];
        sk_bytes.copy_from_slice(&bytes[4..]);
        let key_pair = x25519::KeyPair::from_secret_bytes(sk_bytes);

        Ok(Self { id, key_pair })
    }
}

impl std::fmt::Debug for OneTimePreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneTimePreKey")
            .field("id", &self.id)
            .field("key_pair", &"[REDACTED]")
            .finish()
    }
}

// ── One-time pre-key (public) ──────────────────────────────────────────

/// The public half of a one-time pre-key.
#[derive(Clone)]
pub struct OneTimePreKeyPublic {
    id: u32,
    public_key: x25519::PublicKey,
}

impl OneTimePreKeyPublic {
    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the X25519 public key.
    #[must_use]
    pub fn public_key(&self) -> x25519::PublicKey {
        self.public_key
    }

    /// Serialize to a byte vector.
    ///
    /// Format: `[id (4 LE) | public_key (32)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(ONE_TIME_PRE_KEY_PUBLIC_LEN);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(self.public_key.as_bytes());
        bytes
    }

    /// Deserialize from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the slice length is wrong.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != ONE_TIME_PRE_KEY_PUBLIC_LEN {
            return Err(CryptoError::Serialization(format!(
                "OneTimePreKeyPublic: expected {ONE_TIME_PRE_KEY_PUBLIC_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let id = u32::from_le_bytes(bytes[..4].try_into().unwrap());

        let mut pk_bytes = [0u8; x25519::KEY_LEN];
        pk_bytes.copy_from_slice(&bytes[4..]);
        let public_key = x25519::PublicKey::from_bytes(pk_bytes);

        Ok(Self { id, public_key })
    }
}

impl std::fmt::Debug for OneTimePreKeyPublic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneTimePreKeyPublic")
            .field("id", &self.id)
            .field("public_key", &self.public_key)
            .finish()
    }
}

// ── KEM pre-key (full, secret) ─────────────────────────────────────────

/// A post-quantum ML-KEM key pair for hybrid key exchange.
///
/// The KEM public key is signed by the identity key to prevent
/// substitution attacks. The decapsulation key is zeroized on drop
/// via the underlying ML-KEM key pair.
pub struct KemPreKey {
    id: u32,
    kem_pair: KemKeyPair,
    signature: Signature,
}

impl KemPreKey {
    /// Generate a new KEM pre-key, signed by the identity key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if signing the KEM public key fails.
    pub fn generate(id: u32, identity: &IdentityKeyPair) -> Result<Self> {
        let kem_pair = KemKeyPair::generate();
        let signature = identity.sign(&kem_pair.public_key().to_bytes())?;
        Ok(Self {
            id,
            kem_pair,
            signature,
        })
    }

    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the KEM public key.
    #[must_use]
    pub fn public_key(&self) -> KemPublicKey {
        self.kem_pair.public_key()
    }

    /// Return a reference to the KEM key pair.
    #[must_use]
    pub fn kem_pair(&self) -> &KemKeyPair {
        &self.kem_pair
    }

    /// Extract the public portion.
    #[must_use]
    pub fn to_public(&self) -> KemPreKeyPublic {
        KemPreKeyPublic {
            id: self.id,
            public_key: self.kem_pair.public_key(),
            signature: self.signature.clone(),
        }
    }

    /// Serialize the KEM pre-key (including secret key) to bytes.
    ///
    /// Format: `[id (4 LE) | kem_pair (3584) | signature (3309)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let kem_bytes = self.kem_pair.to_bytes();
        let mut bytes = Vec::with_capacity(4 + kem_bytes.len() + signing::SIGNATURE_LEN);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&kem_bytes);
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes
    }

    /// Deserialize a KEM pre-key from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let kem_pair_len = kem::SECRET_KEY_LEN + kem::PUBLIC_KEY_LEN;
        let expected = 4 + kem_pair_len + signing::SIGNATURE_LEN;
        if bytes.len() != expected {
            return Err(CryptoError::Serialization(format!(
                "KemPreKey: expected {expected} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let id = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let kem_pair = KemKeyPair::from_bytes(&bytes[offset..offset + kem_pair_len])?;
        offset += kem_pair_len;

        let signature = Signature::from_bytes(&bytes[offset..offset + signing::SIGNATURE_LEN])?;

        Ok(Self {
            id,
            kem_pair,
            signature,
        })
    }
}

impl std::fmt::Debug for KemPreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KemPreKey")
            .field("id", &self.id)
            .field("kem_pair", &"[REDACTED]")
            .field("signature", &self.signature)
            .finish()
    }
}

// ── KEM pre-key (public) ──────────────────────────────────────────────

/// Public half of a KEM pre-key.
///
/// Contains the ML-KEM public key and the ML-DSA signature binding it
/// to an identity.
#[derive(Clone)]
pub struct KemPreKeyPublic {
    id: u32,
    public_key: KemPublicKey,
    signature: Signature,
}

impl KemPreKeyPublic {
    /// Return the pre-key identifier.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Return the KEM public key.
    #[must_use]
    pub fn public_key(&self) -> KemPublicKey {
        self.public_key.clone()
    }

    /// Verify this pre-key's signature against an identity public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if verification fails.
    pub fn verify(&self, identity_public: &IdentityPublicKey) -> Result<()> {
        identity_public.verify(&self.public_key.to_bytes(), &self.signature)
    }

    /// Serialize to a byte vector.
    ///
    /// Format: `[id (4 LE) | public_key (1184) | signature (3309)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(KEM_PRE_KEY_PUBLIC_LEN);
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&self.public_key.to_bytes());
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes
    }

    /// Deserialize from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the slice length is wrong
    /// or the embedded keys/signature are malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEM_PRE_KEY_PUBLIC_LEN {
            return Err(CryptoError::Serialization(format!(
                "KemPreKeyPublic: expected {KEM_PRE_KEY_PUBLIC_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let id = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let public_key = KemPublicKey::from_bytes(&bytes[offset..offset + kem::PUBLIC_KEY_LEN])?;
        offset += kem::PUBLIC_KEY_LEN;

        let signature = Signature::from_bytes(&bytes[offset..offset + signing::SIGNATURE_LEN])?;

        Ok(Self {
            id,
            public_key,
            signature,
        })
    }
}

impl std::fmt::Debug for KemPreKeyPublic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KemPreKeyPublic")
            .field("id", &self.id)
            .field("public_key", &self.public_key)
            .field("signature", &self.signature)
            .finish()
    }
}

// ── Pre-key bundle ─────────────────────────────────────────────────────

/// Complete bundle published for others to initiate key exchange.
///
/// Contains the identity public key, a signed pre-key, a KEM pre-key
/// (post-quantum extension), and an optional one-time pre-key. The
/// one-time pre-key may be absent if the server's supply is exhausted.
pub struct PreKeyBundle {
    identity_public: IdentityPublicKey,
    signed_pre_key: SignedPreKeyPublic,
    kem_pre_key: KemPreKeyPublic,
    one_time_pre_key: Option<OneTimePreKeyPublic>,
}

impl PreKeyBundle {
    /// Create a new pre-key bundle.
    #[must_use]
    pub fn new(
        identity_public: IdentityPublicKey,
        signed_pre_key: SignedPreKeyPublic,
        kem_pre_key: KemPreKeyPublic,
        one_time_pre_key: Option<OneTimePreKeyPublic>,
    ) -> Self {
        Self {
            identity_public,
            signed_pre_key,
            kem_pre_key,
            one_time_pre_key,
        }
    }

    /// Return a reference to the identity public key.
    #[must_use]
    pub fn identity_public(&self) -> &IdentityPublicKey {
        &self.identity_public
    }

    /// Return a reference to the signed pre-key.
    #[must_use]
    pub fn signed_pre_key(&self) -> &SignedPreKeyPublic {
        &self.signed_pre_key
    }

    /// Return a reference to the KEM pre-key.
    #[must_use]
    pub fn kem_pre_key(&self) -> &KemPreKeyPublic {
        &self.kem_pre_key
    }

    /// Return a reference to the optional one-time pre-key.
    #[must_use]
    pub fn one_time_pre_key(&self) -> Option<&OneTimePreKeyPublic> {
        self.one_time_pre_key.as_ref()
    }

    /// Validate all signatures in this bundle.
    ///
    /// Verifies the signed pre-key and KEM pre-key signatures against
    /// the embedded identity public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Signature`] if any signature is invalid.
    pub fn validate(&self) -> Result<()> {
        self.signed_pre_key.verify(&self.identity_public)?;
        self.kem_pre_key.verify(&self.identity_public)?;
        Ok(())
    }

    /// Serialize the bundle to a byte vector.
    ///
    /// Format:
    /// ```text
    /// [identity_public | signed_pre_key | kem_pre_key | has_otpk (1) | otpk?]
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let base_len =
            IDENTITY_PUBLIC_KEY_LEN + SIGNED_PRE_KEY_PUBLIC_LEN + KEM_PRE_KEY_PUBLIC_LEN + 1;
        let total_len = base_len
            + if self.one_time_pre_key.is_some() {
                ONE_TIME_PRE_KEY_PUBLIC_LEN
            } else {
                0
            };

        let mut bytes = Vec::with_capacity(total_len);
        bytes.extend_from_slice(&self.identity_public.to_bytes());
        bytes.extend_from_slice(&self.signed_pre_key.to_bytes());
        bytes.extend_from_slice(&self.kem_pre_key.to_bytes());

        if let Some(otpk) = &self.one_time_pre_key {
            bytes.push(1);
            bytes.extend_from_slice(&otpk.to_bytes());
        } else {
            bytes.push(0);
        }

        bytes
    }

    /// Deserialize a bundle from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is too short,
    /// or a sub-key error if any embedded key/signature is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let min_len =
            IDENTITY_PUBLIC_KEY_LEN + SIGNED_PRE_KEY_PUBLIC_LEN + KEM_PRE_KEY_PUBLIC_LEN + 1;
        if bytes.len() < min_len {
            return Err(CryptoError::Serialization(format!(
                "PreKeyBundle: expected at least {min_len} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let identity_public =
            IdentityPublicKey::from_bytes(&bytes[offset..offset + IDENTITY_PUBLIC_KEY_LEN])?;
        offset += IDENTITY_PUBLIC_KEY_LEN;

        let signed_pre_key =
            SignedPreKeyPublic::from_bytes(&bytes[offset..offset + SIGNED_PRE_KEY_PUBLIC_LEN])?;
        offset += SIGNED_PRE_KEY_PUBLIC_LEN;

        let kem_pre_key =
            KemPreKeyPublic::from_bytes(&bytes[offset..offset + KEM_PRE_KEY_PUBLIC_LEN])?;
        offset += KEM_PRE_KEY_PUBLIC_LEN;

        let has_otpk = bytes[offset];
        offset += 1;

        let one_time_pre_key = if has_otpk == 1 {
            if bytes.len() < offset + ONE_TIME_PRE_KEY_PUBLIC_LEN {
                return Err(CryptoError::Serialization(format!(
                    "PreKeyBundle: expected {} bytes for OTPK, got {}",
                    ONE_TIME_PRE_KEY_PUBLIC_LEN,
                    bytes.len() - offset
                )));
            }
            Some(OneTimePreKeyPublic::from_bytes(
                &bytes[offset..offset + ONE_TIME_PRE_KEY_PUBLIC_LEN],
            )?)
        } else {
            None
        };

        Ok(Self {
            identity_public,
            signed_pre_key,
            kem_pre_key,
            one_time_pre_key,
        })
    }
}

impl std::fmt::Debug for PreKeyBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreKeyBundle")
            .field("identity_public", &self.identity_public)
            .field("signed_pre_key", &self.signed_pre_key)
            .field("kem_pre_key", &self.kem_pre_key)
            .field("has_one_time_pre_key", &self.one_time_pre_key.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Signed pre-key ─────────────────────────────────────────────────

    #[test]
    fn signed_pre_key_generate_and_verify() {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 1_700_000_000).expect("generate failed");

        assert_eq!(spk.id(), 1);
        spk.verify(&identity.public_identity())
            .expect("verification failed");
    }

    #[test]
    fn signed_pre_key_wrong_identity_fails() {
        let identity1 = IdentityKeyPair::generate();
        let identity2 = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity1, 1_700_000_000).expect("generate failed");

        let result = spk.verify(&identity2.public_identity());
        assert!(result.is_err());
    }

    #[test]
    fn signed_pre_key_public_roundtrip() {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(42, &identity, 1_700_000_000).expect("generate failed");
        let spk_pub = spk.to_public();

        let bytes = spk_pub.to_bytes();
        assert_eq!(bytes.len(), SIGNED_PRE_KEY_PUBLIC_LEN);

        let restored = SignedPreKeyPublic::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.id(), 42);
        assert_eq!(
            restored.public_key().as_bytes(),
            spk_pub.public_key().as_bytes()
        );
        assert_eq!(restored.timestamp(), 1_700_000_000);

        restored
            .verify(&identity.public_identity())
            .expect("deserialized verification failed");
    }

    #[test]
    fn signed_pre_key_public_verify_after_roundtrip() {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 0).expect("generate failed");
        let spk_pub = spk.to_public();

        let bytes = spk_pub.to_bytes();
        let restored = SignedPreKeyPublic::from_bytes(&bytes).expect("deserialize failed");

        restored
            .verify(&identity.public_identity())
            .expect("verification with deserialized key failed");
    }

    #[test]
    fn signed_pre_key_public_wrong_length_fails() {
        let result = SignedPreKeyPublic::from_bytes(&[0u8; 100]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    // ── One-time pre-key ───────────────────────────────────────────────

    #[test]
    fn one_time_pre_key_generate() {
        let otpk = OneTimePreKey::generate(7);
        assert_eq!(otpk.id(), 7);
        let pk = otpk.public_key();
        assert!(pk.as_bytes().iter().any(|&b| b != 0));
    }

    #[test]
    fn one_time_pre_key_public_roundtrip() {
        let otpk = OneTimePreKey::generate(99);
        let otpk_pub = otpk.to_public();

        let bytes = otpk_pub.to_bytes();
        assert_eq!(bytes.len(), ONE_TIME_PRE_KEY_PUBLIC_LEN);

        let restored = OneTimePreKeyPublic::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.id(), 99);
        assert_eq!(
            restored.public_key().as_bytes(),
            otpk_pub.public_key().as_bytes()
        );
    }

    #[test]
    fn one_time_pre_key_public_wrong_length_fails() {
        let result = OneTimePreKeyPublic::from_bytes(&[0u8; 10]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    // ── KEM pre-key ────────────────────────────────────────────────────

    #[test]
    fn kem_pre_key_generate_and_verify() {
        let identity = IdentityKeyPair::generate();
        let kpk = KemPreKey::generate(3, &identity).expect("generate failed");

        assert_eq!(kpk.id(), 3);
        kpk.to_public()
            .verify(&identity.public_identity())
            .expect("verification failed");
    }

    #[test]
    fn kem_pre_key_wrong_identity_fails() {
        let identity1 = IdentityKeyPair::generate();
        let identity2 = IdentityKeyPair::generate();
        let kpk = KemPreKey::generate(1, &identity1).expect("generate failed");

        let result = kpk.to_public().verify(&identity2.public_identity());
        assert!(result.is_err());
    }

    #[test]
    fn kem_pre_key_public_roundtrip() {
        let identity = IdentityKeyPair::generate();
        let kpk = KemPreKey::generate(55, &identity).expect("generate failed");
        let kpk_pub = kpk.to_public();

        let bytes = kpk_pub.to_bytes();
        assert_eq!(bytes.len(), KEM_PRE_KEY_PUBLIC_LEN);

        let restored = KemPreKeyPublic::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.id(), 55);

        restored
            .verify(&identity.public_identity())
            .expect("deserialized verification failed");
    }

    #[test]
    fn kem_pre_key_public_wrong_length_fails() {
        let result = KemPreKeyPublic::from_bytes(&[0u8; 100]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    // ── PreKeyBundle ───────────────────────────────────────────────────

    fn make_bundle(with_otpk: bool) -> (IdentityKeyPair, PreKeyBundle) {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 1_700_000_000).expect("spk failed");
        let kpk = KemPreKey::generate(1, &identity).expect("kpk failed");
        let otpk = if with_otpk {
            Some(OneTimePreKey::generate(1).to_public())
        } else {
            None
        };

        let bundle = PreKeyBundle::new(
            identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            otpk,
        );
        (identity, bundle)
    }

    #[test]
    fn bundle_validate_succeeds() {
        let (_, bundle) = make_bundle(true);
        bundle.validate().expect("validate failed");
    }

    #[test]
    fn bundle_validate_without_otpk_succeeds() {
        let (_, bundle) = make_bundle(false);
        bundle.validate().expect("validate failed");
    }

    #[test]
    fn bundle_validate_fails_with_tampered_signed_pre_key() {
        let identity = IdentityKeyPair::generate();
        let wrong_identity = IdentityKeyPair::generate();
        // Sign with wrong identity
        let spk = SignedPreKey::generate(1, &wrong_identity, 0).expect("spk failed");
        let kpk = KemPreKey::generate(1, &identity).expect("kpk failed");

        let bundle = PreKeyBundle::new(
            identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            None,
        );

        let result = bundle.validate();
        assert!(result.is_err());
    }

    #[test]
    fn bundle_validate_fails_with_tampered_kem_pre_key() {
        let identity = IdentityKeyPair::generate();
        let wrong_identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 0).expect("spk failed");
        // Sign with wrong identity
        let kpk = KemPreKey::generate(1, &wrong_identity).expect("kpk failed");

        let bundle = PreKeyBundle::new(
            identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            None,
        );

        let result = bundle.validate();
        assert!(result.is_err());
    }

    #[test]
    fn bundle_roundtrip_with_otpk() {
        let (_, bundle) = make_bundle(true);

        let bytes = bundle.to_bytes();
        let restored = PreKeyBundle::from_bytes(&bytes).expect("deserialize failed");

        assert!(restored.one_time_pre_key().is_some());
        restored.validate().expect("deserialized validate failed");
    }

    #[test]
    fn bundle_roundtrip_without_otpk() {
        let (_, bundle) = make_bundle(false);

        let bytes = bundle.to_bytes();
        let restored = PreKeyBundle::from_bytes(&bytes).expect("deserialize failed");

        assert!(restored.one_time_pre_key().is_none());
        restored.validate().expect("deserialized validate failed");
    }

    #[test]
    fn bundle_from_bytes_too_short_fails() {
        let result = PreKeyBundle::from_bytes(&[0u8; 100]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    #[test]
    fn bundle_accessors() {
        let (identity, bundle) = make_bundle(true);

        assert_eq!(
            bundle.identity_public(),
            &identity.public_identity()
        );
        assert_eq!(bundle.signed_pre_key().id(), 1);
        assert_eq!(bundle.kem_pre_key().id(), 1);
        assert!(bundle.one_time_pre_key().is_some());
        assert_eq!(bundle.one_time_pre_key().unwrap().id(), 1);
    }

    // ── Debug output ───────────────────────────────────────────────────

    #[test]
    fn signed_pre_key_debug_redacts_secret() {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 0).expect("generate failed");
        let debug = format!("{spk:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("SignedPreKey"));
    }

    #[test]
    fn one_time_pre_key_debug_redacts_secret() {
        let otpk = OneTimePreKey::generate(1);
        let debug = format!("{otpk:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("OneTimePreKey"));
    }

    #[test]
    fn kem_pre_key_debug_redacts_secret() {
        let identity = IdentityKeyPair::generate();
        let kpk = KemPreKey::generate(1, &identity).expect("generate failed");
        let debug = format!("{kpk:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("KemPreKey"));
    }

    #[test]
    fn bundle_debug_output() {
        let (_, bundle) = make_bundle(true);
        let debug = format!("{bundle:?}");
        assert!(debug.contains("PreKeyBundle"));
        assert!(debug.contains("has_one_time_pre_key"));
    }

    // ── Serialization size constants ───────────────────────────────────

    #[test]
    fn serialization_sizes_are_consistent() {
        let identity = IdentityKeyPair::generate();

        let spk = SignedPreKey::generate(1, &identity, 0).expect("spk failed");
        assert_eq!(spk.to_public().to_bytes().len(), SIGNED_PRE_KEY_PUBLIC_LEN);

        let otpk = OneTimePreKey::generate(1);
        assert_eq!(
            otpk.to_public().to_bytes().len(),
            ONE_TIME_PRE_KEY_PUBLIC_LEN
        );

        let kpk = KemPreKey::generate(1, &identity).expect("kpk failed");
        assert_eq!(kpk.to_public().to_bytes().len(), KEM_PRE_KEY_PUBLIC_LEN);
    }
}
