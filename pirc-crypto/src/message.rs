//! Message types and serialization.
//!
//! Defines the wire format for encrypted messages, including message
//! headers, ciphertexts, and associated metadata. Headers carry the
//! ratchet state needed by the receiver to derive the correct message
//! key. The [`EncryptedMessage`] bundles the encrypted header and
//! encrypted body for transmission.

use crate::aead::NONCE_SIZE;
use crate::error::{CryptoError, Result};
use crate::kem::{KemCiphertext, KemPublicKey, CIPHERTEXT_LEN, PUBLIC_KEY_LEN};
use crate::x25519;

/// A message header containing ratchet metadata.
///
/// The header is encrypted before transmission to prevent metadata
/// leakage. It carries the sender's current DH public key, message
/// counters, and optionally post-quantum KEM data when a PQ ratchet
/// step occurs.
#[derive(Clone, Debug)]
pub struct MessageHeader {
    /// Sender's current DH ratchet public key.
    pub dh_public: x25519::PublicKey,
    /// Message number in the current sending chain.
    pub message_number: u32,
    /// Number of messages in the previous sending chain.
    pub previous_chain_length: u32,
    /// PQ KEM ciphertext (present when a PQ ratchet step occurs).
    pub kem_ciphertext: Option<KemCiphertext>,
    /// Sender's new KEM public key (present when a PQ ratchet step occurs).
    pub kem_public: Option<KemPublicKey>,
}

/// Fixed portion of the serialized header:
/// 32 bytes DH public key + 4 bytes message number + 4 bytes previous chain length + 1 byte flags.
const HEADER_FIXED_LEN: usize = x25519::KEY_LEN + 4 + 4 + 1;

/// Flags byte layout:
/// bit 0: `kem_ciphertext` present
/// bit 1: `kem_public` present
const FLAG_KEM_CIPHERTEXT: u8 = 0x01;
const FLAG_KEM_PUBLIC: u8 = 0x02;

impl MessageHeader {
    /// Serialize the header to a byte vector.
    ///
    /// Wire format:
    /// ```text
    /// [32 bytes DH public key]
    /// [4 bytes message_number (big-endian)]
    /// [4 bytes previous_chain_length (big-endian)]
    /// [1 byte flags]
    /// [if flag 0: CIPHERTEXT_LEN bytes KEM ciphertext]
    /// [if flag 1: PUBLIC_KEY_LEN bytes KEM public key]
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut flags = 0u8;
        if self.kem_ciphertext.is_some() {
            flags |= FLAG_KEM_CIPHERTEXT;
        }
        if self.kem_public.is_some() {
            flags |= FLAG_KEM_PUBLIC;
        }

        let mut buf = Vec::with_capacity(self.serialized_len());
        buf.extend_from_slice(self.dh_public.as_bytes());
        buf.extend_from_slice(&self.message_number.to_be_bytes());
        buf.extend_from_slice(&self.previous_chain_length.to_be_bytes());
        buf.push(flags);

        if let Some(ref ct) = self.kem_ciphertext {
            buf.extend_from_slice(&ct.to_bytes());
        }
        if let Some(ref pk) = self.kem_public {
            buf.extend_from_slice(&pk.to_bytes());
        }

