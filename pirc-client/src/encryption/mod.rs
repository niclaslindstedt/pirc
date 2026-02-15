//! Client-side encryption manager for E2E encrypted private messages.
//!
//! Manages identity keys, triple ratchet sessions per peer, and the
//! key exchange state machine. Provides encrypt/decrypt operations and
//! pre-key bundle generation for server upload.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pirc_crypto::identity::{IdentityKeyPair, IdentityPublicKey};
use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::prekey::{KemPreKey, OneTimePreKey, PreKeyBundle, SignedPreKey};
use pirc_crypto::protocol::KeyExchangeMessage;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519;
use pirc_crypto::x3dh::{self, X3DHInitMessage};
use pirc_crypto::CryptoError;

/// Number of one-time pre-keys to generate in a batch.
const ONE_TIME_PRE_KEY_BATCH_SIZE: u32 = 10;

/// Encryption status for a peer conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionStatus {
    /// No encryption session or pending exchange.
    None,
    /// Key exchange is in progress.
    Establishing,
    /// Active encrypted session.
    Active,
}

/// Client-side encryption manager.
///
/// Holds long-term identity keys, manages per-peer triple ratchet sessions,
/// and drives the X3DH key exchange state machine. All private messages are
/// encrypted and decrypted through this manager.
pub struct EncryptionManager {
    /// Our long-term identity key pair.
    identity: IdentityKeyPair,
    /// Active triple ratchet sessions, keyed by peer nickname.
    sessions: HashMap<String, TripleRatchetSession>,
    /// Pending key exchanges (waiting for bundle response, etc.).
    pending_exchanges: HashMap<String, PendingExchange>,
    /// Our signed pre-key (medium-term, for bundle upload).
    signed_pre_key: SignedPreKey,
    /// Our KEM pre-key (post-quantum, for bundle upload).
    kem_pre_key: KemPreKey,
    /// Our one-time pre-keys (ephemeral, for bundle upload).
    one_time_pre_keys: Vec<OneTimePreKey>,
    /// Peer identity public keys, for fingerprint lookups.
    peer_identities: HashMap<String, IdentityPublicKey>,
}

/// State machine for a pending key exchange with a peer.
pub enum PendingExchange {
    /// We requested their bundle, waiting for response.
    AwaitingBundle {
        /// Messages queued while waiting for the bundle.
        queued_messages: Vec<Vec<u8>>,
    },
    /// We sent X3DH init, waiting for Complete.
    AwaitingComplete {
        /// The session established during X3DH (not yet promoted).
        session: Box<TripleRatchetSession>,
        /// Messages queued during the exchange.
        queued_messages: Vec<Vec<u8>>,
    },
}

impl EncryptionManager {
    /// Create a new encryption manager with freshly generated keys.
    ///
    /// Generates an identity key pair, signed pre-key, KEM pre-key, and
    /// a batch of one-time pre-keys.
    ///
    /// # Panics
    ///
    /// Panics if key generation fails (should not happen with valid RNG).
    #[must_use]
    pub fn new() -> Self {
        let identity = IdentityKeyPair::generate();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        let signed_pre_key = SignedPreKey::generate(1, &identity, timestamp)
            .expect("signed pre-key generation should not fail");
        let kem_pre_key =
            KemPreKey::generate(1, &identity).expect("KEM pre-key generation should not fail");

        let mut one_time_pre_keys = Vec::with_capacity(ONE_TIME_PRE_KEY_BATCH_SIZE as usize);
        for i in 0..ONE_TIME_PRE_KEY_BATCH_SIZE {
            one_time_pre_keys.push(OneTimePreKey::generate(i));
        }

        Self {
            identity,
            sessions: HashMap::new(),
            pending_exchanges: HashMap::new(),
            signed_pre_key,
            kem_pre_key,
            one_time_pre_keys,
            peer_identities: HashMap::new(),
        }
    }

    /// Check whether an active session exists with a peer.
    #[must_use]
    pub fn has_session(&self, peer: &str) -> bool {
        self.sessions.contains_key(peer)
    }

