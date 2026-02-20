//! X3DH-inspired key exchange with post-quantum KEM extension.
//!
//! Implements the initial key exchange between two users. The sender
//! fetches the receiver's [`PreKeyBundle`] and performs 3-4 Diffie-Hellman
//! operations plus an ML-KEM encapsulation, then derives a shared secret
//! via HKDF. The receiver mirrors the operations to arrive at the same
//! shared secret.
//!
//! The resulting shared secret can be fed directly into
//! [`TripleRatchetSession`](crate::triple_ratchet::TripleRatchetSession)
//! to establish an encrypted session.

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};
use crate::identity::{IdentityKeyPair, IdentityPublicKey, IDENTITY_PUBLIC_KEY_LEN};
use crate::kem::{self, KemCiphertext};
use crate::kdf;
use crate::prekey::{KemPreKey, OneTimePreKey, PreKeyBundle, SignedPreKey};
use crate::x25519;

/// HKDF salt for the final shared secret derivation.
const HKDF_SALT: [u8; 32] = [0u8; 32];

/// HKDF info string for the final shared secret derivation.
const HKDF_INFO: &[u8] = b"pirc-x3dh-shared-secret";

/// Output of the sender side of the X3DH key exchange.
///
/// Contains the derived shared secret, the ephemeral public key sent
/// to the receiver, the KEM ciphertext, and which one-time pre-key
/// (if any) was consumed.
pub struct X3DHSenderResult {
    shared_secret: [u8; 32],
    ephemeral_public: x25519::PublicKey,
    kem_ciphertext: KemCiphertext,
    used_one_time_pre_key_id: Option<u32>,
}

impl Zeroize for X3DHSenderResult {
    fn zeroize(&mut self) {
        self.shared_secret.zeroize();
    }
}

impl Drop for X3DHSenderResult {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl X3DHSenderResult {
    /// Return the 32-byte shared secret.
    #[must_use]
    pub fn shared_secret(&self) -> &[u8; 32] {
        &self.shared_secret
    }

    /// Return the sender's ephemeral X25519 public key.
    #[must_use]
    pub fn ephemeral_public(&self) -> x25519::PublicKey {
        self.ephemeral_public
    }

    /// Return a reference to the KEM ciphertext.
    #[must_use]
    pub fn kem_ciphertext(&self) -> &KemCiphertext {
        &self.kem_ciphertext
    }

    /// Return the ID of the one-time pre-key used, if any.
    #[must_use]
    pub fn used_one_time_pre_key_id(&self) -> Option<u32> {
        self.used_one_time_pre_key_id
    }
}

impl std::fmt::Debug for X3DHSenderResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X3DHSenderResult")
            .field("shared_secret", &"[REDACTED]")
            .field("ephemeral_public", &self.ephemeral_public)
            .field("kem_ciphertext", &self.kem_ciphertext)
            .field("used_one_time_pre_key_id", &self.used_one_time_pre_key_id)
            .finish()
    }
}

/// Output of the receiver side of the X3DH key exchange.
///
/// Contains only the derived shared secret. The receiver already
/// knows its own keys so no additional data is needed.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct X3DHReceiverResult {
    shared_secret: [u8; 32],
}

impl X3DHReceiverResult {
    /// Return the 32-byte shared secret.
    #[must_use]
    pub fn shared_secret(&self) -> &[u8; 32] {
        &self.shared_secret
    }
}

impl std::fmt::Debug for X3DHReceiverResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X3DHReceiverResult")
            .field("shared_secret", &"[REDACTED]")
            .finish()
    }
}

/// Message sent from the initiator to the receiver to complete the exchange.
///
/// Contains everything the receiver needs to perform their half of the
/// protocol: the sender's identity, the ephemeral DH public key, the
/// KEM ciphertext, and which pre-keys were used.
pub struct X3DHInitMessage {
    sender_identity: IdentityPublicKey,
    ephemeral_public: x25519::PublicKey,
    kem_ciphertext: KemCiphertext,
    used_one_time_pre_key_id: Option<u32>,
    used_signed_pre_key_id: u32,
    used_kem_pre_key_id: u32,
}

impl X3DHInitMessage {
    /// Return a reference to the sender's identity public key.
    #[must_use]
    pub fn sender_identity(&self) -> &IdentityPublicKey {
        &self.sender_identity
    }

