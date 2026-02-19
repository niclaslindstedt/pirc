//! Encrypted P2P transport integration tests.
//!
//! Exercises the encrypted transport wrapper: send/recv round-trips, key
//! rotation, data-on-wire validation, bidirectional communication, and
//! error paths.

use std::sync::Arc;

use pirc_p2p::encrypted_transport::{EncryptedP2pTransport, TransportCipher};
use pirc_p2p::transport::{P2pTransport, UdpTransport, MAX_PAYLOAD_SIZE};
use tokio::net::UdpSocket;

use super::make_connected_udp_pair;

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

/// A cipher that tracks how many times encrypt/decrypt has been called,
/// simulating a ratchet that advances state on each operation.
struct CountingCipher {
    encrypt_count: u32,
    decrypt_count: u32,
}

impl CountingCipher {
    fn new() -> Self {
        Self {
            encrypt_count: 0,
            decrypt_count: 0,
        }
    }
}

impl TransportCipher for CountingCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        self.encrypt_count += 1;
        // XOR with the encrypt count as a key byte (simulates key evolution)
        #[allow(clippy::cast_possible_truncation)]
        let key = self.encrypt_count as u8;
        Ok(plaintext.iter().map(|b| b ^ key).collect())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        self.decrypt_count += 1;
        #[allow(clippy::cast_possible_truncation)]
        let key = self.decrypt_count as u8;
        Ok(ciphertext.iter().map(|b| b ^ key).collect())
    }
}

/// A cipher that always fails.
struct FailCipher;

impl TransportCipher for FailCipher {
    fn encrypt(&mut self, _plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Err("encryption failed".into())
    }

    fn decrypt(&mut self, _ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Err("decryption failed".into())
    }
}

async fn make_encrypted_pair(
    cipher_a: Box<dyn TransportCipher>,
    cipher_b: Box<dyn TransportCipher>,
) -> (EncryptedP2pTransport, EncryptedP2pTransport) {
    let (sock_a, sock_b) = make_connected_udp_pair().await;

    let transport_a = P2pTransport::Direct(UdpTransport::new(sock_a));
    let transport_b = P2pTransport::Direct(UdpTransport::new(sock_b));

    (
        EncryptedP2pTransport::new(transport_a, cipher_a),
        EncryptedP2pTransport::new(transport_b, cipher_b),
    )
}

// --- Basic round-trip ---

#[tokio::test]
async fn encrypted_send_recv_roundtrip() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0x42)),
        Box::new(XorCipher::new(0x42)),
    )
    .await;

    let msg = b"hello encrypted world";
    enc_a.send(msg).await.unwrap();
    let received = enc_b.recv().await.unwrap();
    assert_eq!(received, msg);
}

#[tokio::test]
async fn encrypted_bidirectional() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0xAB)),
        Box::new(XorCipher::new(0xAB)),
    )
    .await;

    // A -> B
    enc_a.send(b"ping").await.unwrap();
    assert_eq!(enc_b.recv().await.unwrap(), b"ping");

    // B -> A
    enc_b.send(b"pong").await.unwrap();
    assert_eq!(enc_a.recv().await.unwrap(), b"pong");
}

#[tokio::test]
async fn encrypted_multiple_messages() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0x55)),
        Box::new(XorCipher::new(0x55)),
    )
    .await;

    for i in 0..5 {
        let msg = format!("message {i}");
        enc_a.send(msg.as_bytes()).await.unwrap();
        let received = enc_b.recv().await.unwrap();
        assert_eq!(received, msg.as_bytes());
    }
}

// --- Data on wire is encrypted ---

#[tokio::test]
async fn data_is_encrypted_on_wire() {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let addr_a = sock_a.local_addr().unwrap();
    let addr_b = sock_b.local_addr().unwrap();

    sock_a.connect(addr_b).await.unwrap();
    sock_b.connect(addr_a).await.unwrap();

    let raw_sock_b = Arc::new(sock_b);

    let enc_a = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(Arc::new(sock_a))),
        Box::new(XorCipher::new(0xFF)),
    );

    let plaintext = b"secret message";
    enc_a.send(plaintext).await.unwrap();

    // Read raw bytes from the wire
    let mut buf = [0u8; 2048];
    let n = raw_sock_b.recv(&mut buf).await.unwrap();
    let wire_data = &buf[..n];

    // Plaintext should NOT appear on the wire
    assert!(
        !wire_data.windows(plaintext.len()).any(|w| w == plaintext),
        "plaintext should not appear on the wire"
    );
}

