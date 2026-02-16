//! STUN (RFC 5389) binding request/response implementation.
//!
//! Implements the subset of STUN needed for NAT traversal:
//! - Binding request serialization
//! - Binding response parsing with `XOR-MAPPED-ADDRESS` extraction
//! - UDP transport for sending requests and receiving responses

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use rand::Rng;
use tokio::net::UdpSocket;
use tracing::debug;

use crate::error::{P2pError, Result};

// --- RFC 5389 constants ---

/// STUN magic cookie (0x2112A442).
const MAGIC_COOKIE: u32 = 0x2112_A442;

/// STUN message header size: 20 bytes.
const HEADER_SIZE: usize = 20;

/// Transaction ID size: 12 bytes.
const TRANSACTION_ID_SIZE: usize = 12;

/// STUN Binding Request method type.
const BINDING_REQUEST: u16 = 0x0001;

/// STUN Binding Response (success) method type.
const BINDING_RESPONSE: u16 = 0x0101;

/// STUN Binding Error Response method type.
const BINDING_ERROR_RESPONSE: u16 = 0x0111;

/// `MAPPED-ADDRESS` attribute type.
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;

/// `XOR-MAPPED-ADDRESS` attribute type.
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// Address family: IPv4.
const FAMILY_IPV4: u8 = 0x01;

/// Address family: IPv6.
const FAMILY_IPV6: u8 = 0x02;

/// Default STUN request timeout in milliseconds.
const DEFAULT_TIMEOUT_MS: u64 = 3000;

/// Maximum number of retransmissions for a STUN request.
const MAX_RETRANSMISSIONS: u32 = 3;

/// A 12-byte STUN transaction ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionId([u8; TRANSACTION_ID_SIZE]);

impl TransactionId {
    /// Generates a new random transaction ID.
    #[must_use]
    pub fn random() -> Self {
        let mut id = [0u8; TRANSACTION_ID_SIZE];
        rand::thread_rng().fill(&mut id);
        Self(id)
    }

    /// Creates a transaction ID from raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; TRANSACTION_ID_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the raw bytes of the transaction ID.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; TRANSACTION_ID_SIZE] {
        &self.0
    }
}

/// A STUN message (request or response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunMessage {
    /// Message type (e.g., Binding Request = 0x0001, Binding Response = 0x0101).
    pub msg_type: u16,
    /// Transaction ID (12 bytes).
    pub transaction_id: TransactionId,
    /// Attributes contained in the message.
    pub attributes: Vec<StunAttribute>,
}

/// A STUN message attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StunAttribute {
    /// `XOR-MAPPED-ADDRESS` — the reflexive transport address, XOR-ed with the magic cookie.
    XorMappedAddress(SocketAddr),
    /// `MAPPED-ADDRESS` — the reflexive transport address (non-XOR, legacy).
    MappedAddress(SocketAddr),
    /// Unknown or unhandled attribute (type, raw value).
    Unknown(u16, Vec<u8>),
}

impl StunMessage {
    /// Creates a new STUN Binding Request with a random transaction ID.
    #[must_use]
    pub fn binding_request() -> Self {
        Self {
            msg_type: BINDING_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: Vec::new(),
        }
    }

    /// Creates a Binding Request with a specific transaction ID (for testing).
    #[must_use]
    pub fn binding_request_with_id(transaction_id: TransactionId) -> Self {
        Self {
            msg_type: BINDING_REQUEST,
            transaction_id,
            attributes: Vec::new(),
        }
    }

    /// Serializes this STUN message to bytes (RFC 5389 wire format).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let attrs_bytes = self.encode_attributes();
        let msg_len = attrs_bytes.len();

        let mut buf = Vec::with_capacity(HEADER_SIZE + msg_len);

