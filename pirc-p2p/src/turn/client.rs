//! TURN client functions for relay allocation and data relay.

use std::net::SocketAddr;

use tokio::net::UdpSocket;
use tracing::debug;

use crate::error::{P2pError, Result};

use super::codec::compute_long_term_key;
use super::types::{
    Allocation, TurnCredentials, TurnMessage,
    DEFAULT_TIMEOUT_MS, MAX_RETRANSMISSIONS,
};

/// Sends a TURN request and waits for the matching response.
///
/// Retransmits with exponential backoff per RFC 5766.
async fn send_turn_request(
    socket: &UdpSocket,
    server: SocketAddr,
    msg: &TurnMessage,
    integrity_key: Option<&[u8]>,
) -> Result<TurnMessage> {
    let request_bytes = msg.to_bytes(integrity_key);
    let expected_tid = msg.transaction_id;
    let timeout_base = std::time::Duration::from_millis(DEFAULT_TIMEOUT_MS);
    let mut buf = [0u8; 4096];

    for attempt in 0..=MAX_RETRANSMISSIONS {
        socket.send_to(&request_bytes, server).await?;
        debug!(
            attempt,
            server = %server,
            msg_type = format!("0x{:04X}", msg.msg_type),
            "sent TURN request"
        );

        let timeout = timeout_base * 2u32.pow(attempt);
        match tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _src))) => {
                let response = TurnMessage::from_bytes(&buf[..len])?;

                if response.transaction_id != expected_tid {
                    debug!("ignoring response with mismatched transaction ID");
                    continue;
                }

                return Ok(response);
            }
            Ok(Err(e)) => return Err(P2pError::Io(e)),
            Err(_) => {
                debug!(attempt, "TURN request timed out, retransmitting");
            }
        }
    }

    Err(P2pError::TurnTimeout(server))
}

/// Performs a TURN Allocate with the long-term credential auth flow.
///
/// 1. Sends an unauthenticated Allocate Request
/// 2. Receives a 401 Unauthorized with realm + nonce
/// 3. Re-sends an authenticated Allocate Request with credentials
/// 4. Returns the allocation details
pub async fn allocate(
    socket: &UdpSocket,
    server: SocketAddr,
    username: &str,
    password: &str,
) -> Result<Allocation> {
    // Step 1: Send unauthenticated request to get realm/nonce
    let initial = TurnMessage::allocate_request();
    let response = send_turn_request(socket, server, &initial, None).await?;

    if !response.is_allocate_error() {
        return Err(P2pError::Turn(
            "expected 401 Unauthorized from initial allocate, got success".into(),
        ));
    }

    let error_code = response.error_code().map_or(0, |(c, _)| c);
    if error_code != 401 {
        let reason = response
            .error_code()
            .map(|(_, r)| r.to_string())
            .unwrap_or_default();
        return Err(P2pError::Turn(format!(
            "unexpected error {error_code}: {reason}"
        )));
    }

    let realm = response
        .realm()
        .ok_or_else(|| P2pError::Turn("401 response missing REALM".into()))?
        .to_string();
    let nonce = response
        .nonce()
        .ok_or_else(|| P2pError::Turn("401 response missing NONCE".into()))?
        .to_string();

    // Step 2: Send authenticated request
    let creds = TurnCredentials {
        username: username.to_string(),
        password: password.to_string(),
        realm: realm.clone(),
        nonce: nonce.clone(),
    };

    let key = compute_long_term_key(username, &realm, password);
    let auth_request = TurnMessage::allocate_request_with_credentials(&creds);
    let auth_response =
        send_turn_request(socket, server, &auth_request, Some(&key)).await?;

    if auth_response.is_allocate_error() {
        let (code, reason) = auth_response
            .error_code()
            .unwrap_or((0, "unknown error"));
        return Err(P2pError::Turn(format!(
            "allocate failed with error {code}: {reason}"
        )));
    }

    if !auth_response.is_allocate_response() {
        return Err(P2pError::Turn(format!(
            "unexpected message type: 0x{:04X}",
            auth_response.msg_type
        )));
    }

    let relay_addr = auth_response
        .relayed_address()
        .ok_or_else(|| P2pError::Turn("Allocate response missing XOR-RELAYED-ADDRESS".into()))?;

    let mapped_addr = auth_response.mapped_address();
    let lifetime = auth_response.lifetime().unwrap_or(600);

    Ok(Allocation {
        relay_addr,
        mapped_addr,
        lifetime,
    })
}

