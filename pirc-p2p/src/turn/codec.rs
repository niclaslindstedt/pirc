//! TURN message encoding, decoding, and construction.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use hmac::{Hmac, Mac};
use sha1::Sha1;

use crate::error::{P2pError, Result};
use crate::stun::{StunMessage, TransactionId};

use super::types::{
    TurnAttribute, TurnCredentials, TurnMessage,
    ALLOCATE_ERROR_RESPONSE, ALLOCATE_REQUEST, ALLOCATE_RESPONSE,
    ATTR_CHANNEL_NUMBER, ATTR_DATA, ATTR_ERROR_CODE, ATTR_LIFETIME,
    ATTR_MESSAGE_INTEGRITY, ATTR_NONCE, ATTR_REALM, ATTR_REQUESTED_TRANSPORT,
    ATTR_USERNAME, ATTR_XOR_MAPPED_ADDRESS, ATTR_XOR_PEER_ADDRESS,
    ATTR_XOR_RELAYED_ADDRESS, CHANNEL_BIND_ERROR_RESPONSE, CHANNEL_BIND_REQUEST,
    CHANNEL_BIND_RESPONSE, CREATE_PERMISSION_ERROR_RESPONSE, CREATE_PERMISSION_REQUEST,
    CREATE_PERMISSION_RESPONSE, DATA_INDICATION, FAMILY_IPV4, FAMILY_IPV6,
    HEADER_SIZE, HMAC_SHA1_LEN, MAGIC_COOKIE, REFRESH_REQUEST, REFRESH_RESPONSE,
    SEND_INDICATION, TRANSPORT_UDP,
};

impl TurnMessage {
    /// Creates a TURN Allocate Request (unauthenticated, for initial 401 challenge).
    #[must_use]
    pub fn allocate_request() -> Self {
        Self {
            msg_type: ALLOCATE_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: vec![TurnAttribute::RequestedTransport(TRANSPORT_UDP)],
        }
    }

