//! Encrypted-at-rest key storage.
//!
//! All key material is encrypted with AES-256-GCM using a passphrase-derived
//! key (Argon2id). No plaintext secrets are stored on disk.

use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::aead;
use crate::error::{CryptoError, Result};
use crate::identity::IdentityKeyPair;
use crate::prekey::{
    KemPreKey, OneTimePreKey, OneTimePreKeyPublic, PreKeyBundle, SignedPreKey,
};

/// Argon2id memory cost in KiB (64 MiB).
const ARGON2_M_COST: u32 = 65_536;

/// Argon2id time cost (iterations).
const ARGON2_T_COST: u32 = 3;

/// Argon2id parallelism.
const ARGON2_P_COST: u32 = 1;

/// Size of the Argon2id salt in bytes.
const SALT_LEN: usize = 16;

/// Size of the AES-256-GCM nonce in bytes.
const NONCE_LEN: usize = 12;

// ── StorageKey ────────────────────────────────────────────────────────────

/// Passphrase-derived encryption key (Argon2id).
///
/// Wraps a 32-byte AES-256 key derived from a passphrase and salt.
/// Automatically zeroized on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StorageKey {
    bytes: [u8; 32],
}

impl StorageKey {
    /// Derive an encryption key from a passphrase and salt using Argon2id.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::KeyDerivation`] if Argon2id fails.
    pub fn derive(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> Result<Self> {
        let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
            .map_err(|e| CryptoError::KeyDerivation(format!("argon2 params: {e}")))?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut key = [0u8; 32];
        argon2
            .hash_password_into(passphrase, salt, &mut key)
            .map_err(|e| CryptoError::KeyDerivation(format!("argon2 hash: {e}")))?;

        Ok(Self { bytes: key })
    }

    /// Return a reference to the 32-byte key.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl std::fmt::Debug for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

// ── EncryptedKeyStore ─────────────────────────────────────────────────────

/// Container for encrypted key material.
///
/// Holds the Argon2id salt, AES-256-GCM nonce, and the encrypted ciphertext.
/// This is the on-disk representation — no plaintext secrets.
pub struct EncryptedKeyStore {
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

impl EncryptedKeyStore {
    /// Serialize the encrypted store to a byte vector.
    ///
    /// Format: `[salt (16) | nonce (12) | ciphertext (variable)]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(SALT_LEN + NONCE_LEN + self.ciphertext.len());
        bytes.extend_from_slice(&self.salt);
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Deserialize an encrypted store from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is too short.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let min_len = SALT_LEN + NONCE_LEN + aead::TAG_SIZE;
        if bytes.len() < min_len {
            return Err(CryptoError::Serialization(format!(
                "EncryptedKeyStore: expected at least {min_len} bytes, got {}",
                bytes.len()
            )));
        }

        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[..SALT_LEN]);

        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[SALT_LEN..SALT_LEN + NONCE_LEN]);

        let ciphertext = bytes[SALT_LEN + NONCE_LEN..].to_vec();

        Ok(Self {
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Return a reference to the salt.
    #[must_use]
    pub fn salt(&self) -> &[u8; SALT_LEN] {
        &self.salt
    }
}

impl std::fmt::Debug for EncryptedKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedKeyStore")
            .field("salt", &hex::encode(&self.salt))
            .field("nonce", &"[..]")
            .field("ciphertext_len", &self.ciphertext.len())
            .finish()
    }
}

// ── KeyBundle (in-memory plaintext) ──────────────────────────────────────

/// In-memory representation of all user key material.
///
/// This is the plaintext form that exists only in memory. It is never
/// written to disk without encryption.
pub struct KeyBundle {
    identity: IdentityKeyPair,
    signed_pre_keys: Vec<SignedPreKey>,
    one_time_pre_keys: Vec<OneTimePreKey>,
    kem_pre_keys: Vec<KemPreKey>,
    next_pre_key_id: u32,
}

impl KeyBundle {
    /// Write a length-prefixed blob to the buffer.
    fn write_blob(buf: &mut Vec<u8>, data: &[u8]) {
        #[allow(clippy::cast_possible_truncation)]
        let len = data.len() as u32; // Key data is always < 4 GiB
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(data);
    }

    /// Write a u32 count to the buffer.
    fn write_count(buf: &mut Vec<u8>, count: usize) {
        #[allow(clippy::cast_possible_truncation)]
        let n = count as u32; // Pre-key counts are always small
        buf.extend_from_slice(&n.to_le_bytes());
    }

