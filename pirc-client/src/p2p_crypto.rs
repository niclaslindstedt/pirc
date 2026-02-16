//! Bridge between P2P transport encryption and the client encryption manager.
//!
//! Provides [`RatchetCipher`], an implementation of [`TransportCipher`] that
//! delegates to the client's [`EncryptionManager`] for per-peer triple ratchet
//! encrypt/decrypt operations. This allows [`EncryptedP2pTransport`] to
//! transparently use the existing E2E encryption sessions.

use std::sync::Arc;

use pirc_crypto::message::EncryptedMessage;
use pirc_p2p::TransportCipher;
use tokio::sync::Mutex;

use crate::encryption::EncryptionManager;

/// A [`TransportCipher`] backed by the client's [`EncryptionManager`].
///
/// Encrypts and decrypts P2P transport data using the triple ratchet session
/// for a specific peer. The encryption manager is shared (behind
/// `Arc<Mutex<>>`) so that both server-relayed and P2P-transported messages
/// use the same ratchet state.
pub struct RatchetCipher {
    encryption: Arc<Mutex<EncryptionManager>>,
    peer: String,
}

impl RatchetCipher {
    /// Creates a new ratchet cipher for the given peer.
    ///
    /// The `encryption` manager must already have an active session with
    /// `peer` for encryption/decryption to succeed.
    pub fn new(encryption: Arc<Mutex<EncryptionManager>>, peer: String) -> Self {
        Self { encryption, peer }
    }
}

impl TransportCipher for RatchetCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        // We need to block on the async mutex from a sync context.
        // The TransportCipher trait is sync, but it's called from an async
        // context (EncryptedP2pTransport::send which holds the Mutex lock).
        // Use try_lock to avoid deadlocks — the caller already holds the
        // outer cipher lock so we should be the only accessor.
        let mut mgr = self
            .encryption
            .try_lock()
            .map_err(|_| "encryption manager is locked".to_string())?;
        let encrypted = mgr.encrypt(&self.peer, plaintext).map_err(|e| e.to_string())?;
        Ok(encrypted.to_bytes())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let msg = EncryptedMessage::from_bytes(ciphertext).map_err(|e| e.to_string())?;
        let mut mgr = self
            .encryption
            .try_lock()
            .map_err(|_| "encryption manager is locked".to_string())?;
        mgr.decrypt(&self.peer, &msg).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratchet_cipher_encrypt_without_session_fails() {
        let mgr = Arc::new(Mutex::new(EncryptionManager::new()));
        let mut cipher = RatchetCipher::new(mgr, "unknown_peer".to_string());

        let result = cipher.encrypt(b"hello");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no active session"));
    }

    #[test]
    fn ratchet_cipher_decrypt_without_session_fails() {
        let mgr = Arc::new(Mutex::new(EncryptionManager::new()));
        let mut cipher = RatchetCipher::new(mgr, "unknown_peer".to_string());

        // Create some dummy bytes (won't even get to session lookup
        // since EncryptedMessage::from_bytes will fail on garbage)
        let result = cipher.decrypt(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn ratchet_cipher_roundtrip_with_established_session() {
        // Run on a thread with a larger stack for ML-DSA key generation.
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (alice_mgr, bob_mgr) = establish_sessions();

                let alice_enc = Arc::new(Mutex::new(alice_mgr));
                let bob_enc = Arc::new(Mutex::new(bob_mgr));

                let mut alice_cipher = RatchetCipher::new(Arc::clone(&alice_enc), "bob".to_string());
                let mut bob_cipher = RatchetCipher::new(Arc::clone(&bob_enc), "alice".to_string());

                // Alice encrypts, Bob decrypts
                let plaintext = b"hello over P2P";
                let ciphertext = alice_cipher.encrypt(plaintext).unwrap();
                let decrypted = bob_cipher.decrypt(&ciphertext).unwrap();
                assert_eq!(decrypted, plaintext);

                // Bob encrypts, Alice decrypts
                let reply = b"reply from Bob";
                let ct_reply = bob_cipher.encrypt(reply).unwrap();
                let decrypted_reply = alice_cipher.decrypt(&ct_reply).unwrap();
                assert_eq!(decrypted_reply, reply);
            })
            .expect("thread spawn failed")
            .join();

        result.expect("ratchet_cipher_roundtrip panicked");
    }

    #[test]
    fn ratchet_cipher_multiple_messages() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (alice_mgr, bob_mgr) = establish_sessions();

                let alice_enc = Arc::new(Mutex::new(alice_mgr));
                let bob_enc = Arc::new(Mutex::new(bob_mgr));

                let mut alice_cipher = RatchetCipher::new(Arc::clone(&alice_enc), "bob".to_string());
                let mut bob_cipher = RatchetCipher::new(Arc::clone(&bob_enc), "alice".to_string());

                // Send multiple messages in one direction
                for i in 0..5 {
                    let msg = format!("message {i}");
                    let ct = alice_cipher.encrypt(msg.as_bytes()).unwrap();
                    let pt = bob_cipher.decrypt(&ct).unwrap();
                    assert_eq!(pt, msg.as_bytes());
                }
            })
            .expect("thread spawn failed")
            .join();

        result.expect("ratchet_cipher_multiple_messages panicked");
    }

    /// Establish encryption sessions between Alice and Bob.
    fn establish_sessions() -> (EncryptionManager, EncryptionManager) {
        let mut alice = EncryptionManager::new();
        let mut bob = EncryptionManager::new();

        let _request = alice.initiate_key_exchange("bob");
        let bob_bundle = bob.create_pre_key_bundle();

        let (init_msg, _) = alice
            .handle_bundle_response("bob", &bob_bundle)
            .expect("handle_bundle_response failed");

        let _complete = bob
            .handle_init_message("alice", &init_msg)
            .expect("handle_init_message failed");

        alice.handle_complete("bob");

        (alice, bob)
    }
}