        buf
    }

    /// Deserialize a header from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the byte slice is too
    /// short or has an invalid format.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_FIXED_LEN {
            return Err(CryptoError::Serialization(format!(
                "header too short: expected at least {HEADER_FIXED_LEN} bytes, got {}",
                bytes.len()
            )));
        }

        let mut pos = 0;

        let mut dh_bytes = [0u8; x25519::KEY_LEN];
        dh_bytes.copy_from_slice(&bytes[pos..pos + x25519::KEY_LEN]);
        let dh_public = x25519::PublicKey::from_bytes(dh_bytes);
        pos += x25519::KEY_LEN;

        let message_number = u32::from_be_bytes([
            bytes[pos],
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
        ]);
        pos += 4;

        let previous_chain_length = u32::from_be_bytes([
            bytes[pos],
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
        ]);
        pos += 4;

        let flags = bytes[pos];
        pos += 1;

        let kem_ciphertext = if flags & FLAG_KEM_CIPHERTEXT != 0 {
            if bytes.len() < pos + CIPHERTEXT_LEN {
                return Err(CryptoError::Serialization(format!(
                    "header too short for KEM ciphertext: need {} more bytes, have {}",
                    CIPHERTEXT_LEN,
                    bytes.len() - pos
                )));
            }
            let ct = KemCiphertext::from_bytes(&bytes[pos..pos + CIPHERTEXT_LEN])?;
            pos += CIPHERTEXT_LEN;
            Some(ct)
        } else {
            None
        };

        let kem_public = if flags & FLAG_KEM_PUBLIC != 0 {
            if bytes.len() < pos + PUBLIC_KEY_LEN {
                return Err(CryptoError::Serialization(format!(
                    "header too short for KEM public key: need {} more bytes, have {}",
                    PUBLIC_KEY_LEN,
                    bytes.len() - pos
                )));
            }
            let pk = KemPublicKey::from_bytes(&bytes[pos..pos + PUBLIC_KEY_LEN])?;
            pos += PUBLIC_KEY_LEN;
            Some(pk)
        } else {
            None
        };

        let _ = pos; // suppress unused warning

        Ok(Self {
            dh_public,
            message_number,
            previous_chain_length,
            kem_ciphertext,
            kem_public,
        })
    }

    /// Returns the serialized byte length of this header.
    #[must_use]
    fn serialized_len(&self) -> usize {
        let mut len = HEADER_FIXED_LEN;
        if self.kem_ciphertext.is_some() {
            len += CIPHERTEXT_LEN;
        }
        if self.kem_public.is_some() {
            len += PUBLIC_KEY_LEN;
        }
        len
    }
}

/// An encrypted message ready for wire transmission.
///
/// Contains the encrypted header (which hides ratchet metadata) and the
/// encrypted message body, each with its own nonce.
#[derive(Clone, Debug)]
pub struct EncryptedMessage {
    /// AES-256-GCM encrypted header bytes (includes auth tag).
    pub encrypted_header: Vec<u8>,
    /// Nonce used for header encryption.
    pub header_nonce: [u8; NONCE_SIZE],
    /// AES-256-GCM encrypted message body (includes auth tag).
    pub ciphertext: Vec<u8>,
    /// Nonce used for body encryption.
    pub body_nonce: [u8; NONCE_SIZE],
}

impl EncryptedMessage {
    /// Serialize the encrypted message to a byte vector.
    ///
    /// Wire format:
    /// ```text
    /// [4 bytes encrypted_header length (big-endian)]
    /// [encrypted_header bytes]
    /// [12 bytes header_nonce]
    /// [4 bytes ciphertext length (big-endian)]
    /// [ciphertext bytes]
    /// [12 bytes body_nonce]
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let total = 4 + self.encrypted_header.len() + NONCE_SIZE
            + 4 + self.ciphertext.len() + NONCE_SIZE;
        let mut buf = Vec::with_capacity(total);

        let header_len = u32::try_from(self.encrypted_header.len())
            .expect("encrypted header exceeds u32::MAX bytes");
        buf.extend_from_slice(&header_len.to_be_bytes());
        buf.extend_from_slice(&self.encrypted_header);
        buf.extend_from_slice(&self.header_nonce);

        let ct_len = u32::try_from(self.ciphertext.len())
            .expect("ciphertext exceeds u32::MAX bytes");
        buf.extend_from_slice(&ct_len.to_be_bytes());
        buf.extend_from_slice(&self.ciphertext);
        buf.extend_from_slice(&self.body_nonce);

