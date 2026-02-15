//! Combined triple ratchet session.
//!
//! Ties together the DH ratchet, symmetric-key ratchet, and post-quantum
//! ratchet into a single session state machine. Manages session
//! initialization, message encryption/decryption, ratchet advancement,
//! and out-of-order message handling.

use std::collections::HashMap;

use crate::aead;
use crate::dh_ratchet::DhRatchetState;
use crate::error::Result;
use crate::header::{self, HeaderKey};
use crate::kdf;
use crate::kem::{KemKeyPair, KemPublicKey};
use crate::message::{EncryptedMessage, MessageHeader};
use crate::pq_ratchet::PqRatchetState;
use crate::symmetric_ratchet::MessageKey;
use crate::x25519;

/// Maximum number of skipped message keys cached for out-of-order
/// message decryption. Prevents unbounded memory growth from an
/// adversary requesting large skip distances.
const MAX_SKIPPED_KEYS: usize = 1000;

/// Default number of DH ratchet steps between PQ ratchet steps.
const DEFAULT_PQ_INTERVAL: u32 = 20;

/// Maximum number of old header keys retained for decrypting
/// headers of cached skipped message keys across DH epochs.
const MAX_RETAINED_HEADER_KEYS: usize = 10;

/// Info string for deriving initial header keys from the shared secret.
const HEADER_KEY_INFO: &[u8] = b"pirc-triple-ratchet-header-keys";

/// Info string for deriving the initial root key from the shared secret.
const ROOT_KEY_INFO: &[u8] = b"pirc-triple-ratchet-root";

/// Info string for deriving the initial PQ chain key from the shared secret.
const PQ_CHAIN_KEY_INFO: &[u8] = b"pirc-triple-ratchet-pq-chain";

/// A triple ratchet encryption session.
///
/// Combines three ratcheting mechanisms for maximum security:
///
/// 1. **Symmetric ratchet** — advances per-message, derives unique keys
/// 2. **DH ratchet** — advances per-round-trip, provides classical
///    break-in recovery
/// 3. **PQ ratchet** — advances periodically, provides post-quantum
///    resistance
///
/// Use [`init_sender`](Self::init_sender) or
/// [`init_receiver`](Self::init_receiver) to create a new session,
/// then call [`encrypt`](Self::encrypt) and [`decrypt`](Self::decrypt)
/// to exchange messages.
pub struct TripleRatchetSession {
    /// The DH ratchet state (manages X25519 key pairs and root key).
    dh_ratchet: DhRatchetState,
    /// The PQ ratchet state (manages ML-KEM key pairs and PQ chain key).
    pq_ratchet: PqRatchetState,
    /// Header key for encrypting outgoing headers.
    sending_header_key: HeaderKey,
    /// Header key for decrypting incoming headers.
    receiving_header_key: HeaderKey,
    /// Previous receiving header key for trial decryption of
    /// out-of-order messages from the prior DH epoch.
    previous_receiving_header_key: Option<HeaderKey>,
    /// Retained old header keys for decrypting headers of cached
    /// skipped message keys across multiple DH epochs.  Limited in
    /// size to avoid unbounded growth.
    retained_header_keys: Vec<HeaderKey>,
    /// Next sending header key, applied after the next DH ratchet step.
    next_sending_header_key: HeaderKey,
    /// Next receiving header key, applied after the next DH ratchet step.
    next_receiving_header_key: HeaderKey,
    /// Cached message keys for out-of-order decryption.
    /// Keyed by (DH public key bytes, message number).
    skipped_keys: HashMap<([u8; 32], u32), MessageKey>,
    /// Number of DH ratchet steps between PQ ratchet steps.
    pq_step_interval: u32,
    /// Counter tracking DH ratchet steps since the last PQ step.
    dh_steps_since_pq: u32,
    /// Number of messages sent in the previous sending chain
    /// (communicated in the header for out-of-order handling).
    previous_chain_length: u32,
    /// Number of messages sent in the current sending chain.
    sending_message_number: u32,
}

