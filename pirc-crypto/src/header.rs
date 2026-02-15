//! Header encryption.
//!
//! Encrypts and decrypts message headers to hide ratchet metadata
//! (public keys, message numbers, previous chain length) from
//! observers. Uses a separate header key derived from the KDF chain.

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::aead::{self, KEY_SIZE, NONCE_SIZE};
use crate::error::{CryptoError, Result};
use crate::message::MessageHeader;

/// A 32-byte key for encrypting and decrypting message headers.
///
/// Header keys are derived from the KDF chain alongside message keys.
/// They are zeroized on drop to support forward secrecy.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct HeaderKey {
    bytes: [u8; KEY_SIZE],
}

impl HeaderKey {
    /// Create a header key from a 32-byte array.
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

impl std::fmt::Debug for HeaderKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Encrypt a message header using AES-256-GCM.
///
/// Serializes the header and encrypts it with a fresh random nonce.
/// The header is authenticated but no additional data is bound.
///
/// # Returns
///
/// A tuple of (encrypted header bytes with auth tag, nonce).
///
/// # Errors
///
/// Returns [`CryptoError::HeaderEncryption`] if encryption fails.
pub fn encrypt_header(
    header_key: &HeaderKey,
    header: &MessageHeader,
) -> Result<(Vec<u8>, [u8; NONCE_SIZE])> {
    let plaintext = header.to_bytes();
    let nonce = aead::generate_nonce();

    let ciphertext = aead::encrypt(header_key.as_bytes(), &nonce, &plaintext, b"")
        .map_err(|e| CryptoError::HeaderEncryption(format!("encrypt failed: {e}")))?;

    Ok((ciphertext, nonce))
}

/// Decrypt a message header using AES-256-GCM.
///
/// Decrypts the header bytes and deserializes the result into a
/// [`MessageHeader`].
///
/// # Errors
///
/// Returns [`CryptoError::HeaderEncryption`] if decryption or
/// authentication fails, or if the decrypted bytes are not a valid
/// header.
pub fn decrypt_header(
    header_key: &HeaderKey,
    encrypted: &[u8],
    nonce: &[u8; NONCE_SIZE],
) -> Result<MessageHeader> {
    let plaintext = aead::decrypt(header_key.as_bytes(), nonce, encrypted, b"")
        .map_err(|e| CryptoError::HeaderEncryption(format!("decrypt failed: {e}")))?;

    MessageHeader::from_bytes(&plaintext)
}

/// Try decrypting a header with multiple keys.
///
/// Attempts each key in order and returns the first successful
/// decryption along with the index of the key that worked. This is
/// used when the receiver doesn't know whether a DH ratchet step
/// occurred — the current header key is tried first, then the
/// previous one.
///
/// # Returns
///
/// A tuple of (decrypted [`MessageHeader`], index of the successful key).
///
/// # Errors
///
/// Returns [`CryptoError::HeaderEncryption`] if none of the keys
/// can decrypt the header.
pub fn try_decrypt_header(
    header_keys: &[HeaderKey],
    encrypted: &[u8],
    nonce: &[u8; NONCE_SIZE],
) -> Result<(MessageHeader, usize)> {
    for (i, key) in header_keys.iter().enumerate() {
        if let Ok(header) = decrypt_header(key, encrypted, nonce) {
            return Ok((header, i));
        }
    }

    Err(CryptoError::HeaderEncryption(
        "no header key could decrypt the header".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::KemKeyPair;
    use crate::x25519;

    fn make_dh_public() -> x25519::PublicKey {
        let kp = x25519::KeyPair::generate();
        kp.public_key()
    }

    fn make_header_key(fill: u8) -> HeaderKey {
        HeaderKey::from_bytes([fill; KEY_SIZE])
    }

    fn make_simple_header() -> MessageHeader {
        MessageHeader {
            dh_public: make_dh_public(),
            message_number: 42,
            previous_chain_length: 7,
            kem_ciphertext: None,
            kem_public: None,
        }
    }

    fn make_pq_header() -> MessageHeader {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();
        let (ct, _) = pk.encapsulate().expect("encapsulate failed");

        MessageHeader {
            dh_public: make_dh_public(),
            message_number: 100,
            previous_chain_length: 50,
            kem_ciphertext: Some(ct),
            kem_public: Some(pk),
        }
    }

    // ── Encrypt / decrypt ───────────────────────────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = make_header_key(0x42);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&key, &header).expect("encrypt failed");
        let decrypted = decrypt_header(&key, &encrypted, &nonce).expect("decrypt failed");

        assert_eq!(decrypted.dh_public, header.dh_public);
        assert_eq!(decrypted.message_number, 42);
        assert_eq!(decrypted.previous_chain_length, 7);
        assert!(decrypted.kem_ciphertext.is_none());
        assert!(decrypted.kem_public.is_none());
    }

    #[test]
    fn encrypt_decrypt_with_pq_fields() {
        let key = make_header_key(0x55);
        let header = make_pq_header();
        let orig_ct_bytes = header.kem_ciphertext.as_ref().unwrap().to_bytes();
        let orig_pk_bytes = header.kem_public.as_ref().unwrap().to_bytes();

        let (encrypted, nonce) = encrypt_header(&key, &header).expect("encrypt failed");
        let decrypted = decrypt_header(&key, &encrypted, &nonce).expect("decrypt failed");

        assert_eq!(decrypted.message_number, 100);
        assert!(decrypted.kem_ciphertext.is_some());
        assert!(decrypted.kem_public.is_some());
        assert_eq!(decrypted.kem_ciphertext.unwrap().to_bytes(), orig_ct_bytes);
        assert_eq!(decrypted.kem_public.unwrap().to_bytes(), orig_pk_bytes);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key = make_header_key(0x01);
        let wrong_key = make_header_key(0x02);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&key, &header).expect("encrypt failed");
        let result = decrypt_header(&wrong_key, &encrypted, &nonce);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("decrypt failed"));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = make_header_key(0x03);
        let header = make_simple_header();

        let (mut encrypted, nonce) = encrypt_header(&key, &header).expect("encrypt failed");
        encrypted[0] ^= 0xFF;

        let result = decrypt_header(&key, &encrypted, &nonce);
        assert!(result.is_err());
    }