        buf
    }

    /// Deserialize an encrypted message from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the byte slice is too
    /// short or has invalid length fields.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut pos = 0;

        // Encrypted header
        if bytes.len() < pos + 4 {
            return Err(CryptoError::Serialization(
                "message too short for header length".into(),
            ));
        }
        let header_len = u32::from_be_bytes([
            bytes[pos],
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
        ]) as usize;
        pos += 4;

        if bytes.len() < pos + header_len {
            return Err(CryptoError::Serialization(
                "message too short for encrypted header".into(),
            ));
        }
        let encrypted_header = bytes[pos..pos + header_len].to_vec();
        pos += header_len;

        // Header nonce
        if bytes.len() < pos + NONCE_SIZE {
            return Err(CryptoError::Serialization(
                "message too short for header nonce".into(),
            ));
        }
        let mut header_nonce = [0u8; NONCE_SIZE];
        header_nonce.copy_from_slice(&bytes[pos..pos + NONCE_SIZE]);
        pos += NONCE_SIZE;

        // Ciphertext
        if bytes.len() < pos + 4 {
            return Err(CryptoError::Serialization(
                "message too short for ciphertext length".into(),
            ));
        }
        let ct_len = u32::from_be_bytes([
            bytes[pos],
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
        ]) as usize;
        pos += 4;

        if bytes.len() < pos + ct_len {
            return Err(CryptoError::Serialization(
                "message too short for ciphertext".into(),
            ));
        }
        let ciphertext = bytes[pos..pos + ct_len].to_vec();
        pos += ct_len;

        // Body nonce
        if bytes.len() < pos + NONCE_SIZE {
            return Err(CryptoError::Serialization(
                "message too short for body nonce".into(),
            ));
        }
        let mut body_nonce = [0u8; NONCE_SIZE];
        body_nonce.copy_from_slice(&bytes[pos..pos + NONCE_SIZE]);

        Ok(Self {
            encrypted_header,
            header_nonce,
            ciphertext,
            body_nonce,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::{KemCiphertext, KemKeyPair, KemPublicKey};

    fn make_dh_public() -> x25519::PublicKey {
        let kp = x25519::KeyPair::generate();
        kp.public_key()
    }

    fn make_kem_data() -> (KemCiphertext, KemPublicKey) {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();
        let (ct, _) = pk.encapsulate().expect("encapsulate failed");
        (ct, pk)
    }

    // ── MessageHeader serialization ─────────────────────────────────

    #[test]
    fn header_roundtrip_no_pq() {
        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: 42,
            previous_chain_length: 7,
            kem_ciphertext: None,
            kem_public: None,
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), HEADER_FIXED_LEN);

        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.dh_public, header.dh_public);
        assert_eq!(restored.message_number, 42);
        assert_eq!(restored.previous_chain_length, 7);
        assert!(restored.kem_ciphertext.is_none());
        assert!(restored.kem_public.is_none());
    }

    #[test]
    fn header_roundtrip_with_pq() {
        let (ct, pk) = make_kem_data();

        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: 100,
            previous_chain_length: 50,
            kem_ciphertext: Some(ct),
            kem_public: Some(pk),
        };

        let bytes = header.to_bytes();
        assert_eq!(
            bytes.len(),
            HEADER_FIXED_LEN + CIPHERTEXT_LEN + PUBLIC_KEY_LEN
        );

        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.dh_public, header.dh_public);
        assert_eq!(restored.message_number, 100);
        assert_eq!(restored.previous_chain_length, 50);
        assert!(restored.kem_ciphertext.is_some());
        assert!(restored.kem_public.is_some());

        // Verify KEM data round-trips
        let orig_ct_bytes = header.kem_ciphertext.unwrap().to_bytes();
        let restored_ct_bytes = restored.kem_ciphertext.unwrap().to_bytes();
        assert_eq!(orig_ct_bytes, restored_ct_bytes);

        let orig_pk_bytes = header.kem_public.unwrap().to_bytes();
        let restored_pk_bytes = restored.kem_public.unwrap().to_bytes();
        assert_eq!(orig_pk_bytes, restored_pk_bytes);
    }

    #[test]
    fn header_roundtrip_ciphertext_only() {
        let (ct, _) = make_kem_data();

        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: 1,
            previous_chain_length: 0,
            kem_ciphertext: Some(ct),
            kem_public: None,
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), HEADER_FIXED_LEN + CIPHERTEXT_LEN);

        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert!(restored.kem_ciphertext.is_some());
        assert!(restored.kem_public.is_none());
    }

    #[test]
    fn header_roundtrip_public_key_only() {
        let kp = KemKeyPair::generate();
        let pk = kp.public_key();

        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: 5,
            previous_chain_length: 3,
            kem_ciphertext: None,
            kem_public: Some(pk),
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), HEADER_FIXED_LEN + PUBLIC_KEY_LEN);

        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert!(restored.kem_ciphertext.is_none());
        assert!(restored.kem_public.is_some());
    }

    #[test]
    fn header_from_bytes_too_short() {
        let bytes = [0u8; HEADER_FIXED_LEN - 1];
        let result = MessageHeader::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn header_from_bytes_truncated_kem_ciphertext() {
        // Create a valid header with kem_ciphertext flag set but truncated data
        let mut bytes = vec![0u8; HEADER_FIXED_LEN + 10]; // too short for ciphertext
        bytes[x25519::KEY_LEN + 4 + 4] = FLAG_KEM_CIPHERTEXT; // set flag

        let result = MessageHeader::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn header_message_numbers_at_boundaries() {
        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: u32::MAX,
            previous_chain_length: u32::MAX,
            kem_ciphertext: None,
            kem_public: None,
        };

        let bytes = header.to_bytes();
        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.message_number, u32::MAX);
        assert_eq!(restored.previous_chain_length, u32::MAX);
    }

    #[test]
    fn header_zero_counters() {
        let header = MessageHeader {
            dh_public: make_dh_public(),
            message_number: 0,
            previous_chain_length: 0,
            kem_ciphertext: None,
            kem_public: None,
        };

        let bytes = header.to_bytes();
        let restored = MessageHeader::from_bytes(&bytes).expect("deserialize failed");
        assert_eq!(restored.message_number, 0);
        assert_eq!(restored.previous_chain_length, 0);
    }

    // ── EncryptedMessage serialization ──────────────────────────────

    #[test]
    fn encrypted_message_roundtrip() {
        let msg = EncryptedMessage {
            encrypted_header: vec![1, 2, 3, 4, 5],
            header_nonce: [0xAA; NONCE_SIZE],
            ciphertext: vec![10, 20, 30, 40, 50, 60],
            body_nonce: [0xBB; NONCE_SIZE],
        };

        let bytes = msg.to_bytes();
        let restored = EncryptedMessage::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(restored.encrypted_header, msg.encrypted_header);
        assert_eq!(restored.header_nonce, msg.header_nonce);
        assert_eq!(restored.ciphertext, msg.ciphertext);
        assert_eq!(restored.body_nonce, msg.body_nonce);
    }

    #[test]
    fn encrypted_message_empty_payloads() {
        let msg = EncryptedMessage {
            encrypted_header: vec![],
            header_nonce: [0; NONCE_SIZE],
            ciphertext: vec![],
            body_nonce: [0; NONCE_SIZE],
        };

        let bytes = msg.to_bytes();
        let restored = EncryptedMessage::from_bytes(&bytes).expect("deserialize failed");

        assert!(restored.encrypted_header.is_empty());
        assert!(restored.ciphertext.is_empty());
    }

    #[test]
    fn encrypted_message_from_bytes_too_short() {
        let bytes = [0u8; 3]; // too short for even the first length field
        let result = EncryptedMessage::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn encrypted_message_truncated_header() {
        // Length says 100 bytes but only 10 available
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&100u32.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 10]);

        let result = EncryptedMessage::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn encrypted_message_large_payloads() {
        let msg = EncryptedMessage {
            encrypted_header: vec![0xCC; 2048],
            header_nonce: [0x11; NONCE_SIZE],
            ciphertext: vec![0xDD; 65536],
            body_nonce: [0x22; NONCE_SIZE],
        };

        let bytes = msg.to_bytes();
        let restored = EncryptedMessage::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(restored.encrypted_header.len(), 2048);
        assert_eq!(restored.ciphertext.len(), 65536);
    }
}
