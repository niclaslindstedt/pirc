//! Framed UDP transport for P2P data exchange.
//!
//! Provides [`UdpTransport`] for direct peer-to-peer communication over a
//! connected UDP socket, [`TurnRelayTransport`] for relaying data through a
//! TURN server when direct connectivity fails, and [`P2pTransport`] which
//! unifies both behind a common interface.
//!
//! All transports use a simple 2-byte big-endian length prefix framing so that
//! message boundaries are preserved over UDP.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::error::{P2pError, Result};
use crate::stun::StunMessage;
use crate::turn;

/// Maximum payload size for a single framed UDP message.
///
/// UDP datagrams should stay below typical path MTU to avoid fragmentation.
/// With a 2-byte length header this keeps total datagram size under ~1200 bytes,
/// which is safe for most network paths (including tunnels and VPNs).
pub const MAX_PAYLOAD_SIZE: usize = 1200;

/// Length-prefix header size (2 bytes, big-endian).
const FRAME_HEADER_SIZE: usize = 2;

/// Default keep-alive interval in seconds.
const DEFAULT_KEEPALIVE_SECS: u64 = 15;

// ---------------------------------------------------------------------------
// Framing helpers
// ---------------------------------------------------------------------------

/// Encodes a payload with a 2-byte big-endian length prefix.
fn encode_frame(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.is_empty() {
        return Err(P2pError::Ice("cannot send empty payload".into()));
    }
    if payload.len() > MAX_PAYLOAD_SIZE {
        return Err(P2pError::Ice(format!(
            "payload too large: {} bytes (max {MAX_PAYLOAD_SIZE})",
            payload.len()
        )));
    }
    #[allow(clippy::cast_possible_truncation)]
    let len = payload.len() as u16;
    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes a framed message, returning the payload without the length prefix.
fn decode_frame(data: &[u8]) -> Result<&[u8]> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err(P2pError::Ice(format!(
            "frame too short: {} bytes (minimum {FRAME_HEADER_SIZE})",
            data.len()
        )));
    }
    let len = u16::from_be_bytes([data[0], data[1]]) as usize;
    if data.len() < FRAME_HEADER_SIZE + len {
        return Err(P2pError::Ice(format!(
            "frame truncated: header says {len} bytes, {} available",
            data.len() - FRAME_HEADER_SIZE
        )));
    }
    if len == 0 {
        return Err(P2pError::Ice("frame with zero-length payload".into()));
    }
    Ok(&data[FRAME_HEADER_SIZE..FRAME_HEADER_SIZE + len])
}

// ---------------------------------------------------------------------------
// UdpTransport — direct peer-to-peer
// ---------------------------------------------------------------------------

/// Framed UDP transport over a connected socket for direct P2P communication.
///
/// The socket must be *connected* (via [`UdpSocket::connect`]) to the peer
/// address so that `send` / `recv` operate without specifying the remote
/// address on every call.
pub struct UdpTransport {
    socket: Arc<UdpSocket>,
    keepalive_handle: Option<JoinHandle<()>>,
    keepalive_stop: Arc<Notify>,
}

