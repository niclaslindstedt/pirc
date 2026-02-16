//! TURN types, constants, and data structures.

use std::net::SocketAddr;

use crate::stun::TransactionId;

// --- RFC 5766 TURN method types ---
// TURN methods use the STUN message format with different method codes.

/// Allocate Request (0x0003).
pub(crate) const ALLOCATE_REQUEST: u16 = 0x0003;

/// Allocate Success Response (0x0103).
pub(crate) const ALLOCATE_RESPONSE: u16 = 0x0103;

/// Allocate Error Response (0x0113).
pub(crate) const ALLOCATE_ERROR_RESPONSE: u16 = 0x0113;

/// Refresh Request (0x0004).
pub(crate) const REFRESH_REQUEST: u16 = 0x0004;

/// Refresh Success Response (0x0104).
pub(crate) const REFRESH_RESPONSE: u16 = 0x0104;

/// `CreatePermission` Request (0x0008).
pub(crate) const CREATE_PERMISSION_REQUEST: u16 = 0x0008;

/// `CreatePermission` Success Response (0x0108).
pub(crate) const CREATE_PERMISSION_RESPONSE: u16 = 0x0108;

/// `CreatePermission` Error Response (0x0118).
pub(crate) const CREATE_PERMISSION_ERROR_RESPONSE: u16 = 0x0118;

/// `ChannelBind` Request (0x0009).
pub(crate) const CHANNEL_BIND_REQUEST: u16 = 0x0009;

/// `ChannelBind` Success Response (0x0109).
pub(crate) const CHANNEL_BIND_RESPONSE: u16 = 0x0109;

/// `ChannelBind` Error Response (0x0119).
pub(crate) const CHANNEL_BIND_ERROR_RESPONSE: u16 = 0x0119;

/// Send Indication (0x0016).
pub(crate) const SEND_INDICATION: u16 = 0x0016;

/// Data Indication (0x0017).
pub(crate) const DATA_INDICATION: u16 = 0x0017;

// --- TURN / STUN attribute types ---

/// `CHANNEL-NUMBER` attribute (0x000C).
pub(crate) const ATTR_CHANNEL_NUMBER: u16 = 0x000C;

/// `LIFETIME` attribute (0x000D).
pub(crate) const ATTR_LIFETIME: u16 = 0x000D;

/// `XOR-PEER-ADDRESS` attribute (0x0012).
pub(crate) const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;

/// `DATA` attribute (0x0013).
pub(crate) const ATTR_DATA: u16 = 0x0013;

/// `XOR-RELAYED-ADDRESS` attribute (0x0016).
pub(crate) const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;

/// `REQUESTED-TRANSPORT` attribute (0x0019).
pub(crate) const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;

/// `USERNAME` attribute (0x0006).
pub(crate) const ATTR_USERNAME: u16 = 0x0006;

/// `REALM` attribute (0x0014).
pub(crate) const ATTR_REALM: u16 = 0x0014;

/// `NONCE` attribute (0x0015).
pub(crate) const ATTR_NONCE: u16 = 0x0015;

/// `MESSAGE-INTEGRITY` attribute (0x0008).
pub(crate) const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;

/// `ERROR-CODE` attribute (0x0009).
pub(crate) const ATTR_ERROR_CODE: u16 = 0x0009;

/// `XOR-MAPPED-ADDRESS` attribute (0x0020).
pub(crate) const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// STUN magic cookie.
pub(crate) const MAGIC_COOKIE: u32 = 0x2112_A442;

/// STUN header size.
pub(crate) const HEADER_SIZE: usize = 20;

/// Transport protocol: UDP (17).
pub(crate) const TRANSPORT_UDP: u8 = 17;

/// Address family: IPv4.
pub(crate) const FAMILY_IPV4: u8 = 0x01;

/// Address family: IPv6.
pub(crate) const FAMILY_IPV6: u8 = 0x02;

/// Default TURN request timeout in milliseconds.
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Maximum retransmissions for a TURN request.
pub(crate) const MAX_RETRANSMISSIONS: u32 = 3;

/// HMAC-SHA1 digest length (20 bytes).
pub(crate) const HMAC_SHA1_LEN: usize = 20;

// --- TURN attribute types ---

/// A TURN-specific attribute in a STUN/TURN message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAttribute {
    /// `USERNAME` — long-term credential username.
    Username(String),
    /// `REALM` — authentication realm.
    Realm(String),
    /// `NONCE` — server-provided nonce.
    Nonce(String),
    /// `MESSAGE-INTEGRITY` — HMAC-SHA1 over the message.
    MessageIntegrity([u8; HMAC_SHA1_LEN]),
    /// `LIFETIME` — allocation lifetime in seconds.
    Lifetime(u32),
    /// `XOR-RELAYED-ADDRESS` — the relay address allocated by the TURN server.
    XorRelayedAddress(SocketAddr),
    /// `XOR-MAPPED-ADDRESS` — server-reflexive address.
    XorMappedAddress(SocketAddr),
    /// `XOR-PEER-ADDRESS` — peer address for permissions / channel binds.
    XorPeerAddress(SocketAddr),
    /// `REQUESTED-TRANSPORT` — transport protocol (UDP = 17).
    RequestedTransport(u8),
    /// `CHANNEL-NUMBER` — channel number for `ChannelBind` (0x4000–0x7FFF).
    ChannelNumber(u16),
    /// `DATA` — application data in Send/Data indications.
    Data(Vec<u8>),
    /// `ERROR-CODE` — error class, number, and reason phrase.
    ErrorCode(u16, String),
    /// Unknown attribute (type, raw bytes).
    Unknown(u16, Vec<u8>),
}

/// A TURN message (extends STUN message format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnMessage {
    /// Message type (method + class).
    pub msg_type: u16,
    /// Transaction ID (12 bytes).
    pub transaction_id: TransactionId,
    /// Attributes contained in the message.
    pub attributes: Vec<TurnAttribute>,
}

/// Long-term credentials for TURN authentication.
#[derive(Debug, Clone)]
pub struct TurnCredentials {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Realm (provided by server in 401 response).
    pub realm: String,
    /// Nonce (provided by server in 401 response).
    pub nonce: String,
}

/// Result of a successful TURN allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    /// The relay address allocated by the TURN server.
    pub relay_addr: SocketAddr,
    /// Server-reflexive address (optional, from `XOR-MAPPED-ADDRESS`).
    pub mapped_addr: Option<SocketAddr>,
    /// Allocation lifetime in seconds.
    pub lifetime: u32,
}