impl TripleRatchetSession {
    /// Initialize a session as the sender (Alice).
    ///
    /// Alice knows Bob's DH public key and KEM public key from an
    /// out-of-band key exchange. She performs the initial DH ratchet
    /// step and sets up all three ratchets.
    ///
    /// # Arguments
    ///
    /// * `shared_secret` — a 32-byte pre-shared secret (e.g. from X3DH)
    /// * `remote_dh_public` — Bob's X25519 public key
    /// * `remote_kem_public` — Bob's ML-KEM public key
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if key derivation or the initial DH step fails.
    pub fn init_sender(
        shared_secret: &[u8; 32],
        remote_dh_public: x25519::PublicKey,
        remote_kem_public: KemPublicKey,
    ) -> Result<Self> {
        let root_key_bytes = derive_initial_key(shared_secret, ROOT_KEY_INFO)?;

        // Derive initial header keys (96 bytes = 3 x 32-byte keys):
        //   [0] = sender's initial sending HK
        //   [1] = sender's initial receiving HK
        //   [2] = initial next receiving HK (shared between both sides)
        let header_key_material = kdf::derive_key(shared_secret, b"", HEADER_KEY_INFO, 96)?;
        let sending_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 0));
        let receiving_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 1));
        let next_receiving_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 2));

        // The DH init step produces a header key alongside the chain key.
        // This becomes the sender's *next* sending header key — the remote
        // party will derive the same value during its first ratchet step.
        let (dh_ratchet, init_hk) =
            DhRatchetState::init_sender(root_key_bytes, remote_dh_public)?;
        let next_sending_hk = HeaderKey::from_bytes(init_hk);

        let pq_chain_key = derive_initial_key(shared_secret, PQ_CHAIN_KEY_INFO)?;
        let mut pq_ratchet = PqRatchetState::new(pq_chain_key);
        pq_ratchet.set_remote_public_key(remote_kem_public);

        Ok(Self {
            dh_ratchet,
            pq_ratchet,
            sending_header_key: sending_hk,
            receiving_header_key: receiving_hk,
            previous_receiving_header_key: None,
            retained_header_keys: Vec::new(),
            next_sending_header_key: next_sending_hk,
            next_receiving_header_key: next_receiving_hk,
            skipped_keys: HashMap::new(),
            pq_step_interval: DEFAULT_PQ_INTERVAL,
            dh_steps_since_pq: 0,
            previous_chain_length: 0,
            sending_message_number: 0,
        })
    }

    /// Initialize a session as the receiver (Bob).
    ///
    /// Bob starts with his own DH and KEM key pairs. He does not have
    /// a sending chain yet — it is created when Alice's first message
    /// triggers a DH ratchet step.
    ///
    /// # Arguments
    ///
    /// * `shared_secret` — a 32-byte pre-shared secret (must match
    ///   the sender's)
    /// * `dh_pair` — Bob's X25519 key pair (whose public key was
    ///   shared with Alice)
    /// * `kem_pair` — Bob's ML-KEM key pair (whose public key was
    ///   shared with Alice)
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if key derivation fails.
    pub fn init_receiver(
        shared_secret: &[u8; 32],
        dh_pair: x25519::KeyPair,
        kem_pair: KemKeyPair,
    ) -> Result<Self> {
        let root_key_bytes = derive_initial_key(shared_secret, ROOT_KEY_INFO)?;

        // Receiver's send/recv are swapped relative to sender:
        //   sender's [0]=send → receiver's recv
        //   sender's [1]=recv → receiver's send
        //   [2] = shared next header key (same for both sides)
        let header_key_material = kdf::derive_key(shared_secret, b"", HEADER_KEY_INFO, 96)?;
        let sending_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 1));
        let receiving_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 0));
        // The receiver's initial next_sending_header_key is key[2].
        // Alice's next_receiving_header_key is also key[2], so after
        // Bob's first ratchet (which promotes next→current), both
        // sides agree.
        let next_sending_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 2));
        // The receiver's next_receiving_header_key is a placeholder;
        // it will be replaced with a ratchet-derived value during
        // the first DH ratchet step.
        let next_receiving_hk = HeaderKey::from_bytes(extract_key(&header_key_material, 2));

        let dh_ratchet = DhRatchetState::init_receiver(root_key_bytes, dh_pair);

        // Use the provided KEM key pair so the sender (who has our
        // public key) can encapsulate to us during PQ ratchet steps.
        let pq_chain_key = derive_initial_key(shared_secret, PQ_CHAIN_KEY_INFO)?;
        let pq_ratchet = PqRatchetState::with_keypair(pq_chain_key, kem_pair);

        Ok(Self {
            dh_ratchet,
            pq_ratchet,
            sending_header_key: sending_hk,
            receiving_header_key: receiving_hk,
            previous_receiving_header_key: None,
            retained_header_keys: Vec::new(),
            next_sending_header_key: next_sending_hk,
            next_receiving_header_key: next_receiving_hk,
            skipped_keys: HashMap::new(),
            pq_step_interval: DEFAULT_PQ_INTERVAL,
            dh_steps_since_pq: 0,
            previous_chain_length: 0,
            sending_message_number: 0,
        })
    }

    /// Encrypt a plaintext message.
    ///
    /// Advances the sending symmetric ratchet to get a message key,
    /// builds and encrypts the header, and encrypts the message body.
    /// If a PQ ratchet step is due, it includes KEM data in the header.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if the sending chain is not initialized,
    /// header encryption fails, or body encryption fails.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<EncryptedMessage> {
        // Check if a PQ ratchet step is due
        let (kem_ciphertext, kem_public) =
            if self.pq_step_interval > 0 && self.dh_steps_since_pq >= self.pq_step_interval {
                let (ct, new_pk) = self.pq_ratchet.initiate_step()?;
                self.dh_steps_since_pq = 0;
                (Some(ct), Some(new_pk))
            } else {
                (None, None)
            };

        // Get message key and DH public key from the DH ratchet
        let (message_key, msg_num, dh_public) = self.dh_ratchet.encrypt_message_key()?;
        self.sending_message_number = msg_num + 1;

        let header = MessageHeader {
            dh_public,
            message_number: msg_num,
            previous_chain_length: self.previous_chain_length,
            kem_ciphertext,
            kem_public,
        };

        let (encrypted_header, header_nonce) =
            header::encrypt_header(&self.sending_header_key, &header)?;

        let body_nonce = aead::generate_nonce();
        let ciphertext = aead::encrypt(message_key.as_bytes(), &body_nonce, plaintext, b"")?;

        Ok(EncryptedMessage {
            encrypted_header,
            header_nonce,
            ciphertext,
            body_nonce,
        })
    }

    /// Decrypt an encrypted message.
    ///
    /// Decrypts the header to extract ratchet metadata, performs any
    /// needed DH/PQ ratchet steps, obtains the message key, and
    /// decrypts the body.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`] if:
    /// - the header key cannot decrypt the header
    /// - a DH ratchet step fails
    /// - the message key cannot be derived (skip limit exceeded)
    /// - body decryption fails
    pub fn decrypt(&mut self, message: &EncryptedMessage) -> Result<Vec<u8>> {
        // Try to find a cached skipped key for this message
        if let Some(plaintext) = self.try_skipped_keys(message)? {
            return Ok(plaintext);
        }

        // Trial-decrypt the header.
        //
        // We try the next receiving header key first: if it succeeds,
        // the remote party has performed a DH ratchet step and moved to
        // a new epoch.  If that fails, we try the current key (same
        // epoch) and then the previous key (late-arriving message from
        // the prior epoch).
        let (header, dh_changed) = self.try_decrypt_header_and_detect_ratchet(message)?;

        if dh_changed {
            // Before the DH ratchet step, save any unseen keys from the
            // old receiving chain so out-of-order messages can still be
            // decrypted later.
            if let Some(old_remote) = self.dh_ratchet.remote_public_key() {
                let old_keys = self
                    .dh_ratchet
                    .skip_remaining_receiving_keys(header.previous_chain_length)?;
                for (num, mk) in old_keys {
                    self.store_skipped_key(&old_remote, num, mk);
                }
            }

            // Rotate header keys: save current receiving as previous,
            // promote next keys to current, and retain the old key for
            // skipped-key lookups across multiple epochs.
            if let Some(prev) = self.previous_receiving_header_key.replace(
                self.receiving_header_key.clone(),
            ) {
                if self.retained_header_keys.len() < MAX_RETAINED_HEADER_KEYS {
                    self.retained_header_keys.push(prev);
                }
            }
            self.receiving_header_key = self.next_receiving_header_key.clone();
            self.sending_header_key = self.next_sending_header_key.clone();

            // Perform DH ratchet step — this creates new sending and
            // receiving chains based on the new remote DH key.
            // The step returns header keys derived alongside the chain
            // keys: (receiving_hk, sending_hk).
            let (recv_hk, send_hk) =
                self.dh_ratchet.ratchet_step(header.dh_public)?;
            self.dh_steps_since_pq += 1;

            // Store the derived header keys as the *next* keys — they
            // will become active on the following DH ratchet step.
            self.next_receiving_header_key = HeaderKey::from_bytes(recv_hk);
            self.next_sending_header_key = HeaderKey::from_bytes(send_hk);

            // Reset sending chain counters
            self.previous_chain_length = self.sending_message_number;
            self.sending_message_number = 0;
        }

        // Handle PQ ratchet data if present
        if let (Some(ref ct), Some(ref new_pk)) = (&header.kem_ciphertext, &header.kem_public) {
            self.pq_ratchet.complete_step(ct, new_pk)?;
        }

        // Obtain the message key, storing any intermediate skipped keys
        let (message_key, skipped) = self
            .dh_ratchet
            .receive_message_key(header.message_number)?;

        for (num, mk) in skipped {
            self.store_skipped_key(&header.dh_public, num, mk);
        }

        let plaintext = aead::decrypt(
            message_key.as_bytes(),
            &message.body_nonce,
            &message.ciphertext,
            b"",
        )?;

        Ok(plaintext)
    }

    /// Set the interval (in DH ratchet steps) between PQ ratchet steps.
    pub fn set_pq_interval(&mut self, interval: u32) {
        self.pq_step_interval = interval;
    }

    /// Return the number of completed PQ ratchet steps.
    #[must_use]
    pub fn pq_step_count(&self) -> u32 {
        self.pq_ratchet.step_counter()
    }

    /// Return the number of cached skipped keys.
    #[must_use]
    pub fn skipped_key_count(&self) -> usize {
        self.skipped_keys.len()
    }

    /// Try to decrypt using a cached skipped key.
    fn try_skipped_keys(&mut self, message: &EncryptedMessage) -> Result<Option<Vec<u8>>> {
        if self.skipped_keys.is_empty() {
            return Ok(None);
        }

        // Try all known header keys: current, next, previous, and
        // any retained from older epochs.
        let mut candidates = vec![
            self.receiving_header_key.clone(),
            self.next_receiving_header_key.clone(),
        ];
        if let Some(ref prev_hk) = self.previous_receiving_header_key {
            candidates.push(prev_hk.clone());
        }
        candidates.extend(self.retained_header_keys.iter().cloned());

        let Ok((header, _)) = header::try_decrypt_header(
            &candidates,
            &message.encrypted_header,
            &message.header_nonce,
        ) else {
            return Ok(None);
        };

        let lookup = (header.dh_public.to_bytes(), header.message_number);
        if let Some(mk) = self.skipped_keys.remove(&lookup) {
            let plaintext = aead::decrypt(
                mk.as_bytes(),
                &message.body_nonce,
                &message.ciphertext,
                b"",
            )?;
            Ok(Some(plaintext))
        } else {
            Ok(None)
        }
    }

    /// Try to decrypt the header and determine whether a DH ratchet
    /// step is needed.
    ///
    /// Returns `(header, dh_changed)`. The header keys are tried in
    /// this priority order:
    ///
    /// 1. **Next receiving header key** — if it decrypts, the remote
    ///    party has advanced to a new DH epoch.
    /// 2. **Current receiving header key** — same epoch, may or may not
    ///    have a new DH public key (but if the DH key differs while
    ///    the current header key worked, it means the remote hasn't
    ///    rotated header keys yet — e.g. the very first message).
    /// 3. **Previous receiving header key** — late-arriving message
    ///    from the prior epoch.
    fn try_decrypt_header_and_detect_ratchet(
        &self,
        message: &EncryptedMessage,
    ) -> Result<(MessageHeader, bool)> {
        // 1. Try next header key — indicates a new DH epoch
        if let Ok(header) = header::decrypt_header(
            &self.next_receiving_header_key,
            &message.encrypted_header,
            &message.header_nonce,
        ) {
            return Ok((header, true));
        }

        // 2. Try current header key — same epoch or initial exchange
        if let Ok(header) = header::decrypt_header(
            &self.receiving_header_key,
            &message.encrypted_header,
            &message.header_nonce,
        ) {
            let dh_changed =
                self.dh_ratchet.remote_public_key() != Some(header.dh_public);
            return Ok((header, dh_changed));
        }

        // 3. Try previous header key — out-of-order from prior epoch
        if let Some(ref prev_hk) = self.previous_receiving_header_key {
            if let Ok(header) = header::decrypt_header(
                prev_hk,
                &message.encrypted_header,
                &message.header_nonce,
            ) {
                return Ok((header, false));
            }
        }

        Err(crate::error::CryptoError::HeaderEncryption(
            "no header key could decrypt the header".into(),
        ))
    }

    /// Store a skipped message key for later out-of-order decryption.
    ///
    /// Evicts an arbitrary existing key if the cache exceeds
    /// [`MAX_SKIPPED_KEYS`].
    pub fn store_skipped_key(
        &mut self,
        dh_public: &x25519::PublicKey,
        msg_num: u32,
        key: MessageKey,
    ) {
        if self.skipped_keys.len() >= MAX_SKIPPED_KEYS {
            if let Some(&first_key) = self.skipped_keys.keys().next() {
                self.skipped_keys.remove(&first_key);
            }
        }
        self.skipped_keys
            .insert((dh_public.to_bytes(), msg_num), key);
    }
}

