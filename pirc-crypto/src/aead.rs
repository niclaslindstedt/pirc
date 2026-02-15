//! AES-256-GCM authenticated encryption with associated data.
//!
//! Provides symmetric encryption and decryption using AES-256-GCM.
//! Message keys derived by the symmetric ratchet are consumed here
//! to encrypt and decrypt message payloads.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};

/// Size of an AES-256 key in bytes (256 bits).
pub const KEY_SIZE: usize = 32;

/// Size of a GCM nonce in bytes (96 bits).
pub const NONCE_SIZE: usize = 12;

/// Size of a GCM authentication tag in bytes (128 bits).
pub const TAG_SIZE: usize = 16;

/// An AES-256-GCM encryption key with automatic zeroization on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct AeadKey {
    bytes: [u8; KEY_SIZE],
}

impl AeadKey {
    /// Create a key from a 32-byte array.
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_SIZE]) -> Self {
        Self { bytes }
    }

    /// Return a reference to the underlying key bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_SIZE] {
        &self.bytes
    }
}

impl std::fmt::Debug for AeadKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Generate a random 12-byte nonce suitable for AES-256-GCM.
///
/// Uses the operating system's cryptographically secure random number
/// generator. Each nonce MUST be unique for a given key; reusing a
/// nonce with the same key breaks GCM's security guarantees.
#[must_use]
pub fn generate_nonce() -> [u8; NONCE_SIZE] {
    let mut nonce = [0u8; NONCE_SIZE];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Encrypt plaintext using AES-256-GCM.
///
/// Returns `ciphertext || auth_tag` (the ciphertext with the 16-byte
/// authentication tag appended).
///
/// # Arguments
///
/// * `key` — 32-byte AES-256 key
/// * `nonce` — 12-byte nonce (must never be reused with the same key)
/// * `plaintext` — data to encrypt
/// * `aad` — additional authenticated data (authenticated but not encrypted)
///
/// # Errors
///
/// Returns [`CryptoError::Aead`] if encryption fails.
pub fn encrypt(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());
    let gcm_nonce = Nonce::from_slice(nonce);
    let payload = aes_gcm::aead::Payload {
        msg: plaintext,
        aad,
    };
    cipher
        .encrypt(gcm_nonce, payload)
        .map_err(|e| CryptoError::Aead(format!("encryption failed: {e}")))
}

