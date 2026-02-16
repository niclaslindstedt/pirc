//! P2P connectivity with STUN/TURN NAT traversal for pirc.
//!
//! This crate provides peer-to-peer connection establishment using:
//!
//! - **STUN** — [`stun`] RFC 5389 binding requests for server-reflexive address discovery
//! - **TURN** — [`turn`] RFC 5766 relay client for NAT traversal fallback
//! - **ICE** — [`ice`] ICE-lite candidate gathering and connectivity types
//! - **Error handling** — [`error`] error types for P2P operations

pub mod error;
pub mod ice;
pub mod stun;
pub mod turn;

pub use error::{P2pError, Result};
pub use ice::{
    compute_priority, CandidateGatherer, CandidateType, GathererConfig, IceCandidate,
};
pub use stun::{discover_reflexive_address, StunAttribute, StunMessage, TransactionId};
pub use turn::{
    allocate, channel_bind, compute_long_term_key, create_permission, decode_channel_data,
    encode_channel_data, parse_data_indication, send_to_peer, Allocation, TurnAttribute,
    TurnCredentials, TurnMessage,
};