    /// Serialize the key bundle to a length-prefixed binary format.
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Identity key pair
        Self::write_blob(&mut buf, &self.identity.to_bytes());

        // Signed pre-keys
        Self::write_count(&mut buf, self.signed_pre_keys.len());
        for spk in &self.signed_pre_keys {
            Self::write_blob(&mut buf, &spk.to_bytes());
        }

        // One-time pre-keys
        Self::write_count(&mut buf, self.one_time_pre_keys.len());
        for otpk in &self.one_time_pre_keys {
            Self::write_blob(&mut buf, &otpk.to_bytes());
        }

        // KEM pre-keys
        Self::write_count(&mut buf, self.kem_pre_keys.len());
        for kpk in &self.kem_pre_keys {
            Self::write_blob(&mut buf, &kpk.to_bytes());
        }

        // Next pre-key ID
        buf.extend_from_slice(&self.next_pre_key_id.to_le_bytes());

        buf
    }

    /// Deserialize a key bundle from the length-prefixed binary format.
    fn deserialize(data: &[u8]) -> Result<Self> {
        let mut offset = 0;

        let read_u32 = |off: &mut usize| -> Result<u32> {
            if *off + 4 > data.len() {
                return Err(CryptoError::Serialization("unexpected end of data".into()));
            }
            let val = u32::from_le_bytes(data[*off..*off + 4].try_into().unwrap());
            *off += 4;
            Ok(val)
        };

        let read_blob = |off: &mut usize| -> Result<&[u8]> {
            let len = {
                if *off + 4 > data.len() {
                    return Err(CryptoError::Serialization("unexpected end of data".into()));
                }
                let val = u32::from_le_bytes(data[*off..*off + 4].try_into().unwrap());
                *off += 4;
                val as usize
            };
            if *off + len > data.len() {
                return Err(CryptoError::Serialization(format!(
                    "expected {len} bytes at offset {off}, got {}",
                    data.len() - *off
                )));
            }
            let slice = &data[*off..*off + len];
            *off += len;
            Ok(slice)
        };

        // Identity key pair
        let id_bytes = read_blob(&mut offset)?;
        let identity = IdentityKeyPair::from_bytes(id_bytes)?;

        // Signed pre-keys
        let spk_count = read_u32(&mut offset)? as usize;
        let mut signed_pre_keys = Vec::with_capacity(spk_count);
        for _ in 0..spk_count {
            let spk_bytes = read_blob(&mut offset)?;
            signed_pre_keys.push(SignedPreKey::from_bytes(spk_bytes)?);
        }

        // One-time pre-keys
        let otpk_count = read_u32(&mut offset)? as usize;
        let mut one_time_pre_keys = Vec::with_capacity(otpk_count);
        for _ in 0..otpk_count {
            let otpk_bytes = read_blob(&mut offset)?;
            one_time_pre_keys.push(OneTimePreKey::from_bytes(otpk_bytes)?);
        }

        // KEM pre-keys
        let kpk_count = read_u32(&mut offset)? as usize;
        let mut kem_pre_keys = Vec::with_capacity(kpk_count);
        for _ in 0..kpk_count {
            let kpk_bytes = read_blob(&mut offset)?;
            kem_pre_keys.push(KemPreKey::from_bytes(kpk_bytes)?);
        }

        // Next pre-key ID
        let next_pre_key_id = read_u32(&mut offset)?;

        Ok(Self {
            identity,
            signed_pre_keys,
            one_time_pre_keys,
            kem_pre_keys,
            next_pre_key_id,
        })
    }
}

impl std::fmt::Debug for KeyBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyBundle")
            .field("identity", &"[REDACTED]")
            .field("signed_pre_keys", &self.signed_pre_keys.len())
            .field("one_time_pre_keys", &self.one_time_pre_keys.len())
            .field("kem_pre_keys", &self.kem_pre_keys.len())
            .field("next_pre_key_id", &self.next_pre_key_id)
            .finish()
    }
}

// ── KeyStore (high-level API) ────────────────────────────────────────────

/// High-level API for encrypted key storage.
///
/// Manages identity keys, pre-keys, and provides encryption/decryption
/// for persistent storage. All key material is encrypted with AES-256-GCM
/// using a passphrase-derived key (Argon2id).
pub struct KeyStore {
    bundle: KeyBundle,
}