/// Creates a permission on the TURN server to allow traffic from a peer.
pub async fn create_permission(
    socket: &UdpSocket,
    server: SocketAddr,
    peer: SocketAddr,
    creds: &TurnCredentials,
) -> Result<()> {
    let key = compute_long_term_key(&creds.username, &creds.realm, &creds.password);
    let request = TurnMessage::create_permission_request(peer, creds);
    let response = send_turn_request(socket, server, &request, Some(&key)).await?;

    if response.is_create_permission_error() {
        let (code, reason) = response.error_code().unwrap_or((0, "unknown error"));
        return Err(P2pError::Turn(format!(
            "CreatePermission failed with error {code}: {reason}"
        )));
    }

    if !response.is_create_permission_response() {
        return Err(P2pError::Turn(format!(
            "unexpected message type: 0x{:04X}",
            response.msg_type
        )));
    }

    Ok(())
}

/// Binds a channel number to a peer address for efficient data relay.
///
/// Channel numbers must be in the range 0x4000–0x7FFF.
pub async fn channel_bind(
    socket: &UdpSocket,
    server: SocketAddr,
    channel: u16,
    peer: SocketAddr,
    creds: &TurnCredentials,
) -> Result<()> {
    if !(0x4000..=0x7FFF).contains(&channel) {
        return Err(P2pError::Turn(format!(
            "invalid channel number 0x{channel:04X}: must be in range 0x4000-0x7FFF"
        )));
    }

    let key = compute_long_term_key(&creds.username, &creds.realm, &creds.password);
    let request = TurnMessage::channel_bind_request(channel, peer, creds);
    let response = send_turn_request(socket, server, &request, Some(&key)).await?;

    if response.is_channel_bind_error() {
        let (code, reason) = response.error_code().unwrap_or((0, "unknown error"));
        return Err(P2pError::Turn(format!(
            "ChannelBind failed with error {code}: {reason}"
        )));
    }

    if !response.is_channel_bind_response() {
        return Err(P2pError::Turn(format!(
            "unexpected message type: 0x{:04X}",
            response.msg_type
        )));
    }

    Ok(())
}

/// Sends data to a peer through the TURN relay using a Send Indication.
///
/// This is used before a channel is bound. After channel binding, use
/// `ChannelData` messages instead for efficiency.
pub async fn send_to_peer(
    socket: &UdpSocket,
    server: SocketAddr,
    peer: SocketAddr,
    data: Vec<u8>,
) -> Result<()> {
    let indication = TurnMessage::send_indication(peer, data);
    // Indications are fire-and-forget (no response expected)
    let bytes = indication.to_bytes(None);
    socket.send_to(&bytes, server).await?;
    Ok(())
}

/// Parses a received Data Indication to extract the peer address and relayed data.
pub fn parse_data_indication(data: &[u8]) -> Result<(SocketAddr, Vec<u8>)> {
    let msg = TurnMessage::from_bytes(data)?;

    if !msg.is_data_indication() {
        return Err(P2pError::Turn(format!(
            "not a Data Indication: 0x{:04X}",
            msg.msg_type
        )));
    }

    let peer = msg
        .peer_address()
        .ok_or_else(|| P2pError::Turn("Data Indication missing XOR-PEER-ADDRESS".into()))?;

    let payload = msg
        .data()
        .ok_or_else(|| P2pError::Turn("Data Indication missing DATA attribute".into()))?
        .to_vec();

    Ok((peer, payload))
}

/// Encodes a `ChannelData` message for efficient relay after channel binding.
///
/// Format: channel number (2 bytes) + length (2 bytes) + data + padding.
#[must_use]
pub fn encode_channel_data(channel: u16, data: &[u8]) -> Vec<u8> {
    #[allow(clippy::cast_possible_truncation)]
    let len = data.len() as u16;
    let mut buf = Vec::with_capacity(4 + data.len() + 3);
    buf.extend_from_slice(&channel.to_be_bytes());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(data);
    // Pad to 4-byte boundary
    let padding = (4 - data.len() % 4) % 4;
    buf.extend(std::iter::repeat_n(0u8, padding));
    buf
}

/// Decodes a `ChannelData` message, returning the channel number and data.
pub fn decode_channel_data(data: &[u8]) -> Result<(u16, Vec<u8>)> {
    if data.len() < 4 {
        return Err(P2pError::Turn("ChannelData too short".into()));
    }

    let channel = u16::from_be_bytes([data[0], data[1]]);
    let len = u16::from_be_bytes([data[2], data[3]]) as usize;

    if data.len() < 4 + len {
        return Err(P2pError::Turn(format!(
            "ChannelData truncated: header says {len} bytes, {} available",
            data.len() - 4
        )));
    }

    Ok((channel, data[4..4 + len].to_vec()))
}