    /// Return the sender's ephemeral X25519 public key.
    #[must_use]
    pub fn ephemeral_public(&self) -> x25519::PublicKey {
        self.ephemeral_public
    }

    /// Return a reference to the KEM ciphertext.
    #[must_use]
    pub fn kem_ciphertext(&self) -> &KemCiphertext {
        &self.kem_ciphertext
    }

    /// Return the ID of the one-time pre-key used, if any.
    #[must_use]
    pub fn used_one_time_pre_key_id(&self) -> Option<u32> {
        self.used_one_time_pre_key_id
    }

    /// Return the ID of the signed pre-key used.
    #[must_use]
    pub fn used_signed_pre_key_id(&self) -> u32 {
        self.used_signed_pre_key_id
    }

    /// Return the ID of the KEM pre-key used.
    #[must_use]
    pub fn used_kem_pre_key_id(&self) -> u32 {
        self.used_kem_pre_key_id
    }

    /// Serialize to a byte vector.
    ///
    /// Format:
    /// ```text
    /// [identity_public | ephemeral_public (32) | kem_ciphertext_len (4 LE)
    ///  | kem_ciphertext | has_otpk (1) | otpk_id (4 LE)? | spk_id (4 LE) | kem_pk_id (4 LE)]
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let identity_bytes = self.sender_identity.to_bytes();
        let kem_ct_bytes = self.kem_ciphertext.to_bytes();

        // Pre-allocate: identity + ephemeral(32) + ct_len(4) + ct + flag(1) + otpk(0|4) + spk(4) + kem_pk(4)
        let capacity = identity_bytes.len() + 32 + 4 + kem_ct_bytes.len() + 1 + 4 + 4 + 4;
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(&identity_bytes);
        bytes.extend_from_slice(self.ephemeral_public.as_bytes());

        #[allow(clippy::cast_possible_truncation)]
        let kem_ct_len = kem_ct_bytes.len() as u32; // KEM ciphertext is always 1088 bytes
        bytes.extend_from_slice(&kem_ct_len.to_le_bytes());
        bytes.extend_from_slice(&kem_ct_bytes);

        if let Some(otpk_id) = self.used_one_time_pre_key_id {
            bytes.push(1);
            bytes.extend_from_slice(&otpk_id.to_le_bytes());
        } else {
            bytes.push(0);
        }

        bytes.extend_from_slice(&self.used_signed_pre_key_id.to_le_bytes());
        bytes.extend_from_slice(&self.used_kem_pre_key_id.to_le_bytes());

        bytes
    }

    /// Deserialize from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Serialization`] if the data is malformed
    /// or too short.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Minimum: identity + ephemeral(32) + ct_len(4) + has_otpk(1) + spk_id(4) + kem_pk_id(4)
        let min_len = IDENTITY_PUBLIC_KEY_LEN + 32 + 4 + 1 + 4 + 4;
        if bytes.len() < min_len {
            return Err(CryptoError::Serialization(format!(
                "X3DHInitMessage: expected at least {min_len} bytes, got {}",
                bytes.len()
            )));
        }

        let mut offset = 0;

        let sender_identity =
            IdentityPublicKey::from_bytes(&bytes[offset..offset + IDENTITY_PUBLIC_KEY_LEN])?;
        offset += IDENTITY_PUBLIC_KEY_LEN;

        let mut eph_bytes = [0u8; 32];
        eph_bytes.copy_from_slice(&bytes[offset..offset + 32]);
        let ephemeral_public = x25519::PublicKey::from_bytes(eph_bytes);
        offset += 32;

        let kem_ct_len =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if bytes.len() < offset + kem_ct_len {
            return Err(CryptoError::Serialization(format!(
                "X3DHInitMessage: KEM ciphertext truncated (need {kem_ct_len}, have {})",
                bytes.len() - offset
            )));
        }
        let kem_ciphertext = KemCiphertext::from_bytes(&bytes[offset..offset + kem_ct_len])?;
        offset += kem_ct_len;

        if bytes.len() < offset + 1 {
            return Err(CryptoError::Serialization(
                "X3DHInitMessage: missing OTPK flag".into(),
            ));
        }
        let has_otpk = bytes[offset];
        offset += 1;

        let used_one_time_pre_key_id = if has_otpk == 1 {
            if bytes.len() < offset + 4 {
                return Err(CryptoError::Serialization(
                    "X3DHInitMessage: missing OTPK id".into(),
                ));
            }
            let id = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            offset += 4;
            Some(id)
        } else {
            None
        };

        if bytes.len() < offset + 8 {
            return Err(CryptoError::Serialization(
                "X3DHInitMessage: missing SPK/KEM pre-key ids".into(),
            ));
        }
        let used_signed_pre_key_id =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let used_kem_pre_key_id =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());

        Ok(Self {
            sender_identity,
            ephemeral_public,
            kem_ciphertext,
            used_one_time_pre_key_id,
            used_signed_pre_key_id,
            used_kem_pre_key_id,
        })
    }
}