    /// Get the encryption status for a peer.
    #[must_use]
    pub fn encryption_status(&self, peer: &str) -> EncryptionStatus {
        if self.sessions.contains_key(peer) {
            EncryptionStatus::Active
        } else if self.pending_exchanges.contains_key(peer) {
            EncryptionStatus::Establishing
        } else {
            EncryptionStatus::None
        }
    }

    /// Encrypt a message for a peer.
    ///
    /// Requires an active session with the peer. Returns
    /// [`CryptoError::Ratchet`] if no session exists.
    ///
    /// # Errors
    ///
    /// Returns an error if no session exists or encryption fails.
    pub fn encrypt(
        &mut self,
        peer: &str,
        plaintext: &[u8],
    ) -> Result<EncryptedMessage, CryptoError> {
        let session = self.sessions.get_mut(peer).ok_or_else(|| {
            CryptoError::Ratchet(format!("no active session with peer '{peer}'"))
        })?;
        session.encrypt(plaintext)
    }

    /// Decrypt a message from a peer.
    ///
    /// Requires an active session with the peer. Returns
    /// [`CryptoError::Ratchet`] if no session exists.
    ///
    /// # Errors
    ///
    /// Returns an error if no session exists or decryption fails.
    pub fn decrypt(
        &mut self,
        peer: &str,
        msg: &EncryptedMessage,
    ) -> Result<Vec<u8>, CryptoError> {
        let session = self.sessions.get_mut(peer).ok_or_else(|| {
            CryptoError::Ratchet(format!("no active session with peer '{peer}'"))
        })?;
        session.decrypt(msg)
    }

    /// Build our pre-key bundle for upload to the server.
    ///
    /// Uses the first available one-time pre-key (if any).
    #[must_use]
    pub fn create_pre_key_bundle(&self) -> PreKeyBundle {
        let otpk = self.one_time_pre_keys.first().map(OneTimePreKey::to_public);
        PreKeyBundle::new(
            self.identity.public_identity(),
            self.signed_pre_key.to_public(),
            self.kem_pre_key.to_public(),
            otpk,
        )
    }

    /// Get our identity fingerprint as a hex string.
    #[must_use]
    pub fn get_identity_fingerprint(&self) -> String {
        hex_fingerprint(&self.identity.public_identity().fingerprint())
    }

    /// Get a peer's identity fingerprint as a hex string.
    ///
    /// Returns `None` if we have never exchanged keys with the peer.
    #[must_use]
    pub fn get_peer_fingerprint(&self, peer: &str) -> Option<String> {
        self.peer_identities
            .get(peer)
            .map(|pk| hex_fingerprint(&pk.fingerprint()))
    }

    // ── Key exchange methods ─────────────────────────────────────────

    /// Initiate a key exchange with a peer.
    ///
    /// Creates a `PendingExchange::AwaitingBundle` state and returns
    /// a [`KeyExchangeMessage::RequestBundle`] to send to the server.
    pub fn initiate_key_exchange(&mut self, peer: &str) -> KeyExchangeMessage {
        self.pending_exchanges.insert(
            peer.to_owned(),
            PendingExchange::AwaitingBundle {
                queued_messages: Vec::new(),
            },
        );
        KeyExchangeMessage::RequestBundle
    }

    /// Queue a plaintext message for a peer whose key exchange is pending.
    ///
    /// Returns `true` if the message was queued (exchange is pending),
    /// or `false` if there is no pending exchange for this peer.
    pub fn queue_message(&mut self, peer: &str, plaintext: Vec<u8>) -> bool {
        if let Some(pending) = self.pending_exchanges.get_mut(peer) {
            match pending {
                PendingExchange::AwaitingBundle { queued_messages }
                | PendingExchange::AwaitingComplete { queued_messages, .. } => {
                    queued_messages.push(plaintext);
                }
            }
            true
        } else {
            false
        }
    }