/// Derive a 32-byte key from a shared secret with domain separation.
fn derive_initial_key(shared_secret: &[u8; 32], info: &[u8]) -> Result<[u8; kdf::KEY_SIZE]> {
    let output = kdf::derive_key(shared_secret, b"", info, kdf::KEY_SIZE)?;
    let mut key = [0u8; kdf::KEY_SIZE];
    key.copy_from_slice(&output);
    Ok(key)
}

/// Extract a 32-byte key from a larger buffer at the given index.
fn extract_key(buf: &[u8], index: usize) -> [u8; 32] {
    let offset = index * 32;
    let mut key = [0u8; 32];
    key.copy_from_slice(&buf[offset..offset + 32]);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::KemKeyPair;
    use crate::x25519::KeyPair;

    fn shared_secret() -> [u8; 32] {
        [0x42u8; 32]
    }

    /// Create a sender/receiver session pair.
    fn make_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
        let bob_dh = KeyPair::generate();
        let bob_kem = KemKeyPair::generate();
        let secret = shared_secret();

        let alice = TripleRatchetSession::init_sender(
            &secret,
            bob_dh.public_key(),
            bob_kem.public_key(),
        )
        .expect("init sender failed");

        let bob = TripleRatchetSession::init_receiver(&secret, bob_dh, bob_kem)
            .expect("init receiver failed");

        (alice, bob)
    }

    // ── Basic send/receive ─────────────────────────────────────────

    #[test]
    fn basic_send_receive() {
        let (mut alice, mut bob) = make_session_pair();

        let plaintext = b"Hello, Bob!";
        let msg = alice.encrypt(plaintext).expect("encrypt failed");
        let decrypted = bob.decrypt(&msg).expect("decrypt failed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn multiple_messages_same_direction() {
        let (mut alice, mut bob) = make_session_pair();

        for i in 0..5 {
            let plaintext = format!("message {i}");
            let msg = alice.encrypt(plaintext.as_bytes()).expect("encrypt failed");
            let decrypted = bob.decrypt(&msg).expect("decrypt failed");
            assert_eq!(decrypted, plaintext.as_bytes(), "mismatch at message {i}");
        }
    }

    // ── Bidirectional communication ────────────────────────────────

    #[test]
    fn bidirectional_exchange() {
        let (mut alice, mut bob) = make_session_pair();

        // Alice -> Bob
        let msg1 = alice.encrypt(b"Hello Bob").expect("alice encrypt 1");
        let dec1 = bob.decrypt(&msg1).expect("bob decrypt 1");
        assert_eq!(dec1, b"Hello Bob");

        // Bob -> Alice
        let msg2 = bob.encrypt(b"Hello Alice").expect("bob encrypt 1");
        let dec2 = alice.decrypt(&msg2).expect("alice decrypt 1");
        assert_eq!(dec2, b"Hello Alice");

        // Alice -> Bob again
        let msg3 = alice.encrypt(b"How are you?").expect("alice encrypt 2");
        let dec3 = bob.decrypt(&msg3).expect("bob decrypt 2");
        assert_eq!(dec3, b"How are you?");
    }

    #[test]
    fn many_round_trips() {
        let (mut alice, mut bob) = make_session_pair();

        for round in 0..10 {
            let a_msg = format!("Alice round {round}");
            let encrypted = alice.encrypt(a_msg.as_bytes()).expect("alice encrypt");
            let decrypted = bob.decrypt(&encrypted).expect("bob decrypt");
            assert_eq!(decrypted, a_msg.as_bytes(), "A->B mismatch round {round}");

            let b_msg = format!("Bob round {round}");
            let encrypted = bob.encrypt(b_msg.as_bytes()).expect("bob encrypt");
            let decrypted = alice.decrypt(&encrypted).expect("alice decrypt");
            assert_eq!(decrypted, b_msg.as_bytes(), "B->A mismatch round {round}");
        }
    }

    // ── Out-of-order messages ──────────────────────────────────────

    #[test]
    fn out_of_order_within_same_chain() {
        let (mut alice, mut bob) = make_session_pair();

        // Alice sends 3 messages (all in the same sending chain)
        let msg0 = alice.encrypt(b"message 0").expect("encrypt 0");
        let msg1 = alice.encrypt(b"message 1").expect("encrypt 1");
        let msg2 = alice.encrypt(b"message 2").expect("encrypt 2");

        // Bob receives message 2 first (skips 0 and 1 in the chain)
        let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
        assert_eq!(dec2, b"message 2");

        // Skipped keys should have been cached
        assert_eq!(bob.skipped_key_count(), 2);

        // Now Bob receives message 0 (from skipped cache)
        let dec0 = bob.decrypt(&msg0).expect("decrypt 0");
        assert_eq!(dec0, b"message 0");

        // And message 1
        let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
        assert_eq!(dec1, b"message 1");

        // All skipped keys should be consumed now
        assert_eq!(bob.skipped_key_count(), 0);
    }

    #[test]
    fn out_of_order_reversed_delivery() {
        let (mut alice, mut bob) = make_session_pair();

        // Alice sends 5 messages
        let msgs: Vec<_> = (0..5)
            .map(|i| alice.encrypt(format!("msg {i}").as_bytes()).expect("encrypt"))
            .collect();

        // Bob receives them in reverse order
        for i in (0..5).rev() {
            let dec = bob.decrypt(&msgs[i]).expect("decrypt");
            assert_eq!(dec, format!("msg {i}").as_bytes(), "mismatch at msg {i}");
        }
    }

    #[test]
    fn out_of_order_across_dh_ratchet() {
        let (mut alice, mut bob) = make_session_pair();

        // Alice sends 2 messages in her first sending chain
        let msg0 = alice.encrypt(b"chain1-msg0").expect("encrypt 0");
        let msg1 = alice.encrypt(b"chain1-msg1").expect("encrypt 1");

        // Bob receives only msg1 (skipping msg0)
        let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
        assert_eq!(dec1, b"chain1-msg1");
        assert_eq!(bob.skipped_key_count(), 1); // msg0 is cached

        // Bob replies (triggers DH ratchet on Alice)
        let reply = bob.encrypt(b"reply").expect("bob encrypt");
        let dec_reply = alice.decrypt(&reply).expect("alice decrypt");
        assert_eq!(dec_reply, b"reply");

        // Alice sends in her new chain
        let msg2 = alice.encrypt(b"chain2-msg0").expect("encrypt 2");
        let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
        assert_eq!(dec2, b"chain2-msg0");

        // msg0 from the old chain should still be recoverable
        let dec0 = bob.decrypt(&msg0).expect("decrypt old msg0");
        assert_eq!(dec0, b"chain1-msg0");
        assert_eq!(bob.skipped_key_count(), 0);
    }

    // ── Empty and large messages ───────────────────────────────────

    #[test]
    fn empty_message() {
        let (mut alice, mut bob) = make_session_pair();

        let msg = alice.encrypt(b"").expect("encrypt empty");
        let decrypted = bob.decrypt(&msg).expect("decrypt empty");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn large_message() {
        let (mut alice, mut bob) = make_session_pair();

        let plaintext = vec![0xAB; 64 * 1024]; // 64 KiB
        let msg = alice.encrypt(&plaintext).expect("encrypt large");
        let decrypted = bob.decrypt(&msg).expect("decrypt large");
        assert_eq!(decrypted, plaintext);
    }

    // ── PQ ratchet step trigger ────────────────────────────────────

    #[test]
    fn pq_step_triggers_at_interval() {
        let (mut alice, mut bob) = make_session_pair();
        alice.set_pq_interval(3);

        // Each round trip causes a DH ratchet step on each side.
        // After 3 DH steps on Alice's side, a PQ step should trigger.
        for round in 0..4 {
            let msg = alice
                .encrypt(format!("alice {round}").as_bytes())
                .expect("alice encrypt");
            bob.decrypt(&msg).expect("bob decrypt");

            let msg = bob
                .encrypt(format!("bob {round}").as_bytes())
                .expect("bob encrypt");
            alice.decrypt(&msg).expect("alice decrypt");
        }

        assert!(
            alice.pq_step_count() > 0,
            "PQ ratchet step should have triggered on sender"
        );
    }

    // ── Session initialization ─────────────────────────────────────

    #[test]
    fn init_sender_succeeds() {
        let bob_dh = KeyPair::generate();
        let bob_kem = KemKeyPair::generate();

        let result = TripleRatchetSession::init_sender(
            &shared_secret(),
            bob_dh.public_key(),
            bob_kem.public_key(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn init_receiver_succeeds() {
        let bob_dh = KeyPair::generate();
        let bob_kem = KemKeyPair::generate();

        let result = TripleRatchetSession::init_receiver(&shared_secret(), bob_dh, bob_kem);
        assert!(result.is_ok());
    }

    #[test]
    fn different_shared_secrets_fail_decryption() {
        let bob_dh = KeyPair::generate();
        let bob_kem = KemKeyPair::generate();

        let mut alice = TripleRatchetSession::init_sender(
            &[0x01u8; 32],
            bob_dh.public_key(),
            bob_kem.public_key(),
        )
        .expect("init sender");

        let mut bob = TripleRatchetSession::init_receiver(&[0x02u8; 32], bob_dh, bob_kem)
            .expect("init receiver");

        let msg = alice.encrypt(b"test").expect("encrypt");
        let result = bob.decrypt(&msg);
        assert!(result.is_err(), "different secrets should fail");
    }

    // ── Key uniqueness ─────────────────────────────────────────────

    #[test]
    fn each_message_uses_unique_encryption() {
        let (mut alice, mut bob) = make_session_pair();

        let msg1 = alice.encrypt(b"same content").expect("encrypt 1");
        let msg2 = alice.encrypt(b"same content").expect("encrypt 2");

        // Even with same plaintext, ciphertexts should differ
        assert_ne!(msg1.ciphertext, msg2.ciphertext);

        // Both should still decrypt correctly
        let dec1 = bob.decrypt(&msg1).expect("decrypt 1");
        let dec2 = bob.decrypt(&msg2).expect("decrypt 2");
        assert_eq!(dec1, b"same content");
        assert_eq!(dec2, b"same content");
    }

    // ── Skipped key management ─────────────────────────────────────

    #[test]
    fn skipped_keys_initially_empty() {
        let (alice, _bob) = make_session_pair();
        assert_eq!(alice.skipped_key_count(), 0);
    }

    #[test]
    fn store_and_retrieve_skipped_key() {
        let (mut alice, _bob) = make_session_pair();

        let dh_pub = KeyPair::generate().public_key();
        let mk = crate::symmetric_ratchet::ChainKey::new([0xAA; 32]);
        let mut ratchet = crate::symmetric_ratchet::SymmetricRatchet::new(mk);
        let msg_key = ratchet.advance();

        alice.store_skipped_key(&dh_pub, 5, msg_key);
        assert_eq!(alice.skipped_key_count(), 1);
    }

    #[test]
    fn skipped_key_eviction_at_limit() {
        let (mut alice, _bob) = make_session_pair();

        let dh_pub = KeyPair::generate().public_key();
        let ck = crate::symmetric_ratchet::ChainKey::new([0xBB; 32]);
        let mut ratchet = crate::symmetric_ratchet::SymmetricRatchet::new(ck);

        // Fill up to MAX_SKIPPED_KEYS + 1
        for i in 0..=MAX_SKIPPED_KEYS {
            let mk = ratchet.advance();
            #[allow(clippy::cast_possible_truncation)]
            alice.store_skipped_key(&dh_pub, i as u32, mk);
        }

        assert!(alice.skipped_key_count() <= MAX_SKIPPED_KEYS);
    }

    // ── PQ interval configuration ──────────────────────────────────

    #[test]
    fn set_pq_interval() {
        let (mut alice, _bob) = make_session_pair();
        alice.set_pq_interval(5);
        assert_eq!(alice.pq_step_interval, 5);
    }

    #[test]
    fn pq_disabled_with_zero_interval() {
        let (mut alice, mut bob) = make_session_pair();
        alice.set_pq_interval(0);

        for round in 0..5 {
            let msg = alice
                .encrypt(format!("alice {round}").as_bytes())
                .expect("encrypt");
            bob.decrypt(&msg).expect("decrypt");

            let msg = bob
                .encrypt(format!("bob {round}").as_bytes())
                .expect("encrypt");
            alice.decrypt(&msg).expect("decrypt");
        }

        assert_eq!(alice.pq_step_count(), 0);
    }

    // ── Multiple messages before first reply ───────────────────────

    #[test]
    fn multiple_messages_before_reply() {
        let (mut alice, mut bob) = make_session_pair();

        for i in 0..10 {
            let msg = alice
                .encrypt(format!("msg {i}").as_bytes())
                .expect("encrypt");
            let dec = bob.decrypt(&msg).expect("decrypt");
            assert_eq!(dec, format!("msg {i}").as_bytes());
        }

        let reply = bob.encrypt(b"got them all").expect("encrypt");
        let dec = alice.decrypt(&reply).expect("decrypt");
        assert_eq!(dec, b"got them all");
    }

    // ── Header key rotation ──────────────────────────────────────

    #[test]
    fn previous_header_key_decrypts_after_dh_ratchet() {
        let (mut alice, mut bob) = make_session_pair();

        // Alice sends two messages in the first epoch.
        let msg_epoch1_0 = alice.encrypt(b"epoch1-msg0").expect("encrypt epoch1-0");
        let msg_epoch1_1 = alice.encrypt(b"epoch1-msg1").expect("encrypt epoch1-1");

        // Bob only receives msg1 (skip msg0 for later).
        let dec = bob.decrypt(&msg_epoch1_1).expect("decrypt epoch1-1");
        assert_eq!(dec, b"epoch1-msg1");

        // Bob replies — this triggers a DH ratchet on Alice.
        let reply = bob.encrypt(b"reply").expect("bob encrypt");
        alice.decrypt(&reply).expect("alice decrypt reply");

        // Alice sends a message in the new epoch (new DH key, new header key).
        let msg_epoch2 = alice.encrypt(b"epoch2-msg0").expect("encrypt epoch2-0");
        bob.decrypt(&msg_epoch2).expect("bob decrypt epoch2-0");

        // Now decrypt the delayed msg0 from epoch 1.
        // This requires the previous receiving header key to
        // decrypt the header, then the cached skipped message key.
        let dec0 = bob.decrypt(&msg_epoch1_0).expect("decrypt old epoch1-msg0");
        assert_eq!(dec0, b"epoch1-msg0");
    }

    #[test]
    fn header_decryption_fails_with_unrelated_key() {
        let (mut alice, _bob) = make_session_pair();

        // Alice encrypts a message.
        let msg = alice.encrypt(b"secret").expect("encrypt");

        // Create a completely separate session with a different
        // shared secret.
        let bob_dh = KeyPair::generate();
        let bob_kem = KemKeyPair::generate();
        let mut eve = TripleRatchetSession::init_receiver(
            &[0xFF; 32],
            bob_dh,
            bob_kem,
        )
        .expect("init eve");

        // Eve should not be able to decrypt Alice's message.
        let result = eve.decrypt(&msg);
        assert!(result.is_err(), "unrelated key must fail decryption");
    }
}
