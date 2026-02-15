//! Wire protocol encoding for key exchange messages.
//!
//! Bridges the binary crypto types ([`PreKeyBundle`], [`X3DHInitMessage`])
//! to the text-based pirc wire protocol using base64 encoding. The pirc
//! protocol is text-based (IRC-style), so binary cryptographic data must
//! be encoded as ASCII-safe strings for transport.
//!
//! # Message flow
//!
//! A typical key exchange proceeds as:
//!
//! 1. Alice sends `RequestBundle` to request Bob's pre-key bundle
//! 2. Bob responds with `Bundle` containing his [`PreKeyBundle`]
//! 3. Alice performs X3DH and sends `InitMessage` with the [`X3DHInitMessage`]
//! 4. Bob processes the init message and sends `Complete`

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use crate::error::{CryptoError, Result};
use crate::prekey::PreKeyBundle;
use crate::x3dh::X3DHInitMessage;

/// A key exchange protocol message that can be serialized for wire transport.
///
/// Each variant maps to a stage of the X3DH-inspired key exchange protocol.
/// Binary payloads are encoded as base64 for the text-based wire format.
#[derive(Debug)]
pub enum KeyExchangeMessage {
    /// Request a user's pre-key bundle. No payload — the target is
    /// conveyed by the protocol message framing.
    RequestBundle,

    /// A pre-key bundle response.
    Bundle(Box<PreKeyBundle>),

    /// The X3DH init message from sender to receiver.
    InitMessage(Box<X3DHInitMessage>),

    /// Acknowledgment that the session is established.
    Complete,
}

/// Message type tag bytes used in the serialized format.
const TAG_REQUEST_BUNDLE: u8 = 0;
const TAG_BUNDLE: u8 = 1;
const TAG_INIT_MESSAGE: u8 = 2;
const TAG_COMPLETE: u8 = 3;

impl KeyExchangeMessage {
    /// Serialize this message to a byte vector.
    ///
    /// Format: `[tag (1) | payload...]`
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::RequestBundle => vec![TAG_REQUEST_BUNDLE],
            Self::Bundle(bundle) => {
                let payload = bundle.to_bytes();
                let mut bytes = Vec::with_capacity(1 + payload.len());
                bytes.push(TAG_BUNDLE);
                bytes.extend_from_slice(&payload);
                bytes
            }
            Self::InitMessage(init) => {
                let payload = init.to_bytes();
                let mut bytes = Vec::with_capacity(1 + payload.len());
                bytes.push(TAG_INIT_MESSAGE);
                bytes.extend_from_slice(&payload);
                bytes
            }
            Self::Complete => vec![TAG_COMPLETE],
        }
    }

    /// Deserialize from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is empty,
    /// has an unknown tag, or the payload is malformed.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(CryptoError::Serialization(
                "KeyExchangeMessage: empty data".into(),
            ));
        }

        match bytes[0] {
            TAG_REQUEST_BUNDLE => Ok(Self::RequestBundle),
            TAG_BUNDLE => {
                let bundle = PreKeyBundle::from_bytes(&bytes[1..])?;
                Ok(Self::Bundle(Box::new(bundle)))
            }
            TAG_INIT_MESSAGE => {
                let init = X3DHInitMessage::from_bytes(&bytes[1..])?;
                Ok(Self::InitMessage(Box::new(init)))
            }
            TAG_COMPLETE => Ok(Self::Complete),
            tag => Err(CryptoError::Serialization(format!(
                "KeyExchangeMessage: unknown tag {tag}"
            ))),
        }
    }
}

/// Encode binary data as base64 for the text-based wire protocol.
#[must_use]
pub fn encode_for_wire(data: &[u8]) -> String {
    BASE64.encode(data)
}

