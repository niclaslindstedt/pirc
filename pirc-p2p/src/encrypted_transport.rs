//! Encrypted P2P transport wrapper.
//!
//! Wraps a [`P2pTransport`] with encryption/decryption callbacks so that
//! all data sent over the P2P link is end-to-end encrypted. The actual
//! cryptographic operations are provided through the [`TransportCipher`]
//! trait, keeping this module decoupled from any specific crypto library.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::debug;

use crate::error::{P2pError, Result};
use crate::transport::P2pTransport;

/// Trait abstracting encryption/decryption for the transport layer.
///
/// Implementors provide the actual crypto operations (e.g. triple ratchet
/// encrypt/decrypt). The trait uses `&mut self` because ratchet-based
/// protocols advance internal state on each operation.
pub trait TransportCipher: Send {
    /// Encrypt plaintext, returning the ciphertext bytes to send on the wire.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption fails (e.g. no active session).
    fn encrypt(&mut self, plaintext: &[u8]) -> std::result::Result<Vec<u8>, String>;

    /// Decrypt ciphertext received from the wire, returning the plaintext.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails (e.g. invalid ciphertext,
    /// message authentication failure).
    fn decrypt(&mut self, ciphertext: &[u8]) -> std::result::Result<Vec<u8>, String>;
}

/// Encrypted P2P transport that wraps a [`P2pTransport`] with a
/// [`TransportCipher`].
///
/// All outbound data is encrypted before being passed to the inner
/// transport, and all inbound data is decrypted after being received.
pub struct EncryptedP2pTransport {
    inner: P2pTransport,
    cipher: Arc<Mutex<Box<dyn TransportCipher>>>,
}

impl EncryptedP2pTransport {
    /// Creates a new encrypted transport wrapping the given inner transport
    /// and cipher.
    pub fn new(inner: P2pTransport, cipher: Box<dyn TransportCipher>) -> Self {
        Self {
            inner,
            cipher: Arc::new(Mutex::new(cipher)),
        }
    }

    /// Encrypts the payload and sends it over the inner transport.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption or the underlying send fails.
    pub async fn send(&self, plaintext: &[u8]) -> Result<()> {
        let ciphertext = {
            let mut cipher = self.cipher.lock().await;
            cipher
                .encrypt(plaintext)
                .map_err(|e| P2pError::Ice(format!("encryption failed: {e}")))?
        };
        debug!(
            plaintext_len = plaintext.len(),
            ciphertext_len = ciphertext.len(),
            "encrypted P2P payload"
        );
        self.inner.send(&ciphertext).await
    }