        // Message Type (2 bytes)
        buf.extend_from_slice(&self.msg_type.to_be_bytes());
        // Message Length (2 bytes) — excludes the 20-byte header
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(msg_len as u16).to_be_bytes());
        // Magic Cookie (4 bytes)
        buf.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        // Transaction ID (12 bytes)
        buf.extend_from_slice(self.transaction_id.as_bytes());
        // Attributes
        buf.extend_from_slice(&attrs_bytes);

        buf
    }

    /// Parses a STUN message from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(P2pError::Stun(format!(
                "message too short: {} bytes (minimum {})",
                data.len(),
                HEADER_SIZE
            )));
        }

        let msg_type = u16::from_be_bytes([data[0], data[1]]);
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
        let cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        if cookie != MAGIC_COOKIE {
            return Err(P2pError::Stun(format!(
                "invalid magic cookie: 0x{cookie:08X} (expected 0x{MAGIC_COOKIE:08X})"
            )));
        }

        // The top two bits of the message type must be zero (RFC 5389 §6).
        if msg_type & 0xC000 != 0 {
            return Err(P2pError::Stun(format!(
                "invalid message type: 0x{msg_type:04X} (top two bits must be zero)"
            )));
        }

        let mut tid = [0u8; TRANSACTION_ID_SIZE];
        tid.copy_from_slice(&data[8..20]);
        let transaction_id = TransactionId::from_bytes(tid);

        if data.len() < HEADER_SIZE + msg_len {
            return Err(P2pError::Stun(format!(
                "message truncated: header says {} attribute bytes but only {} available",
                msg_len,
                data.len() - HEADER_SIZE
            )));
        }

        let attr_data = &data[HEADER_SIZE..HEADER_SIZE + msg_len];
        let attributes = Self::parse_attributes(attr_data, &transaction_id)?;

        Ok(Self {
            msg_type,
            transaction_id,
            attributes,
        })
    }

    /// Returns `true` if this is a Binding Response (success).
    #[must_use]
    pub fn is_binding_response(&self) -> bool {
        self.msg_type == BINDING_RESPONSE
    }

    /// Returns `true` if this is a Binding Error Response.
    #[must_use]
    pub fn is_binding_error(&self) -> bool {
        self.msg_type == BINDING_ERROR_RESPONSE
    }

    /// Extracts the server-reflexive address from the response.
    ///
    /// Prefers `XOR-MAPPED-ADDRESS` over `MAPPED-ADDRESS` per RFC 5389 recommendation.
    #[must_use]
    pub fn mapped_address(&self) -> Option<SocketAddr> {
        // First look for XOR-MAPPED-ADDRESS (preferred)
        for attr in &self.attributes {
            if let StunAttribute::XorMappedAddress(addr) = attr {
                return Some(*addr);
            }
        }
        // Fall back to MAPPED-ADDRESS
        for attr in &self.attributes {
            if let StunAttribute::MappedAddress(addr) = attr {
                return Some(*addr);
            }
        }
        None
    }

    fn encode_attributes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for attr in &self.attributes {
            match attr {
                StunAttribute::XorMappedAddress(addr) => {
                    let value = encode_xor_mapped_address(*addr, &self.transaction_id);
                    buf.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(&value);
                    // Pad to 4-byte boundary
                    let padding = (4 - value.len() % 4) % 4;
                    buf.extend(std::iter::repeat_n(0u8, padding));
                }
                StunAttribute::MappedAddress(addr) => {
                    let value = encode_mapped_address(*addr);
                    buf.extend_from_slice(&ATTR_MAPPED_ADDRESS.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(&value);
                    let padding = (4 - value.len() % 4) % 4;
                    buf.extend(std::iter::repeat_n(0u8, padding));
                }
                StunAttribute::Unknown(attr_type, value) => {
                    buf.extend_from_slice(&attr_type.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(value);
                    let padding = (4 - value.len() % 4) % 4;
                    buf.extend(std::iter::repeat_n(0u8, padding));
                }
            }
        }
        buf
    }

    fn parse_attributes(
        data: &[u8],
        transaction_id: &TransactionId,
    ) -> Result<Vec<StunAttribute>> {
        let mut attrs = Vec::new();
        let mut offset = 0;

        while offset + 4 <= data.len() {
            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;

            if offset + attr_len > data.len() {
                return Err(P2pError::Stun(format!(
                    "attribute 0x{attr_type:04X} truncated: needs {attr_len} bytes, {} available",
                    data.len() - offset
                )));
            }

            let value = &data[offset..offset + attr_len];

            let attr = match attr_type {
                ATTR_XOR_MAPPED_ADDRESS => {
                    let addr = parse_xor_mapped_address(value, transaction_id)?;
                    StunAttribute::XorMappedAddress(addr)
                }
                ATTR_MAPPED_ADDRESS => {
                    let addr = parse_mapped_address(value)?;
                    StunAttribute::MappedAddress(addr)
                }
                _ => StunAttribute::Unknown(attr_type, value.to_vec()),
            };

            attrs.push(attr);

            // Advance past value + padding to 4-byte boundary
            let padded_len = (attr_len + 3) & !3;
            offset += padded_len;
        }

        Ok(attrs)
    }
}

