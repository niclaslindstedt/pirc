//! P2P connectivity with STUN/TURN NAT traversal for pirc.
//!
//! This crate provides peer-to-peer connection establishment using:
//!
//! - **STUN** — [`stun`] RFC 5389 binding requests for server-reflexive address discovery
//! - **TURN** — [`turn`] RFC 5766 relay client for NAT traversal fallback
//! - **ICE** — [`ice`] ICE-lite candidate gathering and connectivity types
//! - **Connectivity** — [`connectivity`] ICE connectivity checks and UDP hole-punching
//! - **Session** — [`session`] P2P session state machine for connection lifecycle
//! - **Error handling** — [`error`] error types for P2P operations

pub mod connectivity;
pub mod error;
pub mod ice;
pub mod session;
pub mod stun;
pub mod turn;

pub use connectivity::{
    compute_pair_priority, form_pairs, CandidatePair, ConnectivityChecker, IceRole, PairState,
};
pub use error::{P2pError, Result};
pub use ice::{
    compute_priority, CandidateGatherer, CandidateType, GathererConfig, IceCandidate,
};
pub use session::{P2pSession, P2pSessionEvent, SessionState};
pub use stun::{discover_reflexive_address, StunAttribute, StunMessage, TransactionId};
pub use turn::{
    allocate, channel_bind, compute_long_term_key, create_permission, decode_channel_data,
    encode_channel_data, parse_data_indication, send_to_peer, Allocation, TurnAttribute,
    TurnCredentials, TurnMessage,
};
