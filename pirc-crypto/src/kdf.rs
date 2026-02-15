//! HKDF-based key derivation.
//!
//! Wraps HKDF-SHA-256 to derive symmetric keys and chain keys from
//! shared secrets produced by X25519 or ML-KEM. Implements the
//! KDF chains used by the symmetric-key ratchet.
