//! Post-quantum ratchet.
//!
//! Implements the third ratchet in the triple ratchet protocol, using
//! ML-KEM key encapsulation to periodically inject post-quantum keying
//! material into the KDF chain. This provides long-term resistance
//! against quantum computing attacks.