impl std::fmt::Debug for X3DHInitMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X3DHInitMessage")
            .field("sender_identity", &self.sender_identity)
            .field("ephemeral_public", &self.ephemeral_public)
            .field("kem_ciphertext", &self.kem_ciphertext)
            .field("used_one_time_pre_key_id", &self.used_one_time_pre_key_id)
            .field("used_signed_pre_key_id", &self.used_signed_pre_key_id)
            .field("used_kem_pre_key_id", &self.used_kem_pre_key_id)
            .finish()
    }
}

/// Perform the sender half of the X3DH key exchange.
///
/// Validates the receiver's pre-key bundle, generates an ephemeral key
/// pair, performs 3-4 DH operations plus ML-KEM encapsulation, and
/// derives the shared secret via HKDF.
///
/// # Arguments
///
/// * `sender_identity` — the sender's long-term identity key pair
/// * `receiver_bundle` — the receiver's published pre-key bundle
///
/// # Errors
///
/// Returns [`CryptoError::Signature`] if the bundle signatures are
/// invalid, or [`CryptoError::KeyExchange`] / [`CryptoError::Kem`]
/// if a DH or KEM operation fails.
pub fn x3dh_sender(
    sender_identity: &IdentityKeyPair,
    receiver_bundle: &PreKeyBundle,
) -> Result<(X3DHSenderResult, X3DHInitMessage)> {
    // Validate all signatures in the bundle
    receiver_bundle.validate()?;

    // Generate ephemeral X25519 key pair
    let ephemeral = x25519::KeyPair::generate();

    // DH1 = DH(IK_A, SPK_B) — sender identity × receiver signed pre-key
    let dh1 = x25519::diffie_hellman_keypair(
        sender_identity.dh_key_pair(),
        &receiver_bundle.signed_pre_key().public_key(),
    )?;

    // DH2 = DH(EK_A, IK_B) — sender ephemeral × receiver identity
    let dh2 = x25519::diffie_hellman_keypair(
        &ephemeral,
        &receiver_bundle.identity_public().dh_public_key(),
    )?;

    // DH3 = DH(EK_A, SPK_B) — sender ephemeral × receiver signed pre-key
    let dh3 = x25519::diffie_hellman_keypair(
        &ephemeral,
        &receiver_bundle.signed_pre_key().public_key(),
    )?;

    // DH4 = DH(EK_A, OPK_B) — sender ephemeral × receiver one-time pre-key (optional)
    let dh4 = if let Some(otpk) = receiver_bundle.one_time_pre_key() {
        Some(x25519::diffie_hellman_keypair(&ephemeral, &otpk.public_key())?)
    } else {
        None
    };

    // KEM encapsulation against receiver's KEM pre-key
    let (kem_ciphertext, kem_ss) =
        kem::encapsulate(&receiver_bundle.kem_pre_key().public_key())?;

    // Build IKM = DH1 || DH2 || DH3 || [DH4] || KEM_SS on the stack
    // Max size: 4 DH secrets (32 each) + 1 KEM secret (32) = 160 bytes
    let mut ikm = [0u8; 160];
    let mut offset = 0;
    ikm[offset..offset + 32].copy_from_slice(dh1.as_bytes());
    offset += 32;
    ikm[offset..offset + 32].copy_from_slice(dh2.as_bytes());
    offset += 32;
    ikm[offset..offset + 32].copy_from_slice(dh3.as_bytes());
    offset += 32;
    if let Some(ref dh4_ss) = dh4 {
        ikm[offset..offset + 32].copy_from_slice(dh4_ss.as_bytes());
        offset += 32;
    }
    ikm[offset..offset + kem::SHARED_SECRET_LEN].copy_from_slice(kem_ss.as_bytes());
    offset += kem::SHARED_SECRET_LEN;
    let ikm_slice = &ikm[..offset];

    // Derive shared secret (zero-allocation)
    let mut shared_secret = [0u8; 32];
    kdf::derive_key_into(&HKDF_SALT, ikm_slice, HKDF_INFO, &mut shared_secret)?;

    // Zeroize IKM
    ikm.zeroize();

    let used_otpk_id = receiver_bundle
        .one_time_pre_key()
        .map(crate::prekey::OneTimePreKeyPublic::id);

    let result = X3DHSenderResult {
        shared_secret,
        ephemeral_public: ephemeral.public_key(),
        kem_ciphertext: kem_ciphertext.clone(),
        used_one_time_pre_key_id: used_otpk_id,
    };

    let init_message = X3DHInitMessage {
        sender_identity: sender_identity.public_identity(),
        ephemeral_public: ephemeral.public_key(),
        kem_ciphertext,
        used_one_time_pre_key_id: used_otpk_id,
        used_signed_pre_key_id: receiver_bundle.signed_pre_key().id(),
        used_kem_pre_key_id: receiver_bundle.kem_pre_key().id(),
    };

    Ok((result, init_message))
}