impl UdpTransport {
    /// Creates a new [`UdpTransport`] wrapping a connected UDP socket.
    ///
    /// The caller is responsible for connecting the socket to the peer address
    /// before constructing this transport.
    #[must_use]
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        Self {
            socket,
            keepalive_handle: None,
            keepalive_stop: Arc::new(Notify::new()),
        }
    }

    /// Returns a reference to the underlying UDP socket.
    #[must_use]
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    /// Sends a framed message to the connected peer.
    pub async fn send(&self, payload: &[u8]) -> Result<()> {
        let frame = encode_frame(payload)?;
        self.socket.send(&frame).await?;
        Ok(())
    }

    /// Receives a framed message from the connected peer.
    ///
    /// Returns the payload without the length prefix.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; FRAME_HEADER_SIZE + MAX_PAYLOAD_SIZE];
        let n = self.socket.recv(&mut buf).await?;
        let payload = decode_frame(&buf[..n])?;
        Ok(payload.to_vec())
    }

    /// Starts a periodic STUN Binding Request keep-alive to maintain the NAT
    /// pinhole.
    ///
    /// Sends binding requests to the connected peer to keep the NAT mapping
    /// alive. The peer simply ignores unsolicited binding requests, but the
    /// outbound traffic keeps the pinhole open. Interval defaults to 15 s if
    /// `None`.
    pub fn start_keepalive(&mut self, interval_secs: Option<u64>) {
        // Stop any existing keep-alive first.
        self.stop_keepalive();

        let socket = Arc::clone(&self.socket);
        let stop = Arc::clone(&self.keepalive_stop);
        let interval = std::time::Duration::from_secs(
            interval_secs.unwrap_or(DEFAULT_KEEPALIVE_SECS),
        );

        self.keepalive_handle = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = stop.notified() => {
                        debug!("keep-alive stopped");
                        return;
                    }
                    () = tokio::time::sleep(interval) => {
                        let request = StunMessage::binding_request();
                        let bytes = request.to_bytes();
                        // Uses `send` (not `send_to`) since the socket is connected.
                        if let Err(e) = socket.send(&bytes).await {
                            warn!(error = %e, "keep-alive STUN binding request failed");
                        } else {
                            debug!("sent keep-alive STUN binding request");
                        }
                    }
                }
            }
        }));
    }

    /// Stops the keep-alive task if running.
    pub fn stop_keepalive(&mut self) {
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
            // Replace the Notify to clear any stored permit, ensuring the
            // next `start_keepalive()` does not wake immediately.
            self.keepalive_stop = Arc::new(Notify::new());
        }
    }
}

impl Drop for UdpTransport {
    fn drop(&mut self) {
        self.stop_keepalive();
    }
}

// ---------------------------------------------------------------------------
// TurnRelayTransport — relayed via TURN server
// ---------------------------------------------------------------------------

/// Framed transport that relays data through a TURN server.
///
/// Uses TURN Send Indications (and Data Indications for receiving) to relay
/// framed application data when direct UDP connectivity is not available.
pub struct TurnRelayTransport {
    /// The socket connected to the TURN server.
    socket: Arc<UdpSocket>,
    /// TURN server address.
    server: SocketAddr,
    /// Remote peer address (as seen by the TURN server).
    peer: SocketAddr,
    keepalive_handle: Option<JoinHandle<()>>,
    keepalive_stop: Arc<Notify>,
}

impl TurnRelayTransport {
    /// Creates a new TURN relay transport.
    ///
    /// - `socket` — UDP socket used to communicate with the TURN server.
    /// - `server` — address of the TURN server.
    /// - `peer` — the peer's address as known to the TURN server (its relay or
    ///   server-reflexive address).
    #[must_use]
    pub fn new(socket: Arc<UdpSocket>, server: SocketAddr, peer: SocketAddr) -> Self {
        Self {
            socket,
            server,
            peer,
            keepalive_handle: None,
            keepalive_stop: Arc::new(Notify::new()),
        }
    }

    /// Returns a reference to the underlying UDP socket.
    #[must_use]
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    /// Returns the TURN server address.
    #[must_use]
    pub fn server_addr(&self) -> SocketAddr {
        self.server
    }