    /// Handle a received pre-key bundle from a peer.
    ///
    /// Performs the X3DH sender-side exchange, creates a triple ratchet
    /// session, and encrypts any queued messages.
    ///
    /// Returns the X3DH init message to send and any encrypted queued
    /// messages.
    ///
    /// # Errors
    ///
    /// Returns an error if X3DH or session initialization fails.
    pub fn handle_bundle_response(
        &mut self,
        peer: &str,
        bundle: &PreKeyBundle,
    ) -> Result<(X3DHInitMessage, Vec<EncryptedMessage>), CryptoError> {
        // Perform X3DH sender-side exchange
        let (sender_result, init_message) = x3dh::x3dh_sender(&self.identity, bundle)?;

        // Store the peer's identity for fingerprint lookups
        self.peer_identities
            .insert(peer.to_owned(), bundle.identity_public().clone());

        // Create a triple ratchet session (sender side).
        // Use the receiver's signed pre-key (DH) and KEM pre-key public
        // keys from the bundle — the receiver will use the matching private
        // keys to initialize their session.
        let session = TripleRatchetSession::init_sender(
            sender_result.shared_secret(),
            bundle.signed_pre_key().public_key(),
            bundle.kem_pre_key().public_key(),
        )?;

        // Extract queued messages from the pending exchange
        let queued = if let Some(PendingExchange::AwaitingBundle { queued_messages }) =
            self.pending_exchanges.remove(peer)
        {
            queued_messages
        } else {
            Vec::new()
        };

        // Store session in AwaitingComplete state so we can encrypt queued messages
        // and still wait for the Complete acknowledgment
        let mut temp_session = session;
        let mut encrypted_queued = Vec::new();
        for msg in &queued {
            encrypted_queued.push(temp_session.encrypt(msg)?);
        }

        // Move to AwaitingComplete state
        self.pending_exchanges.insert(
            peer.to_owned(),
            PendingExchange::AwaitingComplete {
                session: Box::new(temp_session),
                queued_messages: Vec::new(),
            },
        );

        Ok((init_message, encrypted_queued))
    }

    /// Handle an incoming X3DH init message (we are the receiver).
    ///
    /// Performs the X3DH receiver-side exchange, creates a triple ratchet
    /// session, and returns a [`KeyExchangeMessage::Complete`] to send back.
    ///
    /// # Errors
    ///
    /// Returns an error if X3DH or session initialization fails.
    pub fn handle_init_message(
        &mut self,
        peer: &str,
        init: &X3DHInitMessage,
    ) -> Result<KeyExchangeMessage, CryptoError> {
        // Find the matching one-time pre-key if used
        let otpk = init.used_one_time_pre_key_id().and_then(|id| {
            self.one_time_pre_keys
                .iter()
                .position(|k| k.id() == id)
                .map(|pos| self.one_time_pre_keys.remove(pos))
        });

        // Perform X3DH receiver-side exchange
        let receiver_result = x3dh::x3dh_receiver(
            &self.identity,
            &self.signed_pre_key,
            &self.kem_pre_key,
            otpk.as_ref(),
            init,
        )?;

        // Store the peer's identity for fingerprint lookups
        self.peer_identities
            .insert(peer.to_owned(), init.sender_identity().clone());

        // Create a triple ratchet session (receiver side).
        // Use our signed pre-key (DH) and KEM pre-key key pairs — the
        // sender initialized with the matching public keys from the bundle.
        let ratchet_dh = x25519::KeyPair::from_secret_bytes(
            self.signed_pre_key.key_pair().secret_key().to_bytes(),
        );
        let ratchet_kem = KemKeyPair::from_bytes(&self.kem_pre_key.kem_pair().to_bytes())
            .map_err(|e| CryptoError::Ratchet(format!("failed to reconstruct KEM key pair: {e}")))?;

        let session = TripleRatchetSession::init_receiver(
            receiver_result.shared_secret(),
            ratchet_dh,
            ratchet_kem,
        )?;

        self.sessions.insert(peer.to_owned(), session);

        Ok(KeyExchangeMessage::Complete)
    }

