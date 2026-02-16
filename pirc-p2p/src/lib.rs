//! P2P connectivity with STUN/TURN NAT traversal for pirc.
//!
//! This crate provides peer-to-peer connection establishment using:
//!
//! - **STUN** — [`stun`] RFC 5389 binding requests for server-reflexive address discovery
//! - **Error handling** — [`error`] error types for P2P operations

pub mod error;
pub mod stun;

pub use error::{P2pError, Result};
pub use stun::{discover_reflexive_address, StunAttribute, StunMessage, TransactionId};