/// Parses an `XOR-MAPPED-ADDRESS` attribute value.
fn parse_xor_mapped_address(data: &[u8], tid: &TransactionId) -> Result<SocketAddr> {
    if data.len() < 4 {
        return Err(P2pError::Stun(
            "XOR-MAPPED-ADDRESS too short".into(),
        ));
    }

    // First byte is reserved (zero), second is family
    let family = data[1];
    let xport = u16::from_be_bytes([data[2], data[3]]);
    #[allow(clippy::cast_possible_truncation)]
    let port = xport ^ (MAGIC_COOKIE >> 16) as u16;

    match family {
        FAMILY_IPV4 => {
            if data.len() < 8 {
                return Err(P2pError::Stun(
                    "XOR-MAPPED-ADDRESS IPv4 too short".into(),
                ));
            }
            let xaddr = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            let addr = Ipv4Addr::from(xaddr ^ MAGIC_COOKIE);
            Ok(SocketAddr::new(IpAddr::V4(addr), port))
        }
        FAMILY_IPV6 => {
            if data.len() < 20 {
                return Err(P2pError::Stun(
                    "XOR-MAPPED-ADDRESS IPv6 too short".into(),
                ));
            }
            // XOR with magic cookie (4 bytes) + transaction ID (12 bytes) = 16 bytes
            let mut xor_key = [0u8; 16];
            xor_key[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            xor_key[4..].copy_from_slice(tid.as_bytes());

            let mut addr_bytes = [0u8; 16];
            for i in 0..16 {
                addr_bytes[i] = data[4 + i] ^ xor_key[i];
            }
            let addr = Ipv6Addr::from(addr_bytes);
            Ok(SocketAddr::new(IpAddr::V6(addr), port))
        }
        _ => Err(P2pError::Stun(format!(
            "unknown address family: 0x{family:02X}"
        ))),
    }
}

/// Parses a `MAPPED-ADDRESS` attribute value.
fn parse_mapped_address(data: &[u8]) -> Result<SocketAddr> {
    if data.len() < 4 {
        return Err(P2pError::Stun("MAPPED-ADDRESS too short".into()));
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]);

    match family {
        FAMILY_IPV4 => {
            if data.len() < 8 {
                return Err(P2pError::Stun(
                    "MAPPED-ADDRESS IPv4 too short".into(),
                ));
            }
            let addr = Ipv4Addr::new(data[4], data[5], data[6], data[7]);
            Ok(SocketAddr::new(IpAddr::V4(addr), port))
        }
        FAMILY_IPV6 => {
            if data.len() < 20 {
                return Err(P2pError::Stun(
                    "MAPPED-ADDRESS IPv6 too short".into(),
                ));
            }
            let mut addr_bytes = [0u8; 16];
            addr_bytes.copy_from_slice(&data[4..20]);
            let addr = Ipv6Addr::from(addr_bytes);
            Ok(SocketAddr::new(IpAddr::V6(addr), port))
        }
        _ => Err(P2pError::Stun(format!(
            "unknown address family: 0x{family:02X}"
        ))),
    }
}

/// Encodes an `XOR-MAPPED-ADDRESS` attribute value.
fn encode_xor_mapped_address(addr: SocketAddr, tid: &TransactionId) -> Vec<u8> {
    let mut buf = Vec::new();
    // Reserved byte
    buf.push(0x00);

    #[allow(clippy::cast_possible_truncation)]
    let xport = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;

    match addr.ip() {
        IpAddr::V4(ipv4) => {
            buf.push(FAMILY_IPV4);
            buf.extend_from_slice(&xport.to_be_bytes());
            let xaddr = u32::from(ipv4) ^ MAGIC_COOKIE;
            buf.extend_from_slice(&xaddr.to_be_bytes());
        }
        IpAddr::V6(ipv6) => {
            buf.push(FAMILY_IPV6);
            buf.extend_from_slice(&xport.to_be_bytes());
            let mut xor_key = [0u8; 16];
            xor_key[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            xor_key[4..].copy_from_slice(tid.as_bytes());
            let addr_bytes = ipv6.octets();
            for i in 0..16 {
                buf.push(addr_bytes[i] ^ xor_key[i]);
            }
        }
    }
    buf
}

/// Encodes a `MAPPED-ADDRESS` attribute value.
fn encode_mapped_address(addr: SocketAddr) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x00); // Reserved
    match addr.ip() {
        IpAddr::V4(ipv4) => {
            buf.push(FAMILY_IPV4);
            buf.extend_from_slice(&addr.port().to_be_bytes());
            buf.extend_from_slice(&ipv4.octets());
        }
        IpAddr::V6(ipv6) => {
            buf.push(FAMILY_IPV6);
            buf.extend_from_slice(&addr.port().to_be_bytes());
            buf.extend_from_slice(&ipv6.octets());
        }
    }
    buf
}