    /// Handle a Complete acknowledgment from a peer.
    ///
    /// Promotes the pending session to an active session.
    pub fn handle_complete(&mut self, peer: &str) {
        if let Some(PendingExchange::AwaitingComplete { session, .. }) =
            self.pending_exchanges.remove(peer)
        {
            self.sessions.insert(peer.to_owned(), *session);
        }
    }

    /// Remove/reset a session with a peer.
    ///
    /// Also removes any pending exchange and stored peer identity.
    pub fn remove_session(&mut self, peer: &str) {
        self.sessions.remove(peer);
        self.pending_exchanges.remove(peer);
        self.peer_identities.remove(peer);
    }

    /// Check whether a key exchange is pending for a peer.
    #[must_use]
    pub fn has_pending_exchange(&self, peer: &str) -> bool {
        self.pending_exchanges.contains_key(peer)
    }

    /// Return a list of all peers with their encryption status.
    ///
    /// Includes peers with active sessions and peers with pending exchanges.
    #[must_use]
    pub fn list_peers(&self) -> Vec<(String, EncryptionStatus)> {
        let mut peers: Vec<(String, EncryptionStatus)> = Vec::new();
        for peer in self.sessions.keys() {
            peers.push((peer.clone(), EncryptionStatus::Active));
        }
        for peer in self.pending_exchanges.keys() {
            if !self.sessions.contains_key(peer) {
                peers.push((peer.clone(), EncryptionStatus::Establishing));
            }
        }
        peers.sort_by(|a, b| a.0.cmp(&b.0));
        peers
    }

    /// Return a reference to our identity public key.
    #[must_use]
    pub fn identity_public(&self) -> IdentityPublicKey {
        self.identity.public_identity()
    }
}

