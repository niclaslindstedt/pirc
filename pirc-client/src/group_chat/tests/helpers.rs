//! Shared test utilities for group chat tests.

use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_p2p::encrypted_transport::TransportCipher;
use pirc_p2p::transport::{P2pTransport, UdpTransport};
use pirc_p2p::EncryptedP2pTransport;
use std::sync::Arc;
use tokio::net::UdpSocket;

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

    let receiver = TripleRatchetSession::init_receiver(&shared_secret, bob_dh, bob_kem)
        .expect("init_receiver");

    (sender, receiver)
}

/// A no-op cipher for testing (data passes through unchanged).
struct NoopCipher;

impl TransportCipher for NoopCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Ok(ciphertext.to_vec())
    }
}

/// Creates a connected pair of mock encrypted P2P transports.
pub async fn mock_transport_pair() -> (EncryptedP2pTransport, EncryptedP2pTransport) {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    let transport_a = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a))),
        Box::new(NoopCipher),
    );
    let transport_b = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_b))),
        Box::new(NoopCipher),
    );

    (transport_a, transport_b)
}

/// Creates a single mock transport (self-connected, for send-only tests).
pub async fn mock_transport() -> EncryptedP2pTransport {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap();
    sock.connect(addr).await.unwrap();
    let udp = UdpTransport::new(Arc::new(sock));
    EncryptedP2pTransport::new(P2pTransport::Direct(udp), Box::new(NoopCipher))
}