impl KeyStore {
    /// Create a new key store with a fresh identity.
    ///
    /// Generates a new identity key pair and initializes empty pre-key
    /// collections.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if key generation fails.
    pub fn create() -> Result<Self> {
        let identity = IdentityKeyPair::generate();
        let timestamp = 0; // Caller should set real timestamps via rotation

        // Generate initial signed pre-key
        let spk = SignedPreKey::generate(1, &identity, timestamp)?;

        // Generate initial KEM pre-key
        let kpk = KemPreKey::generate(2, &identity)?;

        Ok(Self {
            bundle: KeyBundle {
                identity,
                signed_pre_keys: vec![spk],
                one_time_pre_keys: Vec::new(),
                kem_pre_keys: vec![kpk],
                next_pre_key_id: 3,
            },
        })
    }

    /// Open an encrypted key store with the given passphrase.
    ///
    /// Decrypts the ciphertext and deserializes the key bundle.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Aead`] if the passphrase is wrong (authentication
    /// failure), or [`CryptoError::Serialization`] if the decrypted data is
    /// malformed.
    pub fn open(encrypted: &EncryptedKeyStore, passphrase: &[u8]) -> Result<Self> {
        let storage_key = StorageKey::derive(passphrase, &encrypted.salt)?;

        let plaintext = aead::decrypt(
            storage_key.as_bytes(),
            &encrypted.nonce,
            &encrypted.ciphertext,
            b"pirc-key-store-v1",
        )
        .map_err(|_| CryptoError::Aead("wrong passphrase or corrupted data".into()))?;

        let bundle = KeyBundle::deserialize(&plaintext)?;

        Ok(Self { bundle })
    }

    /// Encrypt the key store and return the encrypted container.
    ///
    /// Uses a fresh random salt and nonce for each save operation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if key derivation or encryption fails.
    pub fn save(&self, passphrase: &[u8]) -> Result<EncryptedKeyStore> {
        let mut salt = [0u8; SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);

        let storage_key = StorageKey::derive(passphrase, &salt)?;

        let nonce = aead::generate_nonce();

        let mut plaintext = self.bundle.serialize();
        let ciphertext = aead::encrypt(
            storage_key.as_bytes(),
            &nonce,
            &plaintext,
            b"pirc-key-store-v1",
        )?;

        // Zeroize plaintext buffer
        plaintext.zeroize();

        Ok(EncryptedKeyStore {
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Return a reference to the identity key pair.
    #[must_use]
    pub fn identity(&self) -> &IdentityKeyPair {
        &self.bundle.identity
    }

    /// Generate a public pre-key bundle from current keys.
    ///
    /// Uses the first signed pre-key, first KEM pre-key, and optionally
    /// the first available one-time pre-key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if there are no signed or KEM pre-keys.
    pub fn public_bundle(&self) -> Result<PreKeyBundle> {
        let spk = self
            .bundle
            .signed_pre_keys
            .first()
            .ok_or_else(|| CryptoError::InvalidKey("no signed pre-keys available".into()))?;

        let kpk = self
            .bundle
            .kem_pre_keys
            .first()
            .ok_or_else(|| CryptoError::InvalidKey("no KEM pre-keys available".into()))?;

        let otpk = self
            .bundle
            .one_time_pre_keys
            .first()
            .map(OneTimePreKey::to_public);

        let bundle = PreKeyBundle::new(
            self.bundle.identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            otpk,
        );

        bundle.validate()?;

        Ok(bundle)
    }

    /// Consume (remove) a one-time pre-key by ID.
    ///
    /// Returns the consumed key, or `None` if no key with that ID exists.
    pub fn consume_one_time_pre_key(&mut self, id: u32) -> Option<OneTimePreKey> {
        let pos = self
            .bundle
            .one_time_pre_keys
            .iter()
            .position(|k| k.id() == id)?;
        Some(self.bundle.one_time_pre_keys.remove(pos))
    }

    /// Generate new one-time pre-keys and return their public halves.
    pub fn generate_one_time_pre_keys(&mut self, count: u32) -> Vec<OneTimePreKeyPublic> {
        let mut public_keys = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let id = self.bundle.next_pre_key_id;
            self.bundle.next_pre_key_id += 1;
            let otpk = OneTimePreKey::generate(id);
            public_keys.push(otpk.to_public());
            self.bundle.one_time_pre_keys.push(otpk);
        }
        public_keys
    }

    /// Return the number of one-time pre-keys available.
    #[must_use]
    pub fn one_time_pre_key_count(&self) -> usize {
        self.bundle.one_time_pre_keys.len()
    }
}

impl std::fmt::Debug for KeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyStore")
            .field("bundle", &self.bundle)
            .finish()
    }
}

