//! Tests for `MessageEnvelope` serialization.

use super::super::envelope::ENVELOPE_HEADER_SIZE;
use super::super::*;

// ── MessageEnvelope tests ───────────────────────────────────────

#[test]
fn envelope_roundtrip() {
    let envelope = MessageEnvelope {
        sequence_number: 42,
        timestamp_ms: 1_700_000_000_000,
        plaintext: b"hello group".to_vec(),
    };

    let bytes = envelope.to_bytes();
    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();

    assert_eq!(restored.sequence_number, 42);
    assert_eq!(restored.timestamp_ms, 1_700_000_000_000);
    assert_eq!(restored.plaintext, b"hello group");
}

#[test]
fn envelope_empty_plaintext() {
    let envelope = MessageEnvelope {
        sequence_number: 1,
        timestamp_ms: 0,
        plaintext: vec![],
    };

    let bytes = envelope.to_bytes();
    assert_eq!(bytes.len(), ENVELOPE_HEADER_SIZE);

    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();
    assert!(restored.plaintext.is_empty());
}

#[test]
fn envelope_too_short() {
    let result = MessageEnvelope::from_bytes(&[0u8; 10]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too short"));
}

#[test]
fn envelope_boundary_values() {
    let envelope = MessageEnvelope {
        sequence_number: u64::MAX,
        timestamp_ms: u64::MAX,
        plaintext: b"x".to_vec(),
    };

    let bytes = envelope.to_bytes();
    let restored = MessageEnvelope::from_bytes(&bytes).unwrap();
    assert_eq!(restored.sequence_number, u64::MAX);
    assert_eq!(restored.timestamp_ms, u64::MAX);
}
