//! Key exchange wire protocol conformance tests.
//!
//! These tests verify the key exchange extension messages (KEYEXCHANGE,
//! KEYEXCHANGE-ACK, KEYEXCHANGE-COMPLETE, FINGERPRINT) serialize and
//! parse correctly through the public API.

use pirc_protocol::{parse, Command, Message, PircSubcommand, Prefix};

// ============================================================================
// Helpers
// ============================================================================

/// Parse input, serialize, re-parse, and assert equality.
fn assert_roundtrip(input: &str) {
    let parsed = parse(input).expect("initial parse failed");
    let wire = format!("{parsed}\r\n");
    let reparsed = parse(&wire).expect("re-parse failed");
    assert_eq!(parsed, reparsed, "round-trip mismatch for input: {input:?}");
}

/// Build a message, serialize, parse, and assert equality.
fn assert_build_roundtrip(msg: &Message) {
    let wire = format!("{msg}\r\n");
    let parsed = parse(&wire).expect("parse of built message failed");
    assert_eq!(*msg, parsed, "build round-trip mismatch for wire: {wire:?}");
}

// ============================================================================
// 1. Key exchange wire protocol messages
// ============================================================================

#[test]
fn keyexchange_with_base64_payload_roundtrip() {
    // Simulate a KEYEXCHANGE message carrying a base64-encoded pre-key bundle
    let payload = "SGVsbG8gV29ybGQ="; // "Hello World" in base64
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
        .param("bob")
        .param(payload)
        .build();
    assert_build_roundtrip(&msg);
}

#[test]
fn keyexchange_ack_with_base64_payload_roundtrip() {
    let payload = "dGVzdCBkYXRh"; // "test data" in base64
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchangeAck))
        .param("alice")
        .param(payload)
        .build();
    assert_build_roundtrip(&msg);
}

#[test]
fn keyexchange_complete_roundtrip() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchangeComplete))
        .param("alice")
        .build();
    assert_build_roundtrip(&msg);
}

#[test]
fn keyexchange_parse_extracts_base64_payload() {
    let input = "PIRC KEYEXCHANGE bob SGVsbG8gV29ybGQ=\r\n";
    let msg = parse(input).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
    assert_eq!(msg.params.len(), 2);
    assert_eq!(msg.params[0], "bob");
    assert_eq!(msg.params[1], "SGVsbG8gV29ybGQ=");
}

#[test]
fn keyexchange_ack_parse_extracts_base64_payload() {
    let input = "PIRC KEYEXCHANGE-ACK alice dGVzdCBkYXRh\r\n";
    let msg = parse(input).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchangeAck));
    assert_eq!(msg.params.len(), 2);
    assert_eq!(msg.params[0], "alice");
    assert_eq!(msg.params[1], "dGVzdCBkYXRh");
}

#[test]
fn keyexchange_with_prefix_roundtrip() {
    let msg = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
        .prefix(Prefix::User {
            nick: pirc_common::Nickname::new("alice").unwrap(),
            user: "alice".into(),
            host: "example.com".into(),
        })
        .param("bob")
        .param("AQID") // base64 for [1, 2, 3]
        .build();
    assert_build_roundtrip(&msg);
}

#[test]
fn keyexchange_large_base64_payload_roundtrip() {
    // Simulate a large payload (like a real pre-key bundle ~13KB base64)
    // but fit within 512 byte IRC limit by using trailing param
    let payload = "A".repeat(200); // 200 chars of valid base64
    let input = format!("PIRC KEYEXCHANGE bob :{payload}\r\n");
    assert_roundtrip(&input);
}

#[test]
fn fingerprint_with_hex_payload_roundtrip() {
    let fingerprint = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
    let msg = Message::builder(Command::Pirc(PircSubcommand::Fingerprint))
        .param("bob")
        .param(fingerprint)
        .build();
    assert_build_roundtrip(&msg);
}
