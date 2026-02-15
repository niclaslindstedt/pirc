//! Cryptographic primitives and triple ratchet protocol for pirc.
//!
//! This crate implements the triple ratchet encryption protocol with
//! post-quantum resistant primitives:
//!
//! - **Identity** — [`identity`] long-term identity key management
//! - **Pre-keys** — [`prekey`] X3DH-style pre-key bundles with PQ extension
//! - **Key exchange** — [`x25519`] X25519 Diffie-Hellman, [`x3dh`] X3DH + PQ extension
//! - **Encryption** — [`aead`] AES-256-GCM authenticated encryption
//! - **Key derivation** — [`kdf`] HKDF-SHA-256
//! - **Key storage** — [`key_storage`] encrypted-at-rest key storage
//! - **Key rotation** — [`rotation`] pre-key rotation policy and scheduling
//! - **Post-quantum KEM** — [`kem`] ML-KEM (Kyber) key encapsulation
//! - **Post-quantum signatures** — [`signing`] ML-DSA (Dilithium) digital signatures
//! - **Ratchets** — [`dh_ratchet`], [`symmetric_ratchet`], [`pq_ratchet`]
//! - **Session** — [`triple_ratchet`] combined session state machine
//! - **Wire format** — [`header`] encryption and [`message`] types
//! - **Protocol** — [`protocol`] wire protocol encoding for key exchange

pub mod aead;
pub mod dh_ratchet;
pub mod error;
pub mod header;
pub mod identity;
pub mod kdf;
pub mod key_storage;
pub mod prekey;
pub mod kem;
pub mod message;
pub mod pq_ratchet;
pub mod protocol;
pub mod rotation;
pub mod signing;
pub mod symmetric_ratchet;
pub mod triple_ratchet;
pub mod x25519;
pub mod x3dh;

pub use error::{CryptoError, Result};