/// Perform the receiver half of the X3DH key exchange.
///
/// Mirrors the sender's DH operations and decapsulates the KEM
/// ciphertext to derive the same shared secret.
///
/// # Arguments
///
/// * `receiver_identity` — the receiver's long-term identity key pair
/// * `signed_pre_key` — the signed pre-key whose ID matches the init message
/// * `kem_pre_key` — the KEM pre-key whose ID matches the init message
/// * `one_time_pre_key` — the one-time pre-key if one was used
/// * `init_message` — the [`X3DHInitMessage`] from the sender
///
/// # Errors
///
/// Returns [`CryptoError::KeyExchange`] / [`CryptoError::Kem`] if a
/// DH or KEM operation fails.
pub fn x3dh_receiver(
    receiver_identity: &IdentityKeyPair,
    signed_pre_key: &SignedPreKey,
    kem_pre_key: &KemPreKey,
    one_time_pre_key: Option<&OneTimePreKey>,
    init_message: &X3DHInitMessage,
) -> Result<X3DHReceiverResult> {
    // DH1 = DH(SPK_B, IK_A) — receiver signed pre-key × sender identity
    let dh1 = x25519::diffie_hellman_keypair(
        signed_pre_key.key_pair(),
        &init_message.sender_identity.dh_public_key(),
    )?;

    // DH2 = DH(IK_B, EK_A) — receiver identity × sender ephemeral
    let dh2 = x25519::diffie_hellman_keypair(
        receiver_identity.dh_key_pair(),
        &init_message.ephemeral_public,
    )?;

    // DH3 = DH(SPK_B, EK_A) — receiver signed pre-key × sender ephemeral
    let dh3 = x25519::diffie_hellman_keypair(
        signed_pre_key.key_pair(),
        &init_message.ephemeral_public,
    )?;

    // DH4 = DH(OPK_B, EK_A) — receiver one-time pre-key × sender ephemeral (optional)
    let dh4 = if let Some(otpk) = one_time_pre_key {
        Some(x25519::diffie_hellman_keypair(
            otpk.key_pair(),
            &init_message.ephemeral_public,
        )?)
    } else {
        None
    };

    // KEM decapsulation
    let kem_ss = kem::decapsulate(kem_pre_key.kem_pair(), &init_message.kem_ciphertext)?;

    // Build IKM = DH1 || DH2 || DH3 || [DH4] || KEM_SS on the stack
    let mut ikm = [0u8; 160];
    let mut offset = 0;
    ikm[offset..offset + 32].copy_from_slice(dh1.as_bytes());
    offset += 32;
    ikm[offset..offset + 32].copy_from_slice(dh2.as_bytes());
    offset += 32;
    ikm[offset..offset + 32].copy_from_slice(dh3.as_bytes());
    offset += 32;
    if let Some(ref dh4_ss) = dh4 {
        ikm[offset..offset + 32].copy_from_slice(dh4_ss.as_bytes());
        offset += 32;
    }
    ikm[offset..offset + kem::SHARED_SECRET_LEN].copy_from_slice(kem_ss.as_bytes());
    offset += kem::SHARED_SECRET_LEN;
    let ikm_slice = &ikm[..offset];

    // Derive shared secret (zero-allocation)
    let mut shared_secret = [0u8; 32];
    kdf::derive_key_into(&HKDF_SALT, ikm_slice, HKDF_INFO, &mut shared_secret)?;

    // Zeroize IKM
    ikm.zeroize();

    Ok(X3DHReceiverResult { shared_secret })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::KemKeyPair;
    use crate::prekey::OneTimePreKey;
    use crate::triple_ratchet::TripleRatchetSession;

    // ── Helpers ──────────────────────────────────────────────────────

    /// Create a full set of keys and pre-key bundle for a receiver.
    fn make_receiver_keys(
        with_otpk: bool,
    ) -> (
        IdentityKeyPair,
        SignedPreKey,
        KemPreKey,
        Option<OneTimePreKey>,
        PreKeyBundle,
    ) {
        let identity = IdentityKeyPair::generate();
        let spk = SignedPreKey::generate(1, &identity, 1_700_000_000).expect("spk failed");
        let kpk = KemPreKey::generate(1, &identity).expect("kpk failed");
        let otpk = if with_otpk {
            Some(OneTimePreKey::generate(1))
        } else {
            None
        };

        let bundle = PreKeyBundle::new(
            identity.public_identity(),
            spk.to_public(),
            kpk.to_public(),
            otpk.as_ref().map(OneTimePreKey::to_public),
        );

        (identity, spk, kpk, otpk, bundle)
    }

    // ── Key exchange basics ─────────────────────────────────────────

    #[test]
    fn sender_and_receiver_derive_same_secret_with_otpk() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, bob_otpk, bob_bundle) = make_receiver_keys(true);

        let (sender_result, init_msg) =
            x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        let receiver_result = x3dh_receiver(
            &bob_identity,
            &bob_spk,
            &bob_kpk,
            bob_otpk.as_ref(),
            &init_msg,
        )
        .expect("receiver failed");

        assert_eq!(
            sender_result.shared_secret(),
            receiver_result.shared_secret(),
            "shared secrets must match"
        );
    }

    #[test]
    fn sender_and_receiver_derive_same_secret_without_otpk() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, _, bob_bundle) = make_receiver_keys(false);

        let (sender_result, init_msg) =
            x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        let receiver_result =
            x3dh_receiver(&bob_identity, &bob_spk, &bob_kpk, None, &init_msg)
                .expect("receiver failed");

        assert_eq!(
            sender_result.shared_secret(),
            receiver_result.shared_secret(),
            "shared secrets must match without OTPK"
        );
    }

    #[test]
    fn with_and_without_otpk_produce_different_secrets() {
        let alice_identity = IdentityKeyPair::generate();
        let bob_identity = IdentityKeyPair::generate();
        let bob_spk = SignedPreKey::generate(1, &bob_identity, 0).expect("spk failed");
        let bob_kpk = KemPreKey::generate(1, &bob_identity).expect("kpk failed");
        let bob_otpk = OneTimePreKey::generate(1);

        let bundle_with = PreKeyBundle::new(
            bob_identity.public_identity(),
            bob_spk.to_public(),
            bob_kpk.to_public(),
            Some(bob_otpk.to_public()),
        );
        let bundle_without = PreKeyBundle::new(
            bob_identity.public_identity(),
            bob_spk.to_public(),
            bob_kpk.to_public(),
            None,
        );

        let (result_with, _) =
            x3dh_sender(&alice_identity, &bundle_with).expect("sender with otpk failed");
        let (result_without, _) =
            x3dh_sender(&alice_identity, &bundle_without).expect("sender without otpk failed");

        // Different because ephemeral keys are random, but also the DH4 term
        // is included/excluded, so the IKMs differ structurally
        assert_ne!(
            result_with.shared_secret(),
            result_without.shared_secret(),
            "secrets should differ with/without OTPK (different ephemeral keys)"
        );
    }

    // ── Bundle validation ───────────────────────────────────────────

    #[test]
    fn sender_rejects_invalid_bundle_signature() {
        let alice_identity = IdentityKeyPair::generate();
        let bob_identity = IdentityKeyPair::generate();
        let wrong_identity = IdentityKeyPair::generate();

        // Sign SPK with wrong identity
        let bad_spk = SignedPreKey::generate(1, &wrong_identity, 0).expect("spk failed");
        let good_kpk = KemPreKey::generate(1, &bob_identity).expect("kpk failed");

        let bad_bundle = PreKeyBundle::new(
            bob_identity.public_identity(),
            bad_spk.to_public(),
            good_kpk.to_public(),
            None,
        );

        let result = x3dh_sender(&alice_identity, &bad_bundle);
        assert!(result.is_err(), "should reject tampered SPK signature");
    }

    #[test]
    fn sender_rejects_invalid_kem_pre_key_signature() {
        let alice_identity = IdentityKeyPair::generate();
        let bob_identity = IdentityKeyPair::generate();
        let wrong_identity = IdentityKeyPair::generate();

        let good_spk = SignedPreKey::generate(1, &bob_identity, 0).expect("spk failed");
        let bad_kpk = KemPreKey::generate(1, &wrong_identity).expect("kpk failed");

        let bad_bundle = PreKeyBundle::new(
            bob_identity.public_identity(),
            good_spk.to_public(),
            bad_kpk.to_public(),
            None,
        );

        let result = x3dh_sender(&alice_identity, &bad_bundle);
        assert!(result.is_err(), "should reject tampered KEM signature");
    }

    // ── Wrong keys produce different secrets ────────────────────────

    #[test]
    fn wrong_receiver_identity_produces_different_secret() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, bob_spk, bob_kpk, _, bob_bundle) = make_receiver_keys(false);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        // Receiver uses a different identity key pair
        let wrong_identity = IdentityKeyPair::generate();
        let receiver_result =
            x3dh_receiver(&wrong_identity, &bob_spk, &bob_kpk, None, &init_msg)
                .expect("receiver should still compute a secret");

        // The DH2 term will differ, so the shared secrets won't match
        let (sender_result, _) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        assert_ne!(
            sender_result.shared_secret(),
            receiver_result.shared_secret(),
            "wrong identity should produce different secret"
        );
    }

    // ── Init message serialization ──────────────────────────────────

    #[test]
    fn init_message_roundtrip_with_otpk() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(true);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        let bytes = init_msg.to_bytes();
        let restored = X3DHInitMessage::from_bytes(&bytes).expect("deserialize failed");

        assert_eq!(
            init_msg.sender_identity(),
            restored.sender_identity()
        );
        assert_eq!(
            init_msg.ephemeral_public().to_bytes(),
            restored.ephemeral_public().to_bytes()
        );
        assert_eq!(
            init_msg.used_one_time_pre_key_id(),
            restored.used_one_time_pre_key_id()
        );
        assert_eq!(
            init_msg.used_signed_pre_key_id(),
            restored.used_signed_pre_key_id()
        );
        assert_eq!(
            init_msg.used_kem_pre_key_id(),
            restored.used_kem_pre_key_id()
        );
    }

    #[test]
    fn init_message_roundtrip_without_otpk() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(false);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        assert!(init_msg.used_one_time_pre_key_id().is_none());

        let bytes = init_msg.to_bytes();
        let restored = X3DHInitMessage::from_bytes(&bytes).expect("deserialize failed");

        assert!(restored.used_one_time_pre_key_id().is_none());
        assert_eq!(
            init_msg.used_signed_pre_key_id(),
            restored.used_signed_pre_key_id()
        );
    }

    #[test]
    fn init_message_from_bytes_too_short_fails() {
        let result = X3DHInitMessage::from_bytes(&[0u8; 10]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected"));
    }

    // ── Deserialized init message still works for key exchange ───────

    #[test]
    fn roundtripped_init_message_produces_same_secret() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, bob_otpk, bob_bundle) = make_receiver_keys(true);

        let (sender_result, init_msg) =
            x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        // Serialize and deserialize the init message
        let bytes = init_msg.to_bytes();
        let restored_msg = X3DHInitMessage::from_bytes(&bytes).expect("deserialize failed");

        let receiver_result = x3dh_receiver(
            &bob_identity,
            &bob_spk,
            &bob_kpk,
            bob_otpk.as_ref(),
            &restored_msg,
        )
        .expect("receiver failed");

        assert_eq!(
            sender_result.shared_secret(),
            receiver_result.shared_secret(),
            "roundtripped init message should produce matching secrets"
        );
    }

    // ── Integration with TripleRatchetSession ───────────────────────

    #[test]
    fn shared_secret_initializes_triple_ratchet_session() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, bob_otpk, bob_bundle) = make_receiver_keys(true);

        let (sender_result, init_msg) =
            x3dh_sender(&alice_identity, &bob_bundle).expect("x3dh sender failed");

        let receiver_result = x3dh_receiver(
            &bob_identity,
            &bob_spk,
            &bob_kpk,
            bob_otpk.as_ref(),
            &init_msg,
        )
        .expect("x3dh receiver failed");

        assert_eq!(sender_result.shared_secret(), receiver_result.shared_secret());

        // Initialize Triple Ratchet sessions with the shared secret.
        // Use fresh DH and KEM key pairs for the ratchet (separate from the
        // pre-key exchange keys).
        let bob_dh = x25519::KeyPair::generate();
        let bob_kem = KemKeyPair::generate();

        let mut alice_session = TripleRatchetSession::init_sender(
            sender_result.shared_secret(),
            bob_dh.public_key(),
            bob_kem.public_key(),
        )
        .expect("alice session init failed");

        let mut bob_session = TripleRatchetSession::init_receiver(
            receiver_result.shared_secret(),
            bob_dh,
            bob_kem,
        )
        .expect("bob session init failed");

        // Alice encrypts, Bob decrypts
        let plaintext = b"Hello from Alice via X3DH!";
        let encrypted = alice_session.encrypt(plaintext).expect("encrypt failed");
        let decrypted = bob_session.decrypt(&encrypted).expect("decrypt failed");

        assert_eq!(decrypted, plaintext);

        // Bob encrypts, Alice decrypts
        let reply = b"Hello back from Bob!";
        let encrypted_reply = bob_session.encrypt(reply).expect("encrypt reply failed");
        let decrypted_reply = alice_session
            .decrypt(&encrypted_reply)
            .expect("decrypt reply failed");

        assert_eq!(decrypted_reply, reply);
    }

    // ── Result accessors and debug ──────────────────────────────────

    #[test]
    fn sender_result_accessors() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(true);

        let (result, _) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

        assert!(result.shared_secret().iter().any(|&b| b != 0));
        assert!(result.ephemeral_public().as_bytes().iter().any(|&b| b != 0));
        assert!(result.used_one_time_pre_key_id().is_some());
    }

    #[test]
    fn sender_result_debug_redacts_secret() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(false);

        let (result, _) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        let debug = format!("{result:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("X3DHSenderResult"));
    }

    #[test]
    fn receiver_result_debug_redacts_secret() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, _, bob_bundle) = make_receiver_keys(false);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        let result =
            x3dh_receiver(&bob_identity, &bob_spk, &bob_kpk, None, &init_msg)
                .expect("receiver failed");

        let debug = format!("{result:?}");
        assert!(debug.contains("REDACTED"));
        assert!(debug.contains("X3DHReceiverResult"));
    }

    #[test]
    fn init_message_debug_output() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(false);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        let debug = format!("{init_msg:?}");
        assert!(debug.contains("X3DHInitMessage"));
        assert!(debug.contains("used_signed_pre_key_id"));
    }

    // ── Zeroize on drop ─────────────────────────────────────────────

    #[test]
    fn sender_result_secret_is_nonzero_before_drop() {
        let alice_identity = IdentityKeyPair::generate();
        let (_, _, _, _, bob_bundle) = make_receiver_keys(false);

        let (result, _) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        let secret_copy = *result.shared_secret();
        assert!(secret_copy.iter().any(|&b| b != 0));
        drop(result);
    }

    #[test]
    fn receiver_result_secret_is_nonzero_before_drop() {
        let alice_identity = IdentityKeyPair::generate();
        let (bob_identity, bob_spk, bob_kpk, _, bob_bundle) = make_receiver_keys(false);

        let (_, init_msg) = x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");
        let result =
            x3dh_receiver(&bob_identity, &bob_spk, &bob_kpk, None, &init_msg)
                .expect("receiver failed");
        let secret_copy = *result.shared_secret();
        assert!(secret_copy.iter().any(|&b| b != 0));
        drop(result);
    }
}
