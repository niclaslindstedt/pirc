//! Combined triple ratchet session.
//!
//! Ties together the DH ratchet, symmetric-key ratchet, and post-quantum
//! ratchet into a single session state machine. Manages session
//! initialization, message encryption/decryption, ratchet advancement,
//! and out-of-order message handling.
