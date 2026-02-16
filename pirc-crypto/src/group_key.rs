//! Group key management using pairwise encryption.
//!
//! Each group member maintains a [`TripleRatchetSession`] with every other
//! member. Group messages are encrypted separately for each recipient using
//! their pairwise session. This avoids the complexity of a shared group key
//! while leveraging the existing triple ratchet implementation.
//!
//! The [`GroupKeyManager`] tracks per-member encryption state and provides
//! [`encrypt_for_group`](GroupKeyManager::encrypt_for_group) and
//! [`decrypt_from_member`](GroupKeyManager::decrypt_from_member) operations.

use std::collections::HashMap;

use pirc_common::types::GroupId;

use crate::error::{CryptoError, Result};
use crate::message::EncryptedMessage;
use crate::triple_ratchet::TripleRatchetSession;

/// Encryption readiness state for a single group member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEncryptionState {
    /// Member is known but no key exchange has been initiated.
    Pending,
    /// Key exchange (X3DH) is in progress with this member.
    Establishing,
    /// Pairwise session is active and ready for encryption.
    Ready,
}

/// Manages pairwise encryption sessions for a group chat.
///
/// Each group member has a separate [`TripleRatchetSession`]. When encrypting
/// a group message, the plaintext is encrypted individually for each member
/// that has a [`GroupEncryptionState::Ready`] session. When decrypting, the
/// pairwise session with the sender is used.
pub struct GroupKeyManager {
    /// The group this manager is responsible for.
    group_id: GroupId,
    /// Active pairwise sessions, keyed by member nickname.
    sessions: HashMap<String, TripleRatchetSession>,
    /// Encryption state for each known member.
    member_states: HashMap<String, GroupEncryptionState>,
}