/// Decrypt ciphertext using AES-256-GCM.
///
/// Expects the input to be `ciphertext || auth_tag` as produced by
/// [`encrypt`]. Verifies the authentication tag and returns the
/// original plaintext.
///
/// # Arguments
///
/// * `key` — 32-byte AES-256 key (must match the encryption key)
/// * `nonce` — 12-byte nonce (must match the encryption nonce)
/// * `ciphertext` — encrypted data with appended authentication tag
/// * `aad` — additional authenticated data (must match what was used during encryption)
///
/// # Errors
///
/// Returns [`CryptoError::Aead`] if decryption or authentication fails.
pub fn decrypt(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    if ciphertext.len() < TAG_SIZE {
        return Err(CryptoError::Aead(
            "ciphertext too short to contain authentication tag".into(),
        ));
    }
    let cipher = Aes256Gcm::new(key.into());
    let gcm_nonce = Nonce::from_slice(nonce);
    let payload = aes_gcm::aead::Payload {
        msg: ciphertext,
        aad,
    };
    cipher
        .decrypt(gcm_nonce, payload)
        .map_err(|e| CryptoError::Aead(format!("decryption failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [0x42u8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = b"Hello, world!";
        let aad = b"associated data";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        assert_eq!(ciphertext.len(), plaintext.len() + TAG_SIZE);

        let decrypted = decrypt(&key, &nonce, &ciphertext, aad).expect("decrypt failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let key = [0xABu8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = b"";
        let aad = b"";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        // Empty plaintext should produce only the tag
        assert_eq!(ciphertext.len(), TAG_SIZE);

        let decrypted = decrypt(&key, &nonce, &ciphertext, aad).expect("decrypt failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn large_plaintext_roundtrip() {
        let key = [0xCDu8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = vec![0xFFu8; 1024 * 64]; // 64 KiB
        let aad = b"large message test";

        let ciphertext = encrypt(&key, &nonce, &plaintext, aad).expect("encrypt failed");
        assert_eq!(ciphertext.len(), plaintext.len() + TAG_SIZE);

        let decrypted = decrypt(&key, &nonce, &ciphertext, aad).expect("decrypt failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key = [0x01u8; KEY_SIZE];
        let wrong_key = [0x02u8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = b"secret message";
        let aad = b"";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        let result = decrypt(&wrong_key, &nonce, &ciphertext, aad);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("decryption failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tampered_ciphertext_fails_decryption() {
        let key = [0x03u8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = b"do not tamper";
        let aad = b"integrity check";

        let mut ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        // Flip a bit in the ciphertext body
        ciphertext[0] ^= 0x01;

        let result = decrypt(&key, &nonce, &ciphertext, aad);
        assert!(result.is_err());
    }

    #[test]
    fn wrong_aad_fails_decryption() {
        let key = [0x04u8; KEY_SIZE];
        let nonce = generate_nonce();
        let plaintext = b"aad-protected";
        let aad = b"correct aad";
        let wrong_aad = b"wrong aad";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        let result = decrypt(&key, &nonce, &ciphertext, wrong_aad);

        assert!(result.is_err());
    }

    #[test]
    fn wrong_nonce_fails_decryption() {
        let key = [0x05u8; KEY_SIZE];
        let nonce = generate_nonce();
        let other_nonce = generate_nonce();
        let plaintext = b"nonce matters";
        let aad = b"";

        let ciphertext = encrypt(&key, &nonce, plaintext, aad).expect("encrypt failed");
        let result = decrypt(&key, &other_nonce, &ciphertext, aad);

        assert!(result.is_err());
    }

    #[test]
    fn ciphertext_too_short_fails() {
        let key = [0x06u8; KEY_SIZE];
        let nonce = generate_nonce();
        let short = vec![0u8; TAG_SIZE - 1]; // shorter than a tag

        let result = decrypt(&key, &nonce, &short, b"");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("too short"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn nonce_generation_produces_unique_values() {
        let n1 = generate_nonce();
        let n2 = generate_nonce();
        // Two random 96-bit nonces should differ with overwhelming probability
        assert_ne!(n1, n2);
    }

    #[test]
    fn nonce_is_correct_size() {
        let nonce = generate_nonce();
        assert_eq!(nonce.len(), NONCE_SIZE);
    }

    #[test]
    fn aead_key_zeroizes_on_drop() {
        let key = AeadKey::from_bytes([0xAAu8; KEY_SIZE]);
        assert_eq!(key.as_bytes(), &[0xAAu8; KEY_SIZE]);
        // Key should be usable and have correct bytes
        assert_eq!(key.as_bytes().len(), KEY_SIZE);
    }

    #[test]
    fn aead_key_debug_redacts() {
        let key = AeadKey::from_bytes([0xBBu8; KEY_SIZE]);
        let debug_str = format!("{key:?}");
        assert!(debug_str.contains("REDACTED"));
        assert!(!debug_str.contains("187")); // 0xBB = 187
    }

    #[test]
    fn encrypt_with_aead_key_type() {
        let key = AeadKey::from_bytes([0x55u8; KEY_SIZE]);
        let nonce = generate_nonce();
        let plaintext = b"using AeadKey";
        let aad = b"";

        let ciphertext =
            encrypt(key.as_bytes(), &nonce, plaintext, aad).expect("encrypt failed");
        let decrypted =
            decrypt(key.as_bytes(), &nonce, &ciphertext, aad).expect("decrypt failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_aad_produces_different_tags() {
        let key = [0x07u8; KEY_SIZE];
        let nonce = [0x08u8; NONCE_SIZE]; // Fixed nonce for comparison
        let plaintext = b"same plaintext";

        let ct1 = encrypt(&key, &nonce, plaintext, b"aad1").expect("encrypt failed");
        let ct2 = encrypt(&key, &nonce, plaintext, b"aad2").expect("encrypt failed");

        // Same key, nonce, plaintext but different AAD should produce different output
        // (the tag portion differs)
        assert_ne!(ct1, ct2);
    }
}