// --- Key rotation (simulated) ---

#[tokio::test]
async fn key_rotation_via_counting_cipher() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(CountingCipher::new()),
        Box::new(CountingCipher::new()),
    )
    .await;

    // Each message uses a different key byte (simulating key rotation)
    for i in 0..3 {
        let msg = format!("msg {i}");
        enc_a.send(msg.as_bytes()).await.unwrap();
        let received = enc_b.recv().await.unwrap();
        assert_eq!(received, msg.as_bytes());
    }
}

// --- Max payload ---

#[tokio::test]
async fn encrypted_max_payload() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0x42)),
        Box::new(XorCipher::new(0x42)),
    )
    .await;

    let payload = vec![0xAB; MAX_PAYLOAD_SIZE];
    enc_a.send(&payload).await.unwrap();
    let received = enc_b.recv().await.unwrap();
    assert_eq!(received, payload);
}

// --- Error paths ---

#[tokio::test]
async fn encrypt_failure_returns_error() {
    let (sock_a, _sock_b) = make_connected_udp_pair().await;
    let transport_a = P2pTransport::Direct(UdpTransport::new(sock_a));
    let enc = EncryptedP2pTransport::new(transport_a, Box::new(FailCipher));

    let result = enc.send(b"test").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("encryption failed"));
}

#[tokio::test]
async fn decrypt_failure_returns_error() {
    let (sock_a, sock_b) = make_connected_udp_pair().await;

    // Send raw (unencrypted) data
    let plain_transport = P2pTransport::Direct(UdpTransport::new(sock_a));
    plain_transport.send(b"raw data").await.unwrap();

    // Receive with a failing cipher
    let enc_b = EncryptedP2pTransport::new(
        P2pTransport::Direct(UdpTransport::new(sock_b)),
        Box::new(FailCipher),
    );
    let result = enc_b.recv().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("decryption failed"));
}

#[tokio::test]
async fn mismatched_keys_produce_wrong_plaintext() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0x42)),
        Box::new(XorCipher::new(0x99)), // different key
    )
    .await;

    let msg = b"secret data";
    enc_a.send(msg).await.unwrap();
    let received = enc_b.recv().await.unwrap();
    assert_ne!(received.as_slice(), msg);
}

// --- Transport type delegation ---

#[tokio::test]
async fn is_direct_and_relayed_delegates() {
    let (sock_a, _sock_b) = make_connected_udp_pair().await;
    let transport = P2pTransport::Direct(UdpTransport::new(sock_a));
    let enc = EncryptedP2pTransport::new(transport, Box::new(XorCipher::new(0)));

    assert!(enc.is_direct());
    assert!(!enc.is_relayed());
}

#[tokio::test]
async fn into_inner_returns_transport() {
    let (sock_a, _sock_b) = make_connected_udp_pair().await;
    let transport = P2pTransport::Direct(UdpTransport::new(sock_a));
    let enc = EncryptedP2pTransport::new(transport, Box::new(XorCipher::new(0)));

    let inner = enc.into_inner();
    assert!(inner.is_direct());
}

// --- Empty payload ---

#[tokio::test]
async fn empty_payload_rejected() {
    let (sock_a, _sock_b) = make_connected_udp_pair().await;
    let transport = P2pTransport::Direct(UdpTransport::new(sock_a));
    let enc = EncryptedP2pTransport::new(transport, Box::new(XorCipher::new(0)));

    let result = enc.send(b"").await;
    assert!(result.is_err());
}

// --- Binary data preserved ---

#[tokio::test]
async fn encrypted_preserves_binary_data() {
    let (enc_a, enc_b) = make_encrypted_pair(
        Box::new(XorCipher::new(0x42)),
        Box::new(XorCipher::new(0x42)),
    )
    .await;

    let payload: Vec<u8> = (0..=255).collect();
    enc_a.send(&payload).await.unwrap();
    let received = enc_b.recv().await.unwrap();
    assert_eq!(received, payload);
}
