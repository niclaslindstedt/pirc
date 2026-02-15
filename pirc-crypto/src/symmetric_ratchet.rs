//! Symmetric-key ratchet (KDF chain).
//!
//! Implements the sending and receiving KDF chains that derive per-message
//! keys. Each chain step advances the chain key through HKDF and outputs
//! a message key for AES-256-GCM encryption.