    #[test]
    fn wrong_nonce_fails() {
        let key = make_header_key(0x04);
        let header = make_simple_header();

        let (encrypted, _nonce) = encrypt_header(&key, &header).expect("encrypt failed");
        let wrong_nonce = [0xFFu8; NONCE_SIZE];

        let result = decrypt_header(&key, &encrypted, &wrong_nonce);
        assert!(result.is_err());
    }

    // ── try_decrypt_header ──────────────────────────────────────────

    #[test]
    fn try_decrypt_finds_correct_key() {
        let first = make_header_key(0x10);
        let second = make_header_key(0x20);
        let third = make_header_key(0x30);
        let header = make_simple_header();

        // Encrypt with second key
        let (encrypted, nonce) = encrypt_header(&second, &header).expect("encrypt failed");

        let candidates = [first, second, third];
        let (decrypted, idx) =
            try_decrypt_header(&candidates, &encrypted, &nonce).expect("try_decrypt failed");

        assert_eq!(idx, 1);
        assert_eq!(decrypted.message_number, 42);
    }

    #[test]
    fn try_decrypt_first_key() {
        let current = make_header_key(0xAA);
        let previous = make_header_key(0xBB);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&current, &header).expect("encrypt failed");

        let candidates = [current, previous];
        let (_, idx) =
            try_decrypt_header(&candidates, &encrypted, &nonce).expect("try_decrypt failed");

        assert_eq!(idx, 0);
    }

    #[test]
    fn try_decrypt_last_key() {
        let current = make_header_key(0xCC);
        let previous = make_header_key(0xDD);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&previous, &header).expect("encrypt failed");

        let candidates = [current, previous];
        let (_, idx) =
            try_decrypt_header(&candidates, &encrypted, &nonce).expect("try_decrypt failed");

        assert_eq!(idx, 1);
    }

    #[test]
    fn try_decrypt_no_matching_key() {
        let current = make_header_key(0x01);
        let previous = make_header_key(0x02);
        let encrypt_key = make_header_key(0xFF);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&encrypt_key, &header).expect("encrypt failed");

        let candidates = [current, previous];
        let result = try_decrypt_header(&candidates, &encrypted, &nonce);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no header key"));
    }

    #[test]
    fn try_decrypt_empty_keys() {
        let key = make_header_key(0x42);
        let header = make_simple_header();

        let (encrypted, nonce) = encrypt_header(&key, &header).expect("encrypt failed");

        let keys: &[HeaderKey] = &[];
        let result = try_decrypt_header(keys, &encrypted, &nonce);

        assert!(result.is_err());
    }

    // ── HeaderKey ───────────────────────────────────────────────────

    #[test]
    fn header_key_debug_redacts() {
        let key = make_header_key(0xAB);
        let debug = format!("{key:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("171")); // 0xAB = 171
    }

    #[test]
    fn header_key_as_bytes() {
        let key = make_header_key(0x99);
        assert_eq!(key.as_bytes(), &[0x99u8; KEY_SIZE]);
    }

    #[test]
    fn header_key_clone() {
        let key = make_header_key(0x77);
        let cloned = key.clone();
        assert_eq!(key.as_bytes(), cloned.as_bytes());
    }
}