    /// Returns the remote peer address.
    #[must_use]
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }

    /// Sends a framed message to the peer via TURN Send Indication.
    pub async fn send(&self, payload: &[u8]) -> Result<()> {
        let frame = encode_frame(payload)?;
        turn::send_to_peer(&self.socket, self.server, self.peer, frame).await
    }

    /// Receives a framed message relayed from the peer via TURN Data Indication.
    ///
    /// Returns the payload without the length prefix.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 4096];
        loop {
            let n = self.socket.recv(&mut buf).await?;
            // Try to parse as a Data Indication.
            match turn::parse_data_indication(&buf[..n]) {
                Ok((_peer_addr, data)) => {
                    let payload = decode_frame(&data)?;
                    return Ok(payload.to_vec());
                }
                Err(_) => {
                    // Not a data indication — could be a STUN/TURN
                    // response; ignore and keep listening.
                    debug!("ignoring non-Data-Indication packet on relay transport");
                }
            }
        }
    }

    /// Starts a periodic STUN Binding Request keep-alive to the TURN server.
    ///
    /// This keeps the TURN allocation alive and the NAT pinhole open.
    pub fn start_keepalive(&mut self, interval_secs: Option<u64>) {
        self.stop_keepalive();

        let socket = Arc::clone(&self.socket);
        let server = self.server;
        let stop = Arc::clone(&self.keepalive_stop);
        let interval = std::time::Duration::from_secs(
            interval_secs.unwrap_or(DEFAULT_KEEPALIVE_SECS),
        );

        self.keepalive_handle = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = stop.notified() => {
                        debug!("relay keep-alive stopped");
                        return;
                    }
                    () = tokio::time::sleep(interval) => {
                        let request = StunMessage::binding_request();
                        let bytes = request.to_bytes();
                        if let Err(e) = socket.send_to(&bytes, server).await {
                            warn!(error = %e, "relay keep-alive STUN binding request failed");
                        } else {
                            debug!(server = %server, "sent relay keep-alive STUN binding request");
                        }
                    }
                }
            }
        }));
    }

    /// Stops the keep-alive task if running.
    pub fn stop_keepalive(&mut self) {
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
            // Replace the Notify to clear any stored permit, ensuring the
            // next `start_keepalive()` does not wake immediately.
            self.keepalive_stop = Arc::new(Notify::new());
        }
    }
}

impl Drop for TurnRelayTransport {
    fn drop(&mut self) {
        self.stop_keepalive();
    }
}

// ---------------------------------------------------------------------------
// P2pTransport — unified enum
// ---------------------------------------------------------------------------

/// Unified P2P transport abstracting over direct UDP and TURN relay paths.
pub enum P2pTransport {
    /// Direct peer-to-peer UDP connection.
    Direct(UdpTransport),
    /// Relayed connection through a TURN server.
    Relayed(TurnRelayTransport),
}

impl P2pTransport {
    /// Sends a framed message regardless of the underlying transport.
    pub async fn send(&self, payload: &[u8]) -> Result<()> {
        match self {
            Self::Direct(t) => t.send(payload).await,
            Self::Relayed(t) => t.send(payload).await,
        }
    }

