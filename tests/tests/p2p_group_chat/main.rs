//! P2P encrypted group chat integration tests.
//!
//! Exercises the full group chat lifecycle: mesh formation, encrypted
//! messaging, peer join/leave, server-relay fallback, and client-level
//! group chat management.
//!
//! Test modules are organized by scenario category:
//! - `mesh_formation` — 3-peer mesh establishment, topology tracking, events
//! - `group_messaging` — broadcast, E2E encryption, ordering, large messages
//! - `peer_join_leave` — graceful/ungraceful leave, group continuity
//! - `server_relay_fallback` — fallback when P2P fails, still E2E encrypted
//! - `client_group_chat` — client creates group, invites, P2P messaging

mod client_group_chat;
mod group_messaging;
mod mesh_formation;
mod peer_join_leave;
mod server_relay_fallback;

use std::collections::HashSet;
use std::sync::Arc;

use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_p2p::encrypted_transport::{EncryptedP2pTransport, TransportCipher};
use pirc_p2p::group_mesh::GroupMesh;
use pirc_p2p::transport::{P2pTransport, UdpTransport};
use tokio::net::UdpSocket;

// =========================================================================
// Helpers
// =========================================================================

/// XOR cipher for testing. Stateless and symmetric.
struct XorCipher {
    key: u8,
}

impl XorCipher {
    fn new(key: u8) -> Self {
        Self { key }
    }
}

impl TransportCipher for XorCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plaintext.iter().map(|b| b ^ self.key).collect())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(ciphertext.iter().map(|b| b ^ self.key).collect())
    }
}

/// No-op cipher for testing (data passes through unchanged).
struct NoopCipher;

impl TransportCipher for NoopCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(ciphertext.to_vec())
    }
}

/// Create a pair of connected encrypted P2P transports over loopback.
pub async fn make_encrypted_transport_pair(
    cipher_a: Box<dyn TransportCipher>,
    cipher_b: Box<dyn TransportCipher>,
) -> (EncryptedP2pTransport, EncryptedP2pTransport) {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    let transport_a = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a))),
        cipher_a,
    );
    let transport_b = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_b))),
        cipher_b,
    );

    (transport_a, transport_b)
}

/// Create a single self-connected mock transport (for mesh tracking tests
/// where actual communication isn't needed).
pub async fn mock_transport() -> EncryptedP2pTransport {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap();
    sock.connect(addr).await.unwrap();
    let udp = UdpTransport::new(Arc::new(sock));
    EncryptedP2pTransport::new(P2pTransport::Direct(udp), Box::new(NoopCipher))
}

/// Create a pair of linked triple ratchet sessions for testing.
pub fn create_test_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
    let shared_secret = [0x42u8; 32];
    let bob_dh = x25519::KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let sender = TripleRatchetSession::init_sender(
        &shared_secret,
        bob_dh.public_key(),
        bob_kem.public_key(),
    )
    .expect("init_sender");

    let receiver =
        TripleRatchetSession::init_receiver(&shared_secret, bob_dh, bob_kem).expect("init_receiver");

    (sender, receiver)
}

/// Create a test session pair with a unique shared secret (for multi-pair tests).
pub fn create_test_session_pair_unique(seed: u8) -> (TripleRatchetSession, TripleRatchetSession) {
    let shared_secret = [seed; 32];
    let bob_dh = x25519::KeyPair::generate();
    let bob_kem = KemKeyPair::generate();

    let sender = TripleRatchetSession::init_sender(
        &shared_secret,
        bob_dh.public_key(),
        bob_kem.public_key(),
    )
    .expect("init_sender");

    let receiver =
        TripleRatchetSession::init_receiver(&shared_secret, bob_dh, bob_kem).expect("init_receiver");

    (sender, receiver)
}

/// Build a `HashSet<String>` from a slice of name literals.
pub fn members(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

/// Set up a 3-member group mesh with all members connected.
pub async fn setup_three_peer_mesh() -> GroupMesh {
    let mut mesh = GroupMesh::new("test-group".into());
    mesh.add_member("alice".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    mesh.member_connected("alice".into(), mock_transport().await);
    mesh.member_connected("bob".into(), mock_transport().await);
    mesh.member_connected("charlie".into(), mock_transport().await);

    // Clear setup events
    mesh.drain_events();

    mesh
}
