//! P2P connection and NAT traversal integration tests.
//!
//! Exercises the full pirc-p2p API through loopback networking:
//! STUN/TURN protocol compliance, ICE candidate gathering, connectivity
//! checking, P2P session lifecycle, and encrypted transport.
//!
//! Test modules are organized by scenario category:
//! - `stun_protocol` — STUN binding request/response encoding and loopback
//! - `turn_protocol` — TURN allocation, permissions, relay, and channel data
//! - `ice_gathering` — ICE candidate gathering and prioritization
//! - `connectivity` — connectivity checking with candidate pairs
//! - `session_lifecycle` — P2P session state machine and signaling events
//! - `encrypted_transport` — encrypted P2P transport round-trips and key rotation

mod connectivity;
mod encrypted_transport;
mod ice_gathering;
mod session_lifecycle;
mod stun_protocol;
mod turn_protocol;

use std::net::SocketAddr;
use std::sync::Arc;

use pirc_p2p::ice::{CandidateType, IceCandidate};
use pirc_p2p::stun::{StunAttribute, StunMessage};
use tokio::net::UdpSocket;

// =========================================================================
// Helpers
// =========================================================================

/// Spawn a mock STUN server that responds with the client's source address as
/// the XOR-MAPPED-ADDRESS. Returns the server socket address.
pub async fn spawn_mock_stun_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server_sock.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            let result = server_sock.recv_from(&mut buf).await;
            match result {
                Ok((len, src)) => {
                    if let Ok(request) = StunMessage::from_bytes(&buf[..len]) {
                        let response = StunMessage {
                            msg_type: 0x0101, // Binding Response
                            transaction_id: request.transaction_id,
                            attributes: vec![StunAttribute::XorMappedAddress(src)],
                        };
                        let _ = server_sock.send_to(&response.to_bytes(), src).await;
                    }
                }
                Err(_) => break,
            }
        }
    });

    (server_addr, handle)
}

/// Create a pair of connected UDP sockets for loopback testing.
pub async fn make_connected_udp_pair() -> (Arc<UdpSocket>, Arc<UdpSocket>) {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    (Arc::new(sock_a), Arc::new(sock_b))
}

/// Create a host ICE candidate from an address string.
pub fn host_candidate(addr: &str) -> IceCandidate {
    IceCandidate::new(
        CandidateType::Host,
        addr.parse().unwrap(),
        65535,
        "host1".into(),
        1,
    )
}