/// Sends a STUN Binding Request to the given server and returns the server-reflexive address.
///
/// Uses the provided UDP socket to send and receive. Retransmits up to
/// [`MAX_RETRANSMISSIONS`] times with exponential backoff.
pub async fn discover_reflexive_address(
    socket: &UdpSocket,
    server: SocketAddr,
) -> Result<SocketAddr> {
    let request = StunMessage::binding_request();
    let request_bytes = request.to_bytes();
    let expected_tid = request.transaction_id;

    let timeout_base = std::time::Duration::from_millis(DEFAULT_TIMEOUT_MS);
    let mut buf = [0u8; 1024];

    for attempt in 0..=MAX_RETRANSMISSIONS {
        socket.send_to(&request_bytes, server).await?;
        debug!(
            attempt,
            server = %server,
            "sent STUN Binding Request"
        );

        let timeout = timeout_base * 2u32.pow(attempt);
        match tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _src))) => {
                let response = StunMessage::from_bytes(&buf[..len])?;

                if response.transaction_id != expected_tid {
                    debug!("ignoring response with mismatched transaction ID");
                    continue;
                }

                if response.is_binding_error() {
                    return Err(P2pError::Stun("server returned Binding Error Response".into()));
                }

                if !response.is_binding_response() {
                    return Err(P2pError::Stun(format!(
                        "unexpected message type: 0x{:04X}",
                        response.msg_type
                    )));
                }

                return response.mapped_address().ok_or_else(|| {
                    P2pError::Stun(
                        "Binding Response contained no MAPPED-ADDRESS or XOR-MAPPED-ADDRESS".into(),
                    )
                });
            }
            Ok(Err(e)) => return Err(P2pError::Io(e)),
            Err(_) => {
                debug!(attempt, "STUN request timed out, retransmitting");
            }
        }
    }

    Err(P2pError::StunTimeout(server))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_tid() -> TransactionId {
        TransactionId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
    }

    #[test]
    fn binding_request_serialization() {
        let tid = fixed_tid();
        let msg = StunMessage::binding_request_with_id(tid);
        let bytes = msg.to_bytes();

        // Header should be exactly 20 bytes for a request with no attributes
        assert_eq!(bytes.len(), HEADER_SIZE);

        // Message type: Binding Request (0x0001)
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x01);

        // Message length: 0 (no attributes)
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0x00);

        // Magic cookie
        assert_eq!(&bytes[4..8], &MAGIC_COOKIE.to_be_bytes());

        // Transaction ID
        assert_eq!(&bytes[8..20], tid.as_bytes());
    }

    #[test]
    fn binding_request_roundtrip() {
        let tid = fixed_tid();
        let msg = StunMessage::binding_request_with_id(tid);
        let bytes = msg.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.msg_type, BINDING_REQUEST);
        assert_eq!(parsed.transaction_id, tid);
        assert!(parsed.attributes.is_empty());
    }

    #[test]
    fn parse_response_with_xor_mapped_address_ipv4() {
        let tid = fixed_tid();
        let addr: SocketAddr = "203.0.113.42:5060".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![StunAttribute::XorMappedAddress(addr)],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert!(parsed.is_binding_response());
        assert_eq!(parsed.mapped_address(), Some(addr));

        // Verify the attribute was parsed correctly
        assert_eq!(parsed.attributes.len(), 1);
        match &parsed.attributes[0] {
            StunAttribute::XorMappedAddress(parsed_addr) => {
                assert_eq!(*parsed_addr, addr);
            }
            other => panic!("expected XorMappedAddress, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_with_xor_mapped_address_ipv6() {
        let tid = fixed_tid();
        let addr: SocketAddr = "[2001:db8::1]:8080".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![StunAttribute::XorMappedAddress(addr)],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert!(parsed.is_binding_response());
        assert_eq!(parsed.mapped_address(), Some(addr));
    }

    #[test]
    fn parse_response_with_mapped_address_fallback() {
        let tid = fixed_tid();
        let addr: SocketAddr = "192.168.1.100:1234".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![StunAttribute::MappedAddress(addr)],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.mapped_address(), Some(addr));
    }

    #[test]
    fn xor_mapped_address_preferred_over_mapped() {
        let tid = fixed_tid();
        let xor_addr: SocketAddr = "1.2.3.4:5678".parse().unwrap();
        let mapped_addr: SocketAddr = "5.6.7.8:9012".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![
                StunAttribute::MappedAddress(mapped_addr),
                StunAttribute::XorMappedAddress(xor_addr),
            ],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        // Should prefer XOR-MAPPED-ADDRESS
        assert_eq!(parsed.mapped_address(), Some(xor_addr));
    }

    #[test]
    fn rejects_short_message() {
        let result = StunMessage::from_bytes(&[0; 10]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("too short"), "error: {err}");
    }

    #[test]
    fn rejects_bad_magic_cookie() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        // Valid message type
        bytes[0] = 0x01;
        bytes[1] = 0x01;
        // Bad magic cookie
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        bytes[6] = 0xFF;
        bytes[7] = 0xFF;

        let result = StunMessage::from_bytes(&bytes);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("magic cookie"), "error: {err}");
    }

    #[test]
    fn rejects_invalid_message_type_bits() {
        let mut msg = StunMessage::binding_request_with_id(fixed_tid());
        msg.msg_type = 0xC000; // Top two bits set — invalid
        let bytes = msg.to_bytes();
        let result = StunMessage::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn handles_unknown_attributes() {
        let tid = fixed_tid();
        let addr: SocketAddr = "10.0.0.1:3478".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![
                StunAttribute::Unknown(0x8028, vec![0xAB, 0xCD, 0xEF, 0x01]),
                StunAttribute::XorMappedAddress(addr),
            ],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.attributes.len(), 2);
        assert_eq!(parsed.mapped_address(), Some(addr));

        match &parsed.attributes[0] {
            StunAttribute::Unknown(t, v) => {
                assert_eq!(*t, 0x8028);
                assert_eq!(v, &[0xAB, 0xCD, 0xEF, 0x01]);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn binding_error_response_detected() {
        let msg = StunMessage {
            msg_type: BINDING_ERROR_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: Vec::new(),
        };
        let bytes = msg.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_binding_error());
        assert!(!parsed.is_binding_response());
    }

    #[test]
    fn transaction_id_random_is_unique() {
        let a = TransactionId::random();
        let b = TransactionId::random();
        assert_ne!(a, b);
    }

    #[test]
    fn no_mapped_address_returns_none() {
        let msg = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![StunAttribute::Unknown(0x9999, vec![1, 2, 3, 4])],
        };
        assert_eq!(msg.mapped_address(), None);
    }

    #[tokio::test]
    async fn stun_client_server_loopback() {
        // Simulate a STUN server on localhost
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();

        let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_addr = client_sock.local_addr().unwrap();

        // Spawn a mock STUN server that echoes back the client's address
        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = server_sock.recv_from(&mut buf).await.unwrap();

            // Parse the request
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();
            assert_eq!(request.msg_type, BINDING_REQUEST);

            // Build a response with the client's source address
            let response = StunMessage {
                msg_type: BINDING_RESPONSE,
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(src)],
            };

            server_sock
                .send_to(&response.to_bytes(), src)
                .await
                .unwrap();
        });

        let reflexive = discover_reflexive_address(&client_sock, server_addr)
            .await
            .unwrap();

        // The reflexive address should be the client's local address
        assert_eq!(reflexive, client_addr);

        server_handle.await.unwrap();
    }

    #[test]
    fn mapped_address_ipv6_roundtrip() {
        let tid = fixed_tid();
        let addr: SocketAddr = "[::1]:12345".parse().unwrap();

        let response = StunMessage {
            msg_type: BINDING_RESPONSE,
            transaction_id: tid,
            attributes: vec![StunAttribute::MappedAddress(addr)],
        };

        let bytes = response.to_bytes();
        let parsed = StunMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.mapped_address(), Some(addr));
    }

    #[test]
    fn truncated_attribute_rejected() {
        let tid = fixed_tid();
        let mut bytes = vec![];
        // Header
        bytes.extend_from_slice(&BINDING_RESPONSE.to_be_bytes());
        // Message length: claim 8 bytes of attributes
        bytes.extend_from_slice(&8u16.to_be_bytes());
        bytes.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        bytes.extend_from_slice(tid.as_bytes());
        // Attribute header claiming 100 bytes of value
        bytes.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
        bytes.extend_from_slice(&100u16.to_be_bytes());
        // But only provide 4 bytes
        bytes.extend_from_slice(&[0u8; 4]);

        let result = StunMessage::from_bytes(&bytes);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("truncated"), "error: {err}");
    }
}