    /// Creates an authenticated TURN Allocate Request with long-term credentials.
    #[must_use]
    pub fn allocate_request_with_credentials(creds: &TurnCredentials) -> Self {
        Self {
            msg_type: ALLOCATE_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::RequestedTransport(TRANSPORT_UDP),
                TurnAttribute::Username(creds.username.clone()),
                TurnAttribute::Realm(creds.realm.clone()),
                TurnAttribute::Nonce(creds.nonce.clone()),
                // MESSAGE-INTEGRITY will be computed during serialization
            ],
        }
    }

    /// Creates a `CreatePermission` Request for a peer address.
    #[must_use]
    pub fn create_permission_request(peer: SocketAddr, creds: &TurnCredentials) -> Self {
        Self {
            msg_type: CREATE_PERMISSION_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::XorPeerAddress(peer),
                TurnAttribute::Username(creds.username.clone()),
                TurnAttribute::Realm(creds.realm.clone()),
                TurnAttribute::Nonce(creds.nonce.clone()),
            ],
        }
    }

    /// Creates a `ChannelBind` Request binding a channel number to a peer address.
    #[must_use]
    pub fn channel_bind_request(
        channel: u16,
        peer: SocketAddr,
        creds: &TurnCredentials,
    ) -> Self {
        Self {
            msg_type: CHANNEL_BIND_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::ChannelNumber(channel),
                TurnAttribute::XorPeerAddress(peer),
                TurnAttribute::Username(creds.username.clone()),
                TurnAttribute::Realm(creds.realm.clone()),
                TurnAttribute::Nonce(creds.nonce.clone()),
            ],
        }
    }

    /// Creates a Refresh Request to keep an allocation alive (or to deallocate with lifetime=0).
    #[must_use]
    pub fn refresh_request(lifetime: u32, creds: &TurnCredentials) -> Self {
        Self {
            msg_type: REFRESH_REQUEST,
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::Lifetime(lifetime),
                TurnAttribute::Username(creds.username.clone()),
                TurnAttribute::Realm(creds.realm.clone()),
                TurnAttribute::Nonce(creds.nonce.clone()),
            ],
        }
    }

    /// Creates a Send Indication to relay data to a peer through the TURN server.
    #[must_use]
    pub fn send_indication(peer: SocketAddr, data: Vec<u8>) -> Self {
        Self {
            msg_type: SEND_INDICATION,
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::XorPeerAddress(peer),
                TurnAttribute::Data(data),
            ],
        }
    }

    /// Serializes this message to bytes (STUN wire format).
    ///
    /// If `integrity_key` is provided, a `MESSAGE-INTEGRITY` attribute is appended
    /// using HMAC-SHA1 over the message contents.
    #[must_use]
    pub fn to_bytes(&self, integrity_key: Option<&[u8]>) -> Vec<u8> {
        let attrs_bytes = self.encode_attributes();

        // If we need MESSAGE-INTEGRITY, compute it over the message up to (but not
        // including) the MESSAGE-INTEGRITY attribute itself, with the message length
        // adjusted to include the MESSAGE-INTEGRITY TLV (4 + 20 = 24 bytes).
        let (attrs_bytes, mi_bytes) = if let Some(key) = integrity_key {
            let mi_tlv_len = 4 + HMAC_SHA1_LEN; // type(2) + length(2) + value(20)
            #[allow(clippy::cast_possible_truncation)]
            let adjusted_len = (attrs_bytes.len() + mi_tlv_len) as u16;

            // Build temporary header with adjusted length for HMAC computation
            let mut hmac_input = Vec::with_capacity(HEADER_SIZE + attrs_bytes.len());
            hmac_input.extend_from_slice(&self.msg_type.to_be_bytes());
            hmac_input.extend_from_slice(&adjusted_len.to_be_bytes());
            hmac_input.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
            hmac_input.extend_from_slice(self.transaction_id.as_bytes());
            hmac_input.extend_from_slice(&attrs_bytes);

            let hmac_value = compute_hmac_sha1(key, &hmac_input);

            // Build MESSAGE-INTEGRITY TLV
            let mut mi = Vec::with_capacity(mi_tlv_len);
            mi.extend_from_slice(&ATTR_MESSAGE_INTEGRITY.to_be_bytes());
            #[allow(clippy::cast_possible_truncation)]
            mi.extend_from_slice(&(HMAC_SHA1_LEN as u16).to_be_bytes());
            mi.extend_from_slice(&hmac_value);

            (attrs_bytes, Some(mi))
        } else {
            (attrs_bytes, None)
        };

        let total_attr_len = attrs_bytes.len() + mi_bytes.as_ref().map_or(0, Vec::len);
        let mut buf = Vec::with_capacity(HEADER_SIZE + total_attr_len);

        // Header
        buf.extend_from_slice(&self.msg_type.to_be_bytes());
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(total_attr_len as u16).to_be_bytes());
        buf.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        buf.extend_from_slice(self.transaction_id.as_bytes());

        // Attributes
        buf.extend_from_slice(&attrs_bytes);
        if let Some(mi) = mi_bytes {
            buf.extend_from_slice(&mi);
        }

        buf
    }

    /// Parses a TURN message from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        // Reuse StunMessage parsing for the header
        let stun_msg = StunMessage::from_bytes(data)?;

        // Re-parse attributes as TURN attributes
        let attr_data = &data[HEADER_SIZE..HEADER_SIZE + (data.len() - HEADER_SIZE).min(
            u16::from_be_bytes([data[2], data[3]]) as usize,
        )];
        let attributes = Self::parse_turn_attributes(attr_data, &stun_msg.transaction_id)?;

        Ok(Self {
            msg_type: stun_msg.msg_type,
            transaction_id: stun_msg.transaction_id,
            attributes,
        })
    }

    /// Returns `true` if this is an Allocate Success Response.
    #[must_use]
    pub fn is_allocate_response(&self) -> bool {
        self.msg_type == ALLOCATE_RESPONSE
    }

    /// Returns `true` if this is an Allocate Error Response.
    #[must_use]
    pub fn is_allocate_error(&self) -> bool {
        self.msg_type == ALLOCATE_ERROR_RESPONSE
    }

    /// Returns `true` if this is a `CreatePermission` Success Response.
    #[must_use]
    pub fn is_create_permission_response(&self) -> bool {
        self.msg_type == CREATE_PERMISSION_RESPONSE
    }

    /// Returns `true` if this is a `CreatePermission` Error Response.
    #[must_use]
    pub fn is_create_permission_error(&self) -> bool {
        self.msg_type == CREATE_PERMISSION_ERROR_RESPONSE
    }

    /// Returns `true` if this is a `ChannelBind` Success Response.
    #[must_use]
    pub fn is_channel_bind_response(&self) -> bool {
        self.msg_type == CHANNEL_BIND_RESPONSE
    }

    /// Returns `true` if this is a `ChannelBind` Error Response.
    #[must_use]
    pub fn is_channel_bind_error(&self) -> bool {
        self.msg_type == CHANNEL_BIND_ERROR_RESPONSE
    }

    /// Returns `true` if this is a Refresh Success Response.
    #[must_use]
    pub fn is_refresh_response(&self) -> bool {
        self.msg_type == REFRESH_RESPONSE
    }

    /// Returns `true` if this is a Data Indication.
    #[must_use]
    pub fn is_data_indication(&self) -> bool {
        self.msg_type == DATA_INDICATION
    }

    /// Returns `true` if this is a Send Indication.
    #[must_use]
    pub fn is_send_indication(&self) -> bool {
        self.msg_type == SEND_INDICATION
    }

    /// Extracts the `XOR-RELAYED-ADDRESS` from the message.
    #[must_use]
    pub fn relayed_address(&self) -> Option<SocketAddr> {
        for attr in &self.attributes {
            if let TurnAttribute::XorRelayedAddress(addr) = attr {
                return Some(*addr);
            }
        }
        None
    }

    /// Extracts the `XOR-MAPPED-ADDRESS` from the message.
    #[must_use]
    pub fn mapped_address(&self) -> Option<SocketAddr> {
        for attr in &self.attributes {
            if let TurnAttribute::XorMappedAddress(addr) = attr {
                return Some(*addr);
            }
        }
        None
    }

    /// Extracts the `LIFETIME` from the message.
    #[must_use]
    pub fn lifetime(&self) -> Option<u32> {
        for attr in &self.attributes {
            if let TurnAttribute::Lifetime(lt) = attr {
                return Some(*lt);
            }
        }
        None
    }

    /// Extracts the `XOR-PEER-ADDRESS` from the message.
    #[must_use]
    pub fn peer_address(&self) -> Option<SocketAddr> {
        for attr in &self.attributes {
            if let TurnAttribute::XorPeerAddress(addr) = attr {
                return Some(*addr);
            }
        }
        None
    }

    /// Extracts `DATA` from the message.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        for attr in &self.attributes {
            if let TurnAttribute::Data(d) = attr {
                return Some(d);
            }
        }
        None
    }

    /// Extracts the `ERROR-CODE` from the message.
    #[must_use]
    pub fn error_code(&self) -> Option<(u16, &str)> {
        for attr in &self.attributes {
            if let TurnAttribute::ErrorCode(code, reason) = attr {
                return Some((*code, reason));
            }
        }
        None
    }

    /// Extracts realm from the message (typically from a 401 error response).
    #[must_use]
    pub fn realm(&self) -> Option<&str> {
        for attr in &self.attributes {
            if let TurnAttribute::Realm(r) = attr {
                return Some(r);
            }
        }
        None
    }

    /// Extracts nonce from the message (typically from a 401 error response).
    #[must_use]
    pub fn nonce(&self) -> Option<&str> {
        for attr in &self.attributes {
            if let TurnAttribute::Nonce(n) = attr {
                return Some(n);
            }
        }
        None
    }

    /// Extracts the `CHANNEL-NUMBER` from the message.
    #[must_use]
    pub fn channel_number(&self) -> Option<u16> {
        for attr in &self.attributes {
            if let TurnAttribute::ChannelNumber(ch) = attr {
                return Some(*ch);
            }
        }
        None
    }

    pub(crate) fn encode_attributes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for attr in &self.attributes {
            match attr {
                TurnAttribute::Username(u) => {
                    encode_string_attr(&mut buf, ATTR_USERNAME, u);
                }
                TurnAttribute::Realm(r) => {
                    encode_string_attr(&mut buf, ATTR_REALM, r);
                }
                TurnAttribute::Nonce(n) => {
                    encode_string_attr(&mut buf, ATTR_NONCE, n);
                }
                TurnAttribute::MessageIntegrity(hmac_val) => {
                    buf.extend_from_slice(&ATTR_MESSAGE_INTEGRITY.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(HMAC_SHA1_LEN as u16).to_be_bytes());
                    buf.extend_from_slice(hmac_val);
                    // HMAC-SHA1 is exactly 20 bytes, already 4-byte aligned
                }
                TurnAttribute::Lifetime(lt) => {
                    buf.extend_from_slice(&ATTR_LIFETIME.to_be_bytes());
                    buf.extend_from_slice(&4u16.to_be_bytes());
                    buf.extend_from_slice(&lt.to_be_bytes());
                }
                TurnAttribute::XorRelayedAddress(addr) => {
                    let value = encode_xor_address(*addr, &self.transaction_id);
                    buf.extend_from_slice(&ATTR_XOR_RELAYED_ADDRESS.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(&value);
                    pad_to_4(&mut buf, value.len());
                }
                TurnAttribute::XorMappedAddress(addr) => {
                    let value = encode_xor_address(*addr, &self.transaction_id);
                    buf.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(&value);
                    pad_to_4(&mut buf, value.len());
                }
                TurnAttribute::XorPeerAddress(addr) => {
                    let value = encode_xor_address(*addr, &self.transaction_id);
                    buf.extend_from_slice(&ATTR_XOR_PEER_ADDRESS.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(&value);
                    pad_to_4(&mut buf, value.len());
                }
                TurnAttribute::RequestedTransport(proto) => {
                    buf.extend_from_slice(&ATTR_REQUESTED_TRANSPORT.to_be_bytes());
                    buf.extend_from_slice(&4u16.to_be_bytes());
                    buf.push(*proto);
                    buf.extend_from_slice(&[0, 0, 0]); // RFFU (3 reserved bytes)
                }
                TurnAttribute::ChannelNumber(ch) => {
                    buf.extend_from_slice(&ATTR_CHANNEL_NUMBER.to_be_bytes());
                    buf.extend_from_slice(&4u16.to_be_bytes());
                    buf.extend_from_slice(&ch.to_be_bytes());
                    buf.extend_from_slice(&[0, 0]); // RFFU (2 reserved bytes)
                }
                TurnAttribute::Data(data) => {
                    buf.extend_from_slice(&ATTR_DATA.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(data.len() as u16).to_be_bytes());
                    buf.extend_from_slice(data);
                    pad_to_4(&mut buf, data.len());
                }
                TurnAttribute::ErrorCode(code, reason) => {
                    let class = *code / 100;
                    let number = *code % 100;
                    let reason_bytes = reason.as_bytes();
                    #[allow(clippy::cast_possible_truncation)]
                    let attr_len = (4 + reason_bytes.len()) as u16;
                    buf.extend_from_slice(&ATTR_ERROR_CODE.to_be_bytes());
                    buf.extend_from_slice(&attr_len.to_be_bytes());
                    buf.extend_from_slice(&[0, 0]); // Reserved
                    #[allow(clippy::cast_possible_truncation)]
                    buf.push(class as u8);
                    #[allow(clippy::cast_possible_truncation)]
                    buf.push(number as u8);
                    buf.extend_from_slice(reason_bytes);
                    pad_to_4(&mut buf, 4 + reason_bytes.len());
                }
                TurnAttribute::Unknown(attr_type, value) => {
                    buf.extend_from_slice(&attr_type.to_be_bytes());
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
                    buf.extend_from_slice(value);
                    pad_to_4(&mut buf, value.len());
                }
            }
        }
        buf
    }

    pub(crate) fn parse_turn_attributes(
        data: &[u8],
        transaction_id: &TransactionId,
    ) -> Result<Vec<TurnAttribute>> {
        let mut attrs = Vec::new();
        let mut offset = 0;

        while offset + 4 <= data.len() {
            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;

            if offset + attr_len > data.len() {
                return Err(P2pError::Turn(format!(
                    "attribute 0x{attr_type:04X} truncated: needs {attr_len} bytes, {} available",
                    data.len() - offset
                )));
            }

            let value = &data[offset..offset + attr_len];

            let attr = match attr_type {
                ATTR_USERNAME => {
                    TurnAttribute::Username(String::from_utf8_lossy(value).into_owned())
                }
                ATTR_REALM => {
                    TurnAttribute::Realm(String::from_utf8_lossy(value).into_owned())
                }
                ATTR_NONCE => {
                    TurnAttribute::Nonce(String::from_utf8_lossy(value).into_owned())
                }
                ATTR_MESSAGE_INTEGRITY => {
                    if value.len() != HMAC_SHA1_LEN {
                        return Err(P2pError::Turn(format!(
                            "MESSAGE-INTEGRITY wrong size: {} (expected {HMAC_SHA1_LEN})",
                            value.len()
                        )));
                    }
                    let mut hmac_val = [0u8; HMAC_SHA1_LEN];
                    hmac_val.copy_from_slice(value);
                    TurnAttribute::MessageIntegrity(hmac_val)
                }
                ATTR_LIFETIME => {
                    if value.len() < 4 {
                        return Err(P2pError::Turn("LIFETIME too short".into()));
                    }
                    let lt = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
                    TurnAttribute::Lifetime(lt)
                }
                ATTR_XOR_RELAYED_ADDRESS => {
                    let addr = parse_xor_address(value, transaction_id)?;
                    TurnAttribute::XorRelayedAddress(addr)
                }
                ATTR_XOR_MAPPED_ADDRESS => {
                    let addr = parse_xor_address(value, transaction_id)?;
                    TurnAttribute::XorMappedAddress(addr)
                }
                ATTR_XOR_PEER_ADDRESS => {
                    let addr = parse_xor_address(value, transaction_id)?;
                    TurnAttribute::XorPeerAddress(addr)
                }
                ATTR_REQUESTED_TRANSPORT => {
                    if value.is_empty() {
                        return Err(P2pError::Turn("REQUESTED-TRANSPORT empty".into()));
                    }
                    TurnAttribute::RequestedTransport(value[0])
                }
                ATTR_CHANNEL_NUMBER => {
                    if value.len() < 4 {
                        return Err(P2pError::Turn("CHANNEL-NUMBER too short".into()));
                    }
                    let ch = u16::from_be_bytes([value[0], value[1]]);
                    TurnAttribute::ChannelNumber(ch)
                }
                ATTR_DATA => TurnAttribute::Data(value.to_vec()),
                ATTR_ERROR_CODE => {
                    if value.len() < 4 {
                        return Err(P2pError::Turn("ERROR-CODE too short".into()));
                    }
                    let class = u16::from(value[2]);
                    let number = u16::from(value[3]);
                    let code = class * 100 + number;
                    let reason = if value.len() > 4 {
                        String::from_utf8_lossy(&value[4..]).into_owned()
                    } else {
                        String::new()
                    };
                    TurnAttribute::ErrorCode(code, reason)
                }
                _ => TurnAttribute::Unknown(attr_type, value.to_vec()),
            };

            attrs.push(attr);

            // Advance past value + padding to 4-byte boundary
            let padded_len = (attr_len + 3) & !3;
            offset += padded_len;
        }

        Ok(attrs)
    }
}