impl Default for EncryptionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a fingerprint byte array as a colon-separated uppercase hex string.
fn hex_fingerprint(fp: &[u8; 32]) -> String {
    fp.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Identity key generation ──────────────────────────────────────

    #[test]
    fn new_generates_identity_keys() {
        let mgr = EncryptionManager::new();
        let fp = mgr.get_identity_fingerprint();
        // Fingerprint is 32 bytes as hex with colons: "XX:XX:...:XX" = 32*3 - 1 = 95 chars
        assert_eq!(fp.len(), 95);
        assert!(fp.contains(':'));
    }

    #[test]
    fn two_managers_have_different_fingerprints() {
        let mgr1 = EncryptionManager::new();
        let mgr2 = EncryptionManager::new();
        assert_ne!(
            mgr1.get_identity_fingerprint(),
            mgr2.get_identity_fingerprint()
        );
    }

    // ── Pre-key bundle ───────────────────────────────────────────────

    #[test]
    fn pre_key_bundle_validates() {
        let mgr = EncryptionManager::new();
        let bundle = mgr.create_pre_key_bundle();
        bundle.validate().expect("bundle should be valid");
    }

    #[test]
    fn pre_key_bundle_has_one_time_pre_key() {
        let mgr = EncryptionManager::new();
        let bundle = mgr.create_pre_key_bundle();
        assert!(bundle.one_time_pre_key().is_some());
    }

    // ── Session lifecycle ────────────────────────────────────────────

    #[test]
    fn no_session_initially() {
        let mgr = EncryptionManager::new();
        assert!(!mgr.has_session("alice"));
    }

    #[test]
    fn encrypt_without_session_fails() {
        let mut mgr = EncryptionManager::new();
        let result = mgr.encrypt("alice", b"hello");
        assert!(result.is_err());
    }

    #[test]
    fn initiate_key_exchange_creates_pending() {
        let mut mgr = EncryptionManager::new();
        let msg = mgr.initiate_key_exchange("bob");
        assert!(matches!(msg, KeyExchangeMessage::RequestBundle));
        assert!(mgr.has_pending_exchange("bob"));
        assert!(!mgr.has_session("bob"));
    }

    #[test]
    fn full_key_exchange_flow() {
        // Run on a thread with a larger stack for ML-DSA key generation.
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let mut alice = EncryptionManager::new();
                let mut bob = EncryptionManager::new();

                // 1. Alice initiates key exchange with Bob
                let _request = alice.initiate_key_exchange("bob");

                // 2. Bob creates his pre-key bundle (simulating server response)
                let bob_bundle = bob.create_pre_key_bundle();

                // 3. Alice handles Bob's bundle → gets init message
                let (init_msg, _encrypted_queued) = alice
                    .handle_bundle_response("bob", &bob_bundle)
                    .expect("handle_bundle_response failed");

                // Alice is now in AwaitingComplete state
                assert!(alice.has_pending_exchange("bob"));
                assert!(!alice.has_session("bob"));

                // 4. Bob handles Alice's init message → session established on Bob's side
                let complete_msg = bob
                    .handle_init_message("alice", &init_msg)
                    .expect("handle_init_message failed");
                assert!(matches!(complete_msg, KeyExchangeMessage::Complete));
                assert!(bob.has_session("alice"));

                // 5. Alice handles Complete → session established on Alice's side
                alice.handle_complete("bob");
                assert!(alice.has_session("bob"));
                assert!(!alice.has_pending_exchange("bob"));
            })
            .expect("thread spawn failed")
            .join();

        result.expect("full_key_exchange_flow panicked");
    }

    // ── Encrypt/decrypt roundtrip ────────────────────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (mut alice, mut bob) = establish_session();

                // Alice encrypts, Bob decrypts
                let plaintext = b"Hello, Bob!";
                let encrypted = alice.encrypt("bob", plaintext).expect("encrypt failed");
                let decrypted = bob.decrypt("alice", &encrypted).expect("decrypt failed");
                assert_eq!(decrypted, plaintext);

                // Bob encrypts, Alice decrypts
                let reply = b"Hello back, Alice!";
                let encrypted_reply = bob.encrypt("alice", reply).expect("encrypt reply failed");
                let decrypted_reply = alice
                    .decrypt("bob", &encrypted_reply)
                    .expect("decrypt reply failed");
                assert_eq!(decrypted_reply, reply);
            })
            .expect("thread spawn failed")
            .join();

        result.expect("encrypt_decrypt_roundtrip panicked");
    }

    // ── Queued messages ──────────────────────────────────────────────

    #[test]
    fn queued_messages_sent_after_session_establishment() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let mut alice = EncryptionManager::new();
                let mut bob = EncryptionManager::new();

                // Alice initiates and queues messages
                let _request = alice.initiate_key_exchange("bob");
                assert!(alice.queue_message("bob", b"msg1".to_vec()));
                assert!(alice.queue_message("bob", b"msg2".to_vec()));

                // Queue fails for unknown peer
                assert!(!alice.queue_message("charlie", b"nope".to_vec()));

                // Bob provides bundle
                let bob_bundle = bob.create_pre_key_bundle();
                let (init_msg, encrypted_queued) = alice
                    .handle_bundle_response("bob", &bob_bundle)
                    .expect("handle_bundle_response failed");

                // Two queued messages should have been encrypted
                assert_eq!(encrypted_queued.len(), 2);

                // Bob handles init → session established
                let _complete = bob
                    .handle_init_message("alice", &init_msg)
                    .expect("handle_init_message failed");

                // Bob should be able to decrypt the queued messages
                let d1 = bob.decrypt("alice", &encrypted_queued[0]).expect("decrypt q1");
                let d2 = bob.decrypt("alice", &encrypted_queued[1]).expect("decrypt q2");
                assert_eq!(d1, b"msg1");
                assert_eq!(d2, b"msg2");
            })
            .expect("thread spawn failed")
            .join();

        result.expect("queued_messages panicked");
    }

    // ── Fingerprints ─────────────────────────────────────────────────

    #[test]
    fn peer_fingerprint_available_after_exchange() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (alice, bob) = establish_session();

                // Alice should know Bob's fingerprint
                let bob_fp = alice.get_peer_fingerprint("bob");
                assert!(bob_fp.is_some());

                // Bob should know Alice's fingerprint
                let alice_fp = bob.get_peer_fingerprint("alice");
                assert!(alice_fp.is_some());

                // Cross-check: Alice's view of Bob's fingerprint should match
                // Bob's self-reported fingerprint
                assert_eq!(bob_fp.unwrap(), bob.get_identity_fingerprint());
                assert_eq!(alice_fp.unwrap(), alice.get_identity_fingerprint());

                // Unknown peer has no fingerprint
                assert!(alice.get_peer_fingerprint("charlie").is_none());
            })
            .expect("thread spawn failed")
            .join();

        result.expect("peer_fingerprint panicked");
    }

    // ── Session removal and re-establishment ─────────────────────────

    #[test]
    fn session_removal_and_reestablishment() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (mut alice, _bob) = establish_session();

                assert!(alice.has_session("bob"));
                alice.remove_session("bob");
                assert!(!alice.has_session("bob"));
                assert!(alice.get_peer_fingerprint("bob").is_none());

                // Can re-initiate
                let msg = alice.initiate_key_exchange("bob");
                assert!(matches!(msg, KeyExchangeMessage::RequestBundle));
                assert!(alice.has_pending_exchange("bob"));
            })
            .expect("thread spawn failed")
            .join();

        result.expect("session_removal panicked");
    }

    // ── Hex fingerprint formatting ───────────────────────────────────

    #[test]
    fn hex_fingerprint_format() {
        let fp = [
            0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
            0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x10, 0x20, 0x30, 0x40,
            0x50, 0x60, 0x70, 0x80,
        ];
        let s = hex_fingerprint(&fp);
        assert_eq!(
            s,
            "AB:CD:EF:01:23:45:67:89:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:10:20:30:40:50:60:70:80"
        );
    }

    // ── Test helper ──────────────────────────────────────────────────

    // ── Encryption status ────────────────────────────────────────────

    #[test]
    fn encryption_status_none_initially() {
        let mgr = EncryptionManager::new();
        assert_eq!(mgr.encryption_status("alice"), EncryptionStatus::None);
    }

    #[test]
    fn encryption_status_establishing_during_exchange() {
        let mut mgr = EncryptionManager::new();
        let _msg = mgr.initiate_key_exchange("bob");
        assert_eq!(mgr.encryption_status("bob"), EncryptionStatus::Establishing);
    }

    #[test]
    fn encryption_status_active_after_session() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (alice, _bob) = establish_session();
                assert_eq!(alice.encryption_status("bob"), EncryptionStatus::Active);
            })
            .expect("thread spawn failed")
            .join();

        result.expect("encryption_status_active panicked");
    }

    #[test]
    fn encryption_status_none_after_removal() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (mut alice, _bob) = establish_session();
                assert_eq!(alice.encryption_status("bob"), EncryptionStatus::Active);
                alice.remove_session("bob");
                assert_eq!(alice.encryption_status("bob"), EncryptionStatus::None);
            })
            .expect("thread spawn failed")
            .join();

        result.expect("encryption_status_none_after_removal panicked");
    }

    // ── list_peers ──────────────────────────────────────────────────

    #[test]
    fn list_peers_empty_initially() {
        let mgr = EncryptionManager::new();
        assert!(mgr.list_peers().is_empty());
    }

    #[test]
    fn list_peers_shows_pending() {
        let mut mgr = EncryptionManager::new();
        let _msg = mgr.initiate_key_exchange("bob");
        let peers = mgr.list_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].0, "bob");
        assert_eq!(peers[0].1, EncryptionStatus::Establishing);
    }

    #[test]
    fn list_peers_shows_active() {
        let result = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let (alice, _bob) = establish_session();
                let peers = alice.list_peers();
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].0, "bob");
                assert_eq!(peers[0].1, EncryptionStatus::Active);
            })
            .expect("thread spawn failed")
            .join();

        result.expect("list_peers_shows_active panicked");
    }

    // ── Test helper ──────────────────────────────────────────────────

    /// Establish a session between Alice and Bob and return both managers.
    fn establish_session() -> (EncryptionManager, EncryptionManager) {
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
