//! Diffie-Hellman ratchet.
//!
//! Manages the X25519 ratchet key pairs and performs DH exchanges on
//! each message turn. Each DH output is fed into the KDF chain to
//! derive new sending and receiving chain keys, providing forward secrecy.
