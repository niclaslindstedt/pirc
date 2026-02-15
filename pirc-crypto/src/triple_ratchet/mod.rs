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
    /// Value is (message key, DH step at which the key was stored).
    skipped_keys: HashMap<([u8; 32], u32), SkippedKey>,
    /// Number of DH ratchet steps between PQ ratchet steps.
    pq_step_interval: u32,
    /// Counter tracking DH ratchet steps since the last PQ step.
    dh_steps_since_pq: u32,
    /// Number of messages sent in the previous sending chain
    /// (communicated in the header for out-of-order handling).
    previous_chain_length: u32,
    /// Number of messages sent in the current sending chain.
    sending_message_number: u32,
    /// Total number of messages sent across all chains.
    total_messages_sent: u64,
    /// Total number of messages received (successfully decrypted).
    total_messages_received: u64,
}

/// A skipped message key together with the DH step at which it was cached.
///
/// Storing the DH step allows age-based eviction of old skipped keys via
/// [`TripleRatchetSession::purge_skipped_keys_older_than`].
struct SkippedKey {
    key: MessageKey,
    dh_step: u32,
}

/// Session state information for monitoring and diagnostics.
///
/// Returned by [`TripleRatchetSession::session_info`].
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Number of DH ratchet steps completed.
    pub dh_step_count: u32,
    /// Number of PQ ratchet steps completed.
    pub pq_step_count: u32,
    /// Total messages sent across all chains.
    pub messages_sent: u64,
    /// Total messages received (successfully decrypted).
    pub messages_received: u64,
    /// Number of cached skipped message keys.
    pub skipped_key_count: usize,
    /// Current DH public key fingerprint (first 8 bytes as hex).
    pub dh_public_fingerprint: [u8; 32],
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
            total_messages_sent: 0,
            total_messages_received: 0,
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
            total_messages_sent: 0,
            total_messages_received: 0,
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
        // message_key is dropped here — zeroized via ZeroizeOnDrop

        self.total_messages_sent += 1;

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
        // message_key is dropped here — zeroized via ZeroizeOnDrop

        self.total_messages_received += 1;

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
        if let Some(skipped) = self.skipped_keys.remove(&lookup) {
            let plaintext = aead::decrypt(
                skipped.key.as_bytes(),
                &message.body_nonce,
                &message.ciphertext,
                b"",
            )?;
            // skipped.key is dropped here — zeroized via ZeroizeOnDrop
            self.total_messages_received += 1;
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
        let dh_step = self.dh_ratchet.step_count();
        self.skipped_keys
            .insert((dh_public.to_bytes(), msg_num), SkippedKey { key, dh_step });
    }

    /// Remove skipped keys that are from DH epochs older than `max_age`
    /// ratchet steps ago.
    ///
    /// For example, if the current DH step count is 10 and `max_age` is 3,
    /// all skipped keys stored at DH step 7 or earlier are purged. The
    /// removed keys are zeroized via [`ZeroizeOnDrop`].
    pub fn purge_skipped_keys_older_than(&mut self, max_age: u32) {
        let current = self.dh_ratchet.step_count();
        self.skipped_keys.retain(|_, sk| {
            current.saturating_sub(sk.dh_step) <= max_age
        });
    }

    /// Return session state information for monitoring.
    ///
    /// Reports DH/PQ ratchet counts, message counters, skipped key
    /// cache size, and the current DH public key fingerprint.
    #[must_use]
    pub fn session_info(&self) -> SessionInfo {
        SessionInfo {
            dh_step_count: self.dh_ratchet.step_count(),
            pq_step_count: self.pq_ratchet.step_counter(),
            messages_sent: self.total_messages_sent,
            messages_received: self.total_messages_received,
            skipped_key_count: self.skipped_keys.len(),
            dh_public_fingerprint: self.dh_ratchet.public_key().to_bytes(),
        }
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
mod tests;