    /// Receives a framed message regardless of the underlying transport.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        match self {
            Self::Direct(t) => t.recv().await,
            Self::Relayed(t) => t.recv().await,
        }
    }

    /// Returns `true` if this is a direct transport.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct(_))
    }

    /// Returns `true` if this is a relayed transport.
    #[must_use]
    pub fn is_relayed(&self) -> bool {
        matches!(self, Self::Relayed(_))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Framing tests --

    #[test]
    fn encode_decode_roundtrip() {
        let payload = b"hello, peer!";
        let frame = encode_frame(payload).unwrap();
        assert_eq!(frame.len(), FRAME_HEADER_SIZE + payload.len());
        let decoded = decode_frame(&frame).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn encode_max_payload() {
        let payload = vec![0xAB; MAX_PAYLOAD_SIZE];
        let frame = encode_frame(&payload).unwrap();
        let decoded = decode_frame(&frame).unwrap();
        assert_eq!(decoded, payload.as_slice());
    }

    #[test]
    fn encode_rejects_empty() {
        let result = encode_frame(b"");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn encode_rejects_oversized() {
        let payload = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        let result = encode_frame(&payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn decode_rejects_too_short() {
        let result = decode_frame(&[0x00]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn decode_rejects_truncated() {
        // Header says 10 bytes but only 3 available.
        let data = [0x00, 0x0A, 0x01, 0x02, 0x03];
        let result = decode_frame(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("truncated"));
    }

    #[test]
    fn decode_rejects_zero_length() {
        let data = [0x00, 0x00];
        let result = decode_frame(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zero-length"));
    }

    #[test]
    fn frame_preserves_binary_data() {
        let payload: Vec<u8> = (0..=255).collect();
        // 256 bytes < MAX_PAYLOAD_SIZE, so this is fine
        let frame = encode_frame(&payload).unwrap();
        let decoded = decode_frame(&frame).unwrap();
        assert_eq!(decoded, payload.as_slice());
    }

    // -- UdpTransport loopback tests --

    #[tokio::test]
    async fn udp_transport_send_recv() {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let addr_a = sock_a.local_addr().unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        sock_a.connect(addr_b).await.unwrap();
        sock_b.connect(addr_a).await.unwrap();

        let transport_a = UdpTransport::new(Arc::new(sock_a));
        let transport_b = UdpTransport::new(Arc::new(sock_b));

        let msg = b"hello from A";
        transport_a.send(msg).await.unwrap();
        let received = transport_b.recv().await.unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn udp_transport_bidirectional() {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let addr_a = sock_a.local_addr().unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        sock_a.connect(addr_b).await.unwrap();
        sock_b.connect(addr_a).await.unwrap();

        let transport_a = UdpTransport::new(Arc::new(sock_a));
        let transport_b = UdpTransport::new(Arc::new(sock_b));

        // A -> B
        transport_a.send(b"ping").await.unwrap();
        assert_eq!(transport_b.recv().await.unwrap(), b"ping");

        // B -> A
        transport_b.send(b"pong").await.unwrap();
        assert_eq!(transport_a.recv().await.unwrap(), b"pong");
    }

    #[tokio::test]
    async fn udp_transport_rejects_oversized() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.connect("127.0.0.1:1234").await.unwrap();
        let transport = UdpTransport::new(Arc::new(sock));

        let big = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        let result = transport.send(&big).await;
        assert!(result.is_err());
    }

    // -- TurnRelayTransport tests --

    #[tokio::test]
    async fn turn_relay_send_creates_send_indication() {
        // Set up a mock "TURN server" that receives the Send Indication
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();
        let client_sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());

        let peer_addr: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let transport = TurnRelayTransport::new(
            Arc::clone(&client_sock),
            server_addr,
            peer_addr,
        );

        let payload = b"relayed message";
        transport.send(payload).await.unwrap();

        // Server receives and parses the Send Indication
        let mut buf = [0u8; 4096];
        let (n, _src) = server_sock.recv_from(&mut buf).await.unwrap();

        // The Send Indication contains the framed payload
        let msg = turn::TurnMessage::from_bytes(&buf[..n]).unwrap();
        assert!(msg.is_send_indication());

        // Extract the data from the Send Indication
        let data = msg.data().expect("missing DATA attribute");

        // The data should be a framed message (length prefix + payload)
        let decoded = decode_frame(data).unwrap();
        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    async fn turn_relay_recv_parses_data_indication() {
        use crate::stun::TransactionId;
        use crate::turn::{TurnAttribute, TurnMessage};

        let client_sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client_addr = client_sock.local_addr().unwrap();

        let sender_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = sender_sock.local_addr().unwrap();
        let peer_addr: SocketAddr = "10.0.0.1:9000".parse().unwrap();

        let transport = TurnRelayTransport::new(
            Arc::clone(&client_sock),
            server_addr,
            peer_addr,
        );

        // Simulate the TURN server sending a Data Indication with a framed payload
        let payload = b"data from peer";
        let framed = encode_frame(payload).unwrap();
        let indication = TurnMessage {
            msg_type: 0x0017, // DATA_INDICATION
            transaction_id: TransactionId::random(),
            attributes: vec![
                TurnAttribute::XorPeerAddress(peer_addr),
                TurnAttribute::Data(framed),
            ],
        };
        let bytes = indication.to_bytes(None);
        sender_sock.send_to(&bytes, client_addr).await.unwrap();

        let received = transport.recv().await.unwrap();
        assert_eq!(received, payload);
    }

    // -- P2pTransport enum tests --

    #[tokio::test]
    async fn p2p_transport_direct_send_recv() {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let addr_a = sock_a.local_addr().unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        sock_a.connect(addr_b).await.unwrap();
        sock_b.connect(addr_a).await.unwrap();

        let transport_a = P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a)));
        let transport_b = P2pTransport::Direct(UdpTransport::new(Arc::new(sock_b)));

        assert!(transport_a.is_direct());
        assert!(!transport_a.is_relayed());

        transport_a.send(b"via p2p").await.unwrap();
        let received = transport_b.recv().await.unwrap();
        assert_eq!(received, b"via p2p");
    }

    #[tokio::test]
    async fn p2p_transport_relayed_variant() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let server: SocketAddr = "127.0.0.1:3478".parse().unwrap();
        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();

        let transport = P2pTransport::Relayed(TurnRelayTransport::new(sock, server, peer));
        assert!(transport.is_relayed());
        assert!(!transport.is_direct());
    }

    // -- Keep-alive tests --

    #[tokio::test]
    async fn keepalive_sends_stun_binding_request() {
        let peer_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = peer_sock.local_addr().unwrap();

        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.connect(peer_addr).await.unwrap();
        let mut transport = UdpTransport::new(Arc::new(sock));

        // Start keep-alive with a very short interval (1 second).
        transport.start_keepalive(Some(1));

        // Wait for a keep-alive packet on the peer side.
        let mut buf = [0u8; 256];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            peer_sock.recv_from(&mut buf),
        )
        .await;

        assert!(result.is_ok(), "should have received a keep-alive packet");
        let (n, _src) = result.unwrap().unwrap();

        // Verify it's a STUN Binding Request.
        let msg = StunMessage::from_bytes(&buf[..n]).unwrap();
        assert_eq!(msg.msg_type, 0x0001); // BINDING_REQUEST

        transport.stop_keepalive();
    }

    #[tokio::test]
    async fn relay_keepalive_sends_stun_binding_request() {
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();

        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let mut transport = TurnRelayTransport::new(sock, server_addr, peer);

        transport.start_keepalive(Some(1));

        let mut buf = [0u8; 256];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            server_sock.recv_from(&mut buf),
        )
        .await;

        assert!(result.is_ok(), "should have received a relay keep-alive");
        let (n, _src) = result.unwrap().unwrap();

        let msg = StunMessage::from_bytes(&buf[..n]).unwrap();
        assert_eq!(msg.msg_type, 0x0001);

        transport.stop_keepalive();
    }

    #[tokio::test]
    async fn keepalive_stop_prevents_further_packets() {
        let peer_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = peer_sock.local_addr().unwrap();

        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.connect(peer_addr).await.unwrap();
        let mut transport = UdpTransport::new(Arc::new(sock));

        transport.start_keepalive(Some(1));

        // Wait for first keep-alive.
        let mut buf = [0u8; 256];
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            peer_sock.recv_from(&mut buf),
        )
        .await;

        // Stop keep-alive.
        transport.stop_keepalive();

        // Wait a bit, then check that no more packets arrive.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            peer_sock.recv_from(&mut buf),
        )
        .await;

        assert!(result.is_err(), "should not receive packets after stop");
    }

    #[tokio::test]
    async fn keepalive_restart_after_stop_works() {
        let peer_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = peer_sock.local_addr().unwrap();

        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.connect(peer_addr).await.unwrap();
        let mut transport = UdpTransport::new(Arc::new(sock));

        // Start, then immediately stop keep-alive.
        transport.start_keepalive(Some(1));
        transport.stop_keepalive();

        // Restart keep-alive — this must not exit prematurely due to a
        // stored Notify permit from the previous stop.
        transport.start_keepalive(Some(1));

        // Verify the restarted keep-alive actually sends packets.
        let mut buf = [0u8; 256];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            peer_sock.recv_from(&mut buf),
        )
        .await;

        assert!(
            result.is_ok(),
            "restarted keep-alive should send packets"
        );
        let (n, _src) = result.unwrap().unwrap();
        let msg = StunMessage::from_bytes(&buf[..n]).unwrap();
        assert_eq!(msg.msg_type, 0x0001);

        transport.stop_keepalive();
    }

    #[tokio::test]
    async fn relay_keepalive_restart_after_stop_works() {
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();

        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let mut transport = TurnRelayTransport::new(sock, server_addr, peer);

        // Start, then immediately stop keep-alive.
        transport.start_keepalive(Some(1));
        transport.stop_keepalive();

        // Restart keep-alive — must not exit prematurely.
        transport.start_keepalive(Some(1));

        // Verify the restarted keep-alive sends packets.
        let mut buf = [0u8; 256];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            server_sock.recv_from(&mut buf),
        )
        .await;

        assert!(
            result.is_ok(),
            "restarted relay keep-alive should send packets"
        );
        let (n, _src) = result.unwrap().unwrap();
        let msg = StunMessage::from_bytes(&buf[..n]).unwrap();
        assert_eq!(msg.msg_type, 0x0001);

        transport.stop_keepalive();
    }
}