/// Decode base64-encoded data from the wire protocol.
///
/// # Errors
///
/// Returns [`CryptoError::Serialization`] if the input is not valid base64.
pub fn decode_from_wire(encoded: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(encoded)
        .map_err(|e| CryptoError::Serialization(format!("base64 decode error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityKeyPair;
    use crate::prekey::{KemPreKey, OneTimePreKey, SignedPreKey};
    use crate::x3dh::x3dh_sender;

    // ── Encoding helpers ─────────────────────────────────────────────

    #[test]
    fn encode_decode_roundtrip() {
        let data = b"hello, wire protocol!";
        let encoded = encode_for_wire(data);
        let decoded = decode_from_wire(&encoded).expect("decode failed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn encode_empty_data() {
        let encoded = encode_for_wire(b"");
        let decoded = decode_from_wire(&encoded).expect("decode failed");
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_invalid_base64_fails() {
        let result = decode_from_wire("not valid base64!!!");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("base64"));
    }

    #[test]
    fn encode_produces_ascii_safe_string() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = encode_for_wire(&data);
        assert!(encoded.is_ascii());
    }

    // ── KeyExchangeMessage: RequestBundle ────────────────────────────

    #[test]
    fn request_bundle_roundtrip() {
        let msg = KeyExchangeMessage::RequestBundle;
        let bytes = msg.to_bytes();
        let restored = KeyExchangeMessage::from_bytes(&bytes).expect("deserialize failed");
        assert!(matches!(restored, KeyExchangeMessage::RequestBundle));
    }

    // ── KeyExchangeMessage: Complete ─────────────────────────────────

    #[test]
    fn complete_roundtrip() {
        let msg = KeyExchangeMessage::Complete;
        let bytes = msg.to_bytes();
        let restored = KeyExchangeMessage::from_bytes(&bytes).expect("deserialize failed");
        assert!(matches!(restored, KeyExchangeMessage::Complete));
    }

    // ── KeyExchangeMessage: Bundle ───────────────────────────────────

    fn make_bundle() -> PreKeyBundle {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 1_700_000_000).expect("spk");
        let kpk = KemPreKey::generate(1, &identity).expect("kpk");
        let otpk = OneTimePreKey::generate(1);
        PreKeyBundle::new(
            identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            Some(otpk.to_public()),
        )
    }

    #[test]
    fn bundle_message_roundtrip() {
        let bundle = make_bundle();
        let msg = KeyExchangeMessage::Bundle(Box::new(bundle));
        let bytes = msg.to_bytes();
        let restored = KeyExchangeMessage::from_bytes(&bytes).expect("deserialize failed");
        assert!(matches!(restored, KeyExchangeMessage::Bundle(_)));

        // Verify the restored bundle is valid
        if let KeyExchangeMessage::Bundle(b) = restored {
            b.validate().expect("bundle validation failed");
        }
    }

    #[test]
    fn bundle_wire_encoding_roundtrip() {
        let bundle = make_bundle();
        let msg = KeyExchangeMessage::Bundle(Box::new(bundle));
        let bytes = msg.to_bytes();

        let encoded = encode_for_wire(&bytes);
        let decoded = decode_from_wire(&encoded).expect("decode failed");
        let restored = KeyExchangeMessage::from_bytes(&decoded).expect("deserialize failed");

        assert!(matches!(restored, KeyExchangeMessage::Bundle(_)));
        if let KeyExchangeMessage::Bundle(b) = restored {
            b.validate().expect("bundle validation failed after wire roundtrip");
        }
    }

    // ── KeyExchangeMessage: InitMessage ──────────────────────────────

    fn make_init_message() -> X3DHInitMessage {
        let alice_identity = IdentityKeyPair::generate();
        let bob_identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &bob_identity, 1_700_000_000).expect("spk");
        let kpk = KemPreKey::generate(1, &bob_identity).expect("kpk");
        let otpk = OneTimePreKey::generate(1);
        let bundle = PreKeyBundle::new(
            bob_identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            Some(otpk.to_public()),
        );

        let (_, init_msg) = x3dh_sender(&alice_identity, &bundle).expect("x3dh sender failed");
        init_msg
    }

    #[test]
    fn init_message_roundtrip() {
        let init = make_init_message();
        let msg = KeyExchangeMessage::InitMessage(Box::new(init));
        let bytes = msg.to_bytes();
        let restored = KeyExchangeMessage::from_bytes(&bytes).expect("deserialize failed");
        assert!(matches!(restored, KeyExchangeMessage::InitMessage(_)));
    }

    #[test]
    fn init_message_wire_encoding_roundtrip() {
        let init = make_init_message();
        let spk_id = init.used_signed_pre_key_id();
        let kem_pk_id = init.used_kem_pre_key_id();

        let msg = KeyExchangeMessage::InitMessage(Box::new(init));
        let bytes = msg.to_bytes();

        let encoded = encode_for_wire(&bytes);
        let decoded = decode_from_wire(&encoded).expect("decode failed");
        let restored = KeyExchangeMessage::from_bytes(&decoded).expect("deserialize failed");

        if let KeyExchangeMessage::InitMessage(init) = restored {
            assert_eq!(init.used_signed_pre_key_id(), spk_id);
            assert_eq!(init.used_kem_pre_key_id(), kem_pk_id);
        } else {
            panic!("expected InitMessage variant");
        }
    }

    // ── Error cases ──────────────────────────────────────────────────

    #[test]
    fn from_bytes_empty_fails() {
        let result = KeyExchangeMessage::from_bytes(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn from_bytes_unknown_tag_fails() {
        let result = KeyExchangeMessage::from_bytes(&[255]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown tag"));
    }

    #[test]
    fn from_bytes_truncated_bundle_fails() {
        let result = KeyExchangeMessage::from_bytes(&[TAG_BUNDLE, 0, 0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn from_bytes_truncated_init_message_fails() {
        let result = KeyExchangeMessage::from_bytes(&[TAG_INIT_MESSAGE, 0, 0, 0]);
        assert!(result.is_err());
    }

    // ── Full flow test ───────────────────────────────────────────────

    #[test]
    fn full_key_exchange_wire_flow() {
        // Run on a thread with a larger stack to accommodate ML-DSA key
        // generation which uses significant stack space.
        let result = std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(|| {
                // 1. Alice requests Bob's bundle
                let request = KeyExchangeMessage::RequestBundle;
                let request_wire = encode_for_wire(&request.to_bytes());

                let request_bytes = decode_from_wire(&request_wire).expect("decode request");
                let request_msg =
                    KeyExchangeMessage::from_bytes(&request_bytes).expect("parse request");
                assert!(matches!(request_msg, KeyExchangeMessage::RequestBundle));

                // 2. Bob sends his bundle
                let bob_identity = IdentityKeyPair::generate();
                let spk =
                    SignedPreKey::generate(1, &bob_identity, 1_700_000_000).expect("spk");
                let kpk = KemPreKey::generate(1, &bob_identity).expect("kpk");
                let otpk = OneTimePreKey::generate(1);
                let bundle = PreKeyBundle::new(
                    bob_identity.public_identity(),
                    spk.to_public(),
                    kpk.to_public(),
                    Some(otpk.to_public()),
                );

                let bundle_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
                let bundle_wire = encode_for_wire(&bundle_msg.to_bytes());

                // Alice receives and decodes the bundle
                let bundle_bytes = decode_from_wire(&bundle_wire).expect("decode bundle");
                let bundle_decoded =
                    KeyExchangeMessage::from_bytes(&bundle_bytes).expect("parse bundle");
                let received_bundle = match bundle_decoded {
                    KeyExchangeMessage::Bundle(b) => {
                        b.validate().expect("bundle validation");
                        b
                    }
                    _ => panic!("expected Bundle"),
                };

                // 3. Alice performs X3DH and sends init message
                let alice_identity = IdentityKeyPair::generate();
                let (_, init_msg) =
                    x3dh_sender(&alice_identity, &received_bundle).expect("x3dh sender");

                let init_wire_msg = KeyExchangeMessage::InitMessage(Box::new(init_msg));
                let init_wire = encode_for_wire(&init_wire_msg.to_bytes());

                // Bob receives and decodes the init message
                let init_bytes = decode_from_wire(&init_wire).expect("decode init");
                let init_decoded =
                    KeyExchangeMessage::from_bytes(&init_bytes).expect("parse init");
                assert!(matches!(init_decoded, KeyExchangeMessage::InitMessage(_)));

                // 4. Bob sends complete
                let complete = KeyExchangeMessage::Complete;
                let complete_wire = encode_for_wire(&complete.to_bytes());

                let complete_bytes =
                    decode_from_wire(&complete_wire).expect("decode complete");
                let complete_decoded =
                    KeyExchangeMessage::from_bytes(&complete_bytes).expect("parse complete");
                assert!(matches!(complete_decoded, KeyExchangeMessage::Complete));
            })
            .expect("thread spawn failed")
            .join();

        result.expect("full_key_exchange_wire_flow panicked");
    }
}
