//! Header encryption.
//!
//! Encrypts and decrypts message headers to hide ratchet metadata
//! (public keys, message numbers, previous chain length) from
//! observers. Uses a separate header key derived from the KDF chain.