impl GroupKeyManager {
    /// Create a new group key manager for the given group.
    #[must_use]
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            sessions: HashMap::new(),
            member_states: HashMap::new(),
        }
    }

    /// Return the group ID this manager is responsible for.
    #[must_use]
    pub fn group_id(&self) -> GroupId {
        self.group_id
    }

    /// Add a member to the group in [`GroupEncryptionState::Pending`] state.
    ///
    /// If the member already exists, their state is unchanged.
    pub fn add_member(&mut self, nickname: &str) {
        self.member_states
            .entry(nickname.to_owned())
            .or_insert(GroupEncryptionState::Pending);
    }

    /// Mark a member's key exchange as in progress.
    ///
    /// Transitions from any state to [`GroupEncryptionState::Establishing`].
    pub fn set_establishing(&mut self, nickname: &str) {
        self.member_states
            .insert(nickname.to_owned(), GroupEncryptionState::Establishing);
    }

    /// Register an established pairwise session for a member.
    ///
    /// Transitions the member to [`GroupEncryptionState::Ready`].
    pub fn set_session(&mut self, nickname: &str, session: TripleRatchetSession) {
        self.sessions.insert(nickname.to_owned(), session);
        self.member_states
            .insert(nickname.to_owned(), GroupEncryptionState::Ready);
    }

    /// Remove a member from the group.
    ///
    /// Discards the pairwise session and encryption state. The triple
    /// ratchet's forward secrecy ensures the removed member cannot decrypt
    /// future messages even if they retained old key material.
    pub fn remove_member(&mut self, nickname: &str) {
        self.sessions.remove(nickname);
        self.member_states.remove(nickname);
    }

    /// Return the encryption state for a member, or `None` if unknown.
    #[must_use]
    pub fn member_state(&self, nickname: &str) -> Option<GroupEncryptionState> {
        self.member_states.get(nickname).copied()
    }

    /// Return all members and their encryption states.
    #[must_use]
    pub fn members(&self) -> Vec<(String, GroupEncryptionState)> {
        let mut result: Vec<_> = self
            .member_states
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Return the number of members with [`GroupEncryptionState::Ready`]
    /// sessions.
    #[must_use]
    pub fn ready_count(&self) -> usize {
        self.member_states
            .values()
            .filter(|s| **s == GroupEncryptionState::Ready)
            .count()
    }

    /// Return `true` if all known members have ready sessions.
    #[must_use]
    pub fn all_ready(&self) -> bool {
        !self.member_states.is_empty()
            && self
                .member_states
                .values()
                .all(|s| *s == GroupEncryptionState::Ready)
    }

    /// Check whether a pairwise session exists with a member.
    #[must_use]
    pub fn has_session(&self, nickname: &str) -> bool {
        self.sessions.contains_key(nickname)
    }

    /// Encrypt a plaintext message for all ready group members.
    ///
    /// Returns a map from member nickname to their individually encrypted
    /// message. Only members in [`GroupEncryptionState::Ready`] state are
    /// included.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption fails for any ready member. In that
    /// case, the entire operation fails and no partial results are returned.
    pub fn encrypt_for_group(
        &mut self,
        plaintext: &[u8],
    ) -> Result<HashMap<String, EncryptedMessage>> {
        let ready_members: Vec<String> = self
            .member_states
            .iter()
            .filter(|(_, state)| **state == GroupEncryptionState::Ready)
            .map(|(nick, _)| nick.clone())
            .collect();

        if ready_members.is_empty() {
            return Err(CryptoError::Ratchet(
                "no group members with ready sessions".into(),
            ));
        }

        let mut encrypted = HashMap::with_capacity(ready_members.len());
        for nick in &ready_members {
            let session = self.sessions.get_mut(nick).ok_or_else(|| {
                CryptoError::Ratchet(format!(
                    "member '{nick}' is Ready but has no session"
                ))
            })?;
            let msg = session.encrypt(plaintext)?;
            encrypted.insert(nick.clone(), msg);
        }

        Ok(encrypted)
    }

    /// Decrypt a message received from a specific group member.
    ///
    /// Uses the pairwise session with the sender.
    ///
    /// # Errors
    ///
    /// Returns an error if no session exists with the sender or decryption
    /// fails.
    pub fn decrypt_from_member(
        &mut self,
        sender: &str,
        encrypted: &EncryptedMessage,
    ) -> Result<Vec<u8>> {
        let session = self.sessions.get_mut(sender).ok_or_else(|| {
            CryptoError::Ratchet(format!(
                "no pairwise session with group member '{sender}'"
            ))
        })?;
        session.decrypt(encrypted)
    }

    /// Return the total number of known members (in any state).
    #[must_use]
    pub fn member_count(&self) -> usize {
        self.member_states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::KemKeyPair;
    use crate::x25519;

    /// Create a pair of linked triple ratchet sessions for testing.
    fn create_test_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
        let shared_secret = [0x42u8; 32];
        let bob_dh = x25519::KeyPair::generate();
        let bob_kem = KemKeyPair::generate();

        let sender = TripleRatchetSession::init_sender(
            &shared_secret,
            bob_dh.public_key(),
            bob_kem.public_key(),
        )
        .expect("init_sender");

        let receiver = TripleRatchetSession::init_receiver(
            &shared_secret,
            bob_dh,
            bob_kem,
        )
        .expect("init_receiver");

        (sender, receiver)
    }

    // ── Construction ────────────────────────────────────────────────

    #[test]
    fn new_creates_empty_manager() {
        let mgr = GroupKeyManager::new(GroupId::new(1));
        assert_eq!(mgr.group_id(), GroupId::new(1));
        assert_eq!(mgr.member_count(), 0);
        assert!(!mgr.all_ready());
    }

    // ── Member management ───────────────────────────────────────────

    #[test]
    fn add_member_starts_pending() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        assert_eq!(mgr.member_state("alice"), Some(GroupEncryptionState::Pending));
        assert_eq!(mgr.member_count(), 1);
    }

    #[test]
    fn add_member_idempotent() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        mgr.set_establishing("alice");
        mgr.add_member("alice"); // should not reset state
        assert_eq!(
            mgr.member_state("alice"),
            Some(GroupEncryptionState::Establishing)
        );
    }

    #[test]
    fn set_establishing_transitions_state() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        mgr.set_establishing("alice");
        assert_eq!(
            mgr.member_state("alice"),
            Some(GroupEncryptionState::Establishing)
        );
    }

    #[test]
    fn set_session_transitions_to_ready() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        let (sender, _receiver) = create_test_session_pair();
        mgr.set_session("alice", sender);
        assert_eq!(mgr.member_state("alice"), Some(GroupEncryptionState::Ready));
        assert!(mgr.has_session("alice"));
    }

    #[test]
    fn remove_member_cleans_up() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (sender, _receiver) = create_test_session_pair();
        mgr.set_session("alice", sender);
        assert_eq!(mgr.member_count(), 1);

        mgr.remove_member("alice");
        assert_eq!(mgr.member_count(), 0);
        assert_eq!(mgr.member_state("alice"), None);
        assert!(!mgr.has_session("alice"));
    }

    #[test]
    fn remove_nonexistent_member_is_noop() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.remove_member("ghost"); // should not panic
        assert_eq!(mgr.member_count(), 0);
    }

    // ── Readiness queries ───────────────────────────────────────────

    #[test]
    fn ready_count_tracks_ready_members() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        mgr.add_member("bob");
        assert_eq!(mgr.ready_count(), 0);

        let (sender, _) = create_test_session_pair();
        mgr.set_session("alice", sender);
        assert_eq!(mgr.ready_count(), 1);

        let (sender2, _) = create_test_session_pair();
        mgr.set_session("bob", sender2);
        assert_eq!(mgr.ready_count(), 2);
    }

    #[test]
    fn all_ready_when_all_members_have_sessions() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (s1, _) = create_test_session_pair();
        let (s2, _) = create_test_session_pair();
        mgr.set_session("alice", s1);
        mgr.set_session("bob", s2);
        assert!(mgr.all_ready());
    }

    #[test]
    fn all_ready_false_with_pending_member() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (sender, _) = create_test_session_pair();
        mgr.set_session("alice", sender);
        mgr.add_member("bob"); // pending
        assert!(!mgr.all_ready());
    }

    // ── members() listing ───────────────────────────────────────────

    #[test]
    fn members_returns_sorted_list() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("charlie");
        mgr.add_member("alice");
        mgr.add_member("bob");
        mgr.set_establishing("bob");

        let members = mgr.members();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0], ("alice".to_owned(), GroupEncryptionState::Pending));
        assert_eq!(members[1], ("bob".to_owned(), GroupEncryptionState::Establishing));
        assert_eq!(members[2], ("charlie".to_owned(), GroupEncryptionState::Pending));
    }

    // ── Encrypt/decrypt ─────────────────────────────────────────────

    #[test]
    fn encrypt_for_group_with_no_ready_members_fails() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        mgr.add_member("alice");
        let result = mgr.encrypt_for_group(b"hello");
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_for_group_encrypts_for_ready_members_only() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (s1, _) = create_test_session_pair();
        mgr.set_session("alice", s1);
        mgr.add_member("bob"); // pending

        let encrypted = mgr.encrypt_for_group(b"hello group").unwrap();
        assert_eq!(encrypted.len(), 1);
        assert!(encrypted.contains_key("alice"));
        assert!(!encrypted.contains_key("bob"));
    }

    #[test]
    fn encrypt_decrypt_roundtrip_two_members() {
        // Simulate: "me" sends to a group with alice and bob.
        // me -> alice: sender session, alice has receiver session
        // me -> bob: sender session, bob has receiver session
        let (me_to_alice, alice_from_me) = create_test_session_pair();
        let (me_to_bob, bob_from_me) = create_test_session_pair();

        // Set up "me" group manager with sender sessions
        let mut me_mgr = GroupKeyManager::new(GroupId::new(1));
        me_mgr.set_session("alice", me_to_alice);
        me_mgr.set_session("bob", me_to_bob);

        // Set up alice's group manager with receiver session from "me"
        let mut alice_mgr = GroupKeyManager::new(GroupId::new(1));
        alice_mgr.set_session("me", alice_from_me);

        // Set up bob's group manager with receiver session from "me"
        let mut bob_mgr = GroupKeyManager::new(GroupId::new(1));
        bob_mgr.set_session("me", bob_from_me);

        let plaintext = b"Hello, group!";
        let encrypted = me_mgr.encrypt_for_group(plaintext).unwrap();
        assert_eq!(encrypted.len(), 2);

        // Alice decrypts her copy
        let alice_msg = &encrypted["alice"];
        let decrypted = alice_mgr.decrypt_from_member("me", alice_msg).unwrap();
        assert_eq!(decrypted, plaintext);

        // Bob decrypts his copy
        let bob_msg = &encrypted["bob"];
        let decrypted = bob_mgr.decrypt_from_member("me", bob_msg).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_from_unknown_member_fails() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let dummy = EncryptedMessage {
            encrypted_header: vec![0; 32],
            header_nonce: [0; 12],
            ciphertext: vec![0; 16],
            body_nonce: [0; 12],
        };
        let result = mgr.decrypt_from_member("ghost", &dummy);
        assert!(result.is_err());
    }

    // ── Member join/leave scenarios ─────────────────────────────────

    #[test]
    fn member_join_adds_new_session() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (s1, _) = create_test_session_pair();
        mgr.set_session("alice", s1);
        assert_eq!(mgr.member_count(), 1);
        assert!(mgr.all_ready());

        // New member joins (starts pending, then establishes)
        mgr.add_member("bob");
        assert!(!mgr.all_ready());
        assert_eq!(mgr.member_count(), 2);

        mgr.set_establishing("bob");
        assert_eq!(
            mgr.member_state("bob"),
            Some(GroupEncryptionState::Establishing)
        );

        let (s2, _) = create_test_session_pair();
        mgr.set_session("bob", s2);
        assert!(mgr.all_ready());
        assert_eq!(mgr.member_count(), 2);
    }

    #[test]
    fn member_leave_discards_session() {
        let mut mgr = GroupKeyManager::new(GroupId::new(1));
        let (s1, _) = create_test_session_pair();
        let (s2, _) = create_test_session_pair();
        mgr.set_session("alice", s1);
        mgr.set_session("bob", s2);
        assert_eq!(mgr.member_count(), 2);

        mgr.remove_member("bob");
        assert_eq!(mgr.member_count(), 1);
        assert!(!mgr.has_session("bob"));
        assert!(mgr.has_session("alice"));
    }

    // ── Multiple messages ───────────────────────────────────────────

    #[test]
    fn multiple_messages_maintain_ratchet_state() {
        let (me_to_alice, alice_from_me) = create_test_session_pair();

        let mut me_mgr = GroupKeyManager::new(GroupId::new(1));
        me_mgr.set_session("alice", me_to_alice);

        let mut alice_mgr = GroupKeyManager::new(GroupId::new(1));
        alice_mgr.set_session("me", alice_from_me);

        for i in 0..5 {
            let plaintext = format!("message {i}");
            let encrypted = me_mgr.encrypt_for_group(plaintext.as_bytes()).unwrap();
            let decrypted = alice_mgr
                .decrypt_from_member("me", &encrypted["alice"])
                .unwrap();
            assert_eq!(decrypted, plaintext.as_bytes());
        }
    }

    // ── Bidirectional communication ─────────────────────────────────

    #[test]
    fn bidirectional_group_communication() {
        // Alice and Bob each have sessions for sending and receiving
        // alice -> bob (alice has sender, bob has receiver)
        let (alice_to_bob, bob_from_alice) = create_test_session_pair();
        // bob -> alice (bob has sender, alice has receiver)
        let (bob_to_alice, alice_from_bob) = create_test_session_pair();

        let mut alice_mgr = GroupKeyManager::new(GroupId::new(1));
        alice_mgr.set_session("bob", alice_to_bob);

        let mut bob_mgr = GroupKeyManager::new(GroupId::new(1));
        bob_mgr.set_session("alice", bob_to_alice);

        // We also need receivers for the other direction
        let mut alice_recv = GroupKeyManager::new(GroupId::new(1));
        alice_recv.set_session("bob", alice_from_bob);

        let mut bob_recv = GroupKeyManager::new(GroupId::new(1));
        bob_recv.set_session("alice", bob_from_alice);

        // Alice sends to group
        let encrypted = alice_mgr.encrypt_for_group(b"from alice").unwrap();
        let decrypted = bob_recv
            .decrypt_from_member("alice", &encrypted["bob"])
            .unwrap();
        assert_eq!(decrypted, b"from alice");

        // Bob sends to group
        let encrypted = bob_mgr.encrypt_for_group(b"from bob").unwrap();
        let decrypted = alice_recv
            .decrypt_from_member("bob", &encrypted["alice"])
            .unwrap();
        assert_eq!(decrypted, b"from bob");
    }
}