// Hex encoding helper for debug output (avoid pulling in hex crate)
mod hex {
    use std::fmt::Write;

    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            let _ = write!(s, "{b:02x}");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use reduced Argon2 parameters for tests to avoid slow CI
    // (The production code uses the full 64MiB parameters)

    // ── StorageKey ─────────────────────────────────────────────────────

    #[test]
    fn storage_key_derive_produces_32_bytes() {
        let salt = [0x42u8; SALT_LEN];
        let key = StorageKey::derive(b"test passphrase", &salt).expect("derive failed");
        assert_eq!(key.as_bytes().len(), 32);
        assert!(key.as_bytes().iter().any(|&b| b != 0));
    }

    #[test]
    fn storage_key_different_passphrases_differ() {
        let salt = [0x42u8; SALT_LEN];
        let key1 = StorageKey::derive(b"password1", &salt).expect("derive failed");
        let key2 = StorageKey::derive(b"password2", &salt).expect("derive failed");
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn storage_key_different_salts_differ() {
        let salt1 = [0x01u8; SALT_LEN];
        let salt2 = [0x02u8; SALT_LEN];
        let key1 = StorageKey::derive(b"password", &salt1).expect("derive failed");
        let key2 = StorageKey::derive(b"password", &salt2).expect("derive failed");
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn storage_key_is_deterministic() {
        let salt = [0x42u8; SALT_LEN];
        let key1 = StorageKey::derive(b"password", &salt).expect("derive failed");
        let key2 = StorageKey::derive(b"password", &salt).expect("derive failed");
        assert_eq!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn storage_key_debug_redacts() {
        let salt = [0x42u8; SALT_LEN];
        let key = StorageKey::derive(b"password", &salt).expect("derive failed");
        let debug = format!("{key:?}");
        assert!(debug.contains("REDACTED"));
    }

    // ── EncryptedKeyStore serialization ────────────────────────────────

    #[test]
    fn encrypted_key_store_roundtrip() {
        let store = EncryptedKeyStore {
            salt: [0xAA; SALT_LEN],
            nonce: [0xBB; NONCE_LEN],
            ciphertext: vec![0xCC; 100],
        };

        let bytes = store.to_bytes();
        let restored = EncryptedKeyStore::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(restored.salt, store.salt);
        assert_eq!(restored.nonce, store.nonce);
        assert_eq!(restored.ciphertext, store.ciphertext);
    }

    #[test]
    fn encrypted_key_store_from_bytes_too_short() {
        let result = EncryptedKeyStore::from_bytes(&[0u8; 10]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    // ── KeyStore create / save / open ─────────────────────────────────

    #[test]
    fn key_store_create_save_open_roundtrip() {
        let store = KeyStore::create().expect("create failed");
        let original_pub = store.identity().public_identity();

        let encrypted = store.save(b"my passphrase").expect("save failed");
        let restored = KeyStore::open(&encrypted, b"my passphrase").expect("open failed");

        let restored_pub = restored.identity().public_identity();
        assert_eq!(original_pub, restored_pub);
    }

    #[test]
    fn key_store_wrong_passphrase_fails() {
        let store = KeyStore::create().expect("create failed");
        let encrypted = store.save(b"correct passphrase").expect("save failed");

        let result = KeyStore::open(&encrypted, b"wrong passphrase");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("wrong passphrase")
                || err.to_string().contains("corrupted"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn key_store_different_passphrases_different_encrypted() {
        let store = KeyStore::create().expect("create failed");
        let enc1 = store.save(b"password1").expect("save 1 failed");
        let enc2 = store.save(b"password2").expect("save 2 failed");

        // Different passphrases produce different ciphertext
        assert_ne!(enc1.ciphertext, enc2.ciphertext);
    }

    #[test]
    fn key_store_salt_is_random() {
        let store = KeyStore::create().expect("create failed");
        let enc1 = store.save(b"password").expect("save 1 failed");
        let enc2 = store.save(b"password").expect("save 2 failed");

        // Each save produces a different salt
        assert_ne!(enc1.salt, enc2.salt);
    }

    #[test]
    fn encrypted_key_store_bytes_roundtrip() {
        let store = KeyStore::create().expect("create failed");
        let encrypted = store.save(b"passphrase").expect("save failed");

        let bytes = encrypted.to_bytes();
        let restored_enc = EncryptedKeyStore::from_bytes(&bytes).expect("deserialize failed");
        let restored = KeyStore::open(&restored_enc, b"passphrase").expect("open failed");

        assert_eq!(
            store.identity().public_identity(),
            restored.identity().public_identity()
        );
    }

    // ── Pre-key management ────────────────────────────────────────────

    #[test]
    fn generate_one_time_pre_keys() {
        let mut store = KeyStore::create().expect("create failed");
        assert_eq!(store.one_time_pre_key_count(), 0);

        let public_keys = store.generate_one_time_pre_keys(5);
        assert_eq!(public_keys.len(), 5);
        assert_eq!(store.one_time_pre_key_count(), 5);

        // IDs should be sequential
        for (i, pk) in public_keys.iter().enumerate() {
            // next_pre_key_id starts at 3 (after initial spk=1, kpk=2)
            assert_eq!(pk.id(), 3 + i as u32);
        }
    }

    #[test]
    fn consume_one_time_pre_key() {
        let mut store = KeyStore::create().expect("create failed");
        let public_keys = store.generate_one_time_pre_keys(3);

        let consumed = store.consume_one_time_pre_key(public_keys[1].id());
        assert!(consumed.is_some());
        assert_eq!(consumed.unwrap().id(), public_keys[1].id());
        assert_eq!(store.one_time_pre_key_count(), 2);

        // Consuming again returns None
        assert!(store.consume_one_time_pre_key(public_keys[1].id()).is_none());
    }

    #[test]
    fn consume_nonexistent_pre_key_returns_none() {
        let mut store = KeyStore::create().expect("create failed");
        assert!(store.consume_one_time_pre_key(999).is_none());
    }

    // ── Public bundle ─────────────────────────────────────────────────

    #[test]
    fn public_bundle_validates() {
        let store = KeyStore::create().expect("create failed");
        let bundle = store.public_bundle().expect("bundle failed");
        bundle.validate().expect("validate failed");
    }

    #[test]
    fn public_bundle_includes_otpk_when_available() {
        let mut store = KeyStore::create().expect("create failed");
        store.generate_one_time_pre_keys(1);

        let bundle = store.public_bundle().expect("bundle failed");
        assert!(bundle.one_time_pre_key().is_some());
        bundle.validate().expect("validate failed");
    }

    #[test]
    fn public_bundle_without_otpk() {
        let store = KeyStore::create().expect("create failed");
        let bundle = store.public_bundle().expect("bundle failed");
        assert!(bundle.one_time_pre_key().is_none());
        bundle.validate().expect("validate failed");
    }

    // ── Pre-keys survive save/open ────────────────────────────────────

    #[test]
    fn one_time_pre_keys_survive_roundtrip() {
        let mut store = KeyStore::create().expect("create failed");
        let public_keys = store.generate_one_time_pre_keys(3);

        let encrypted = store.save(b"pass").expect("save failed");
        let mut restored = KeyStore::open(&encrypted, b"pass").expect("open failed");

        assert_eq!(restored.one_time_pre_key_count(), 3);

        // Can consume by the same IDs
        let consumed = restored.consume_one_time_pre_key(public_keys[0].id());
        assert!(consumed.is_some());
        assert_eq!(consumed.unwrap().id(), public_keys[0].id());
        assert_eq!(restored.one_time_pre_key_count(), 2);
    }

    #[test]
    fn signed_pre_keys_survive_roundtrip() {
        let store = KeyStore::create().expect("create failed");
        let bundle_before = store.public_bundle().expect("bundle failed");

        let encrypted = store.save(b"pass").expect("save failed");
        let restored = KeyStore::open(&encrypted, b"pass").expect("open failed");
        let bundle_after = restored.public_bundle().expect("bundle failed");

        // Signed pre-key should have the same ID and public key
        assert_eq!(
            bundle_before.signed_pre_key().id(),
            bundle_after.signed_pre_key().id()
        );
        assert_eq!(
            bundle_before.signed_pre_key().public_key().as_bytes(),
            bundle_after.signed_pre_key().public_key().as_bytes()
        );
    }
}