// --- XOR address encoding/decoding (shared with STUN) ---

/// Encodes an XOR-ed address (used for `XOR-RELAYED-ADDRESS`, `XOR-PEER-ADDRESS`,
/// and `XOR-MAPPED-ADDRESS`).
fn encode_xor_address(addr: SocketAddr, tid: &TransactionId) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x00); // Reserved

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

/// Parses an XOR-ed address attribute value.
fn parse_xor_address(data: &[u8], tid: &TransactionId) -> Result<SocketAddr> {
    if data.len() < 4 {
        return Err(P2pError::Turn("XOR address too short".into()));
    }

    let family = data[1];
    let xport = u16::from_be_bytes([data[2], data[3]]);
    #[allow(clippy::cast_possible_truncation)]
    let port = xport ^ (MAGIC_COOKIE >> 16) as u16;

    match family {
        FAMILY_IPV4 => {
            if data.len() < 8 {
                return Err(P2pError::Turn("XOR address IPv4 too short".into()));
            }
            let xaddr = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            let addr = Ipv4Addr::from(xaddr ^ MAGIC_COOKIE);
            Ok(SocketAddr::new(IpAddr::V4(addr), port))
        }
        FAMILY_IPV6 => {
            if data.len() < 20 {
                return Err(P2pError::Turn("XOR address IPv6 too short".into()));
            }
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
        _ => Err(P2pError::Turn(format!(
            "unknown address family: 0x{family:02X}"
        ))),
    }
}

/// Encodes a string attribute with TLV header and padding.
fn encode_string_attr(buf: &mut Vec<u8>, attr_type: u16, value: &str) {
    let bytes = value.as_bytes();
    buf.extend_from_slice(&attr_type.to_be_bytes());
    #[allow(clippy::cast_possible_truncation)]
    buf.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(bytes);
    pad_to_4(buf, bytes.len());
}

/// Pads buffer to the next 4-byte boundary based on unpadded length.
fn pad_to_4(buf: &mut Vec<u8>, unpadded_len: usize) {
    let padding = (4 - unpadded_len % 4) % 4;
    buf.extend(std::iter::repeat_n(0u8, padding));
}

// --- Long-term credential mechanism (RFC 5389 §10.2.2) ---

/// Computes the long-term credential key: `MD5(username ":" realm ":" password)`.
#[must_use]
pub fn compute_long_term_key(username: &str, realm: &str, password: &str) -> Vec<u8> {
    let input = format!("{username}:{realm}:{password}");
    let digest = md5::compute(input.as_bytes());
    digest.0.to_vec()
}

/// Computes HMAC-SHA1 over data using the given key.
pub(crate) fn compute_hmac_sha1(key: &[u8], data: &[u8]) -> [u8; HMAC_SHA1_LEN] {
    let mut mac =
        Hmac::<Sha1>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    let result = mac.finalize();
    let mut output = [0u8; HMAC_SHA1_LEN];
    output.copy_from_slice(&result.into_bytes());
    output
}