    /// Receives data from the inner transport and decrypts it.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying receive or decryption fails.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        let ciphertext = self.inner.recv().await?;
        let plaintext = {
            let mut cipher = self.cipher.lock().await;
            cipher
                .decrypt(&ciphertext)
                .map_err(|e| P2pError::Ice(format!("decryption failed: {e}")))?
        };
        debug!(
            ciphertext_len = ciphertext.len(),
            plaintext_len = plaintext.len(),
            "decrypted P2P payload"
        );
        Ok(plaintext)
    }

    /// Returns whether the inner transport is a direct connection.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        self.inner.is_direct()
    }

    /// Returns whether the inner transport is relayed via TURN.
    #[must_use]
    pub fn is_relayed(&self) -> bool {
        self.inner.is_relayed()
    }

    /// Consumes the encrypted transport and returns the inner transport.
    #[must_use]
    pub fn into_inner(self) -> P2pTransport {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{UdpTransport, MAX_PAYLOAD_SIZE};
    use std::sync::Arc as StdArc;
    use tokio::net::UdpSocket;

    /// A simple XOR cipher for testing purposes.
    struct XorCipher {
        key: u8,
    }

    impl XorCipher {
        fn new(key: u8) -> Self {
            Self { key }
        }
    }

    impl TransportCipher for XorCipher {
        fn encrypt(&mut self, plaintext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Ok(plaintext.iter().map(|b| b ^ self.key).collect())
        }

        fn decrypt(&mut self, ciphertext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            // XOR is its own inverse
            Ok(ciphertext.iter().map(|b| b ^ self.key).collect())
        }
    }

    /// A cipher that always fails, for testing error paths.
    struct FailCipher;

    impl TransportCipher for FailCipher {
        fn encrypt(&mut self, _plaintext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("encrypt failed".into())
        }

        fn decrypt(&mut self, _ciphertext: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("decrypt failed".into())
        }
    }

    async fn make_connected_pair() -> (UdpTransport, UdpTransport) {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let addr_a = sock_a.local_addr().unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        sock_a.connect(addr_b).await.unwrap();
        sock_b.connect(addr_a).await.unwrap();

        (
            UdpTransport::new(StdArc::new(sock_a)),
            UdpTransport::new(StdArc::new(sock_b)),
        )
    }

    #[tokio::test]
    async fn encrypted_send_recv_roundtrip() {
        let (transport_a, transport_b) = make_connected_pair().await;

        let enc_a = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_a),
            Box::new(XorCipher::new(0x42)),
        );
        let enc_b = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_b),
            Box::new(XorCipher::new(0x42)),
        );

        let msg = b"hello encrypted world";
        enc_a.send(msg).await.unwrap();
        let received = enc_b.recv().await.unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn encrypted_bidirectional() {
        let (transport_a, transport_b) = make_connected_pair().await;

        let enc_a = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_a),
            Box::new(XorCipher::new(0xAB)),
        );
        let enc_b = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_b),
            Box::new(XorCipher::new(0xAB)),
        );

        // A -> B
        enc_a.send(b"ping").await.unwrap();
        assert_eq!(enc_b.recv().await.unwrap(), b"ping");

        // B -> A
        enc_b.send(b"pong").await.unwrap();
        assert_eq!(enc_a.recv().await.unwrap(), b"pong");
    }

    #[tokio::test]
    async fn data_is_encrypted_on_wire() {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let addr_a = sock_a.local_addr().unwrap();
        let addr_b = sock_b.local_addr().unwrap();

        sock_a.connect(addr_b).await.unwrap();
        sock_b.connect(addr_a).await.unwrap();

        let raw_sock_b = StdArc::new(sock_b);

        let enc_a = EncryptedP2pTransport::new(
            P2pTransport::Direct(UdpTransport::new(StdArc::new(sock_a))),
            Box::new(XorCipher::new(0xFF)),
        );

        let plaintext = b"secret message";
        enc_a.send(plaintext).await.unwrap();

        // Read raw bytes from the wire (includes 2-byte frame header)
        let mut buf = [0u8; 2048];
        let n = raw_sock_b.recv(&mut buf).await.unwrap();

        // The raw wire data should NOT contain the plaintext
        let wire_data = &buf[..n];
        assert!(
            !wire_data
                .windows(plaintext.len())
                .any(|w| w == plaintext),
            "plaintext should not appear on the wire"
        );
    }

    #[tokio::test]
    async fn encrypt_failure_returns_error() {
        let (transport_a, _transport_b) = make_connected_pair().await;

        let enc = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_a),
            Box::new(FailCipher),
        );

        let result = enc.send(b"test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("encryption failed"));
    }

    #[tokio::test]
    async fn decrypt_failure_returns_error() {
        let (transport_a, transport_b) = make_connected_pair().await;

        // Send with a working cipher
        let plain_transport = P2pTransport::Direct(transport_a);
        plain_transport.send(b"raw data").await.unwrap();

        // Receive with a failing cipher
        let enc_b = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_b),
            Box::new(FailCipher),
        );

        let result = enc_b.recv().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("decryption failed"));
    }

    #[tokio::test]
    async fn is_direct_and_relayed_delegates() {
        let (transport, _) = make_connected_pair().await;

        let enc = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport),
            Box::new(XorCipher::new(0)),
        );

        assert!(enc.is_direct());
        assert!(!enc.is_relayed());
    }

    #[tokio::test]
    async fn mismatched_keys_produce_wrong_plaintext() {
        let (transport_a, transport_b) = make_connected_pair().await;

        let enc_a = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_a),
            Box::new(XorCipher::new(0x42)),
        );
        let enc_b = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_b),
            Box::new(XorCipher::new(0x99)), // Different key
        );

        let msg = b"secret data";
        enc_a.send(msg).await.unwrap();
        let received = enc_b.recv().await.unwrap();

        // With mismatched XOR keys, the data will be garbled
        assert_ne!(received.as_slice(), msg);
    }

    #[tokio::test]
    async fn empty_payload_rejected() {
        let (transport, _) = make_connected_pair().await;

        let enc = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport),
            Box::new(XorCipher::new(0)),
        );

        // XOR cipher preserves length, so empty -> empty -> transport rejects
        let result = enc.send(b"").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn max_payload_encrypted() {
        let (transport_a, transport_b) = make_connected_pair().await;

        let enc_a = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_a),
            Box::new(XorCipher::new(0x42)),
        );
        let enc_b = EncryptedP2pTransport::new(
            P2pTransport::Direct(transport_b),
            Box::new(XorCipher::new(0x42)),
        );

        // XOR cipher preserves payload size, so MAX_PAYLOAD_SIZE should work
        let payload = vec![0xAB; MAX_PAYLOAD_SIZE];
        enc_a.send(&payload).await.unwrap();
        let received = enc_b.recv().await.unwrap();
        assert_eq!(received, payload);
    }
}
