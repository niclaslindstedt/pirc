//! Encrypted group chat manager.
//!
//! Orchestrates [`GroupMesh`] (P2P transport tracking) and
//! [`GroupKeyManager`] (pairwise encryption) to provide encrypted
//! group message fan-out and reception.

mod envelope;
mod types;

#[cfg(test)]
mod tests;

pub use envelope::MessageEnvelope;
pub use types::{DeliveryPath, DeliveryResult, ReceivedGroupMessage, RelayMessage};

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pirc_common::types::GroupId;
use pirc_crypto::group_key::{GroupEncryptionState, GroupKeyManager};
use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_p2p::group_mesh::{GroupMesh, GroupMeshEvent, PeerConnectionState};
use pirc_p2p::EncryptedP2pTransport;
use tracing::{debug, info, warn};

/// Per-group state: encryption manager, mesh topology, and sequence counter.
struct GroupState {
    /// Pairwise encryption sessions for each member.
    key_manager: GroupKeyManager,
    /// P2P transport tracking for each member.
    mesh: GroupMesh,
    /// Next outgoing sequence number for this group.
    next_sequence: u64,
}

/// Manages encrypted group chat fan-out and reception.
///
/// For each group, the manager tracks:
/// - A [`GroupKeyManager`] with pairwise encryption sessions per member
/// - A [`GroupMesh`] with P2P transport state per member
/// - A per-group sequence counter for message ordering
///
/// When sending a message, it is wrapped in a [`MessageEnvelope`] (with
/// sequence number and timestamp), then encrypted individually for each
/// member. Connected members receive the message via P2P; degraded
/// members generate [`RelayMessage`]s for the caller to send through the
/// server.
pub struct GroupChatManager {
    /// Per-group state, keyed by group ID.
    groups: HashMap<GroupId, GroupState>,
}

impl GroupChatManager {
    /// Creates a new empty group chat manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
        }
    }

    /// Registers a new group for management.
    ///
    /// If the group already exists, this is a no-op.
    pub fn add_group(&mut self, group_id: GroupId) {
        self.groups.entry(group_id).or_insert_with(|| {
            info!(group_id = group_id.as_u64(), "adding group to chat manager");
            GroupState {
                key_manager: GroupKeyManager::new(group_id),
                mesh: GroupMesh::new(group_id.as_u64().to_string()),
                next_sequence: 1,
            }
        });
    }

    /// Removes a group from management.
    pub fn remove_group(&mut self, group_id: GroupId) {
        self.groups.remove(&group_id);
    }

    /// Returns whether a group is being managed.
    #[must_use]
    pub fn has_group(&self, group_id: GroupId) -> bool {
        self.groups.contains_key(&group_id)
    }

    /// Returns the list of managed group IDs.
    #[must_use]
    pub fn group_ids(&self) -> Vec<GroupId> {
        self.groups.keys().copied().collect()
    }

    // ── Member management ───────────────────────────────────────────

    /// Adds a member to a group's mesh and key manager.
    ///
    /// The member starts in `Connecting` mesh state and `Pending`
    /// encryption state.
    pub fn add_member(&mut self, group_id: GroupId, nickname: &str) {
        let Some(state) = self.groups.get_mut(&group_id) else {
            warn!(group_id = group_id.as_u64(), nickname, "add_member: group not found");
            return;
        };
        state.key_manager.add_member(nickname);
        state.mesh.add_member(nickname.to_owned());
    }

    /// Removes a member from a group.
    pub fn remove_member(&mut self, group_id: GroupId, nickname: &str) {
        let Some(state) = self.groups.get_mut(&group_id) else {
            return;
        };
        state.key_manager.remove_member(nickname);
        state.mesh.remove_member(nickname);
    }

    /// Marks a member's key exchange as in progress.
    pub fn set_member_establishing(&mut self, group_id: GroupId, nickname: &str) {
        if let Some(state) = self.groups.get_mut(&group_id) {
            state.key_manager.set_establishing(nickname);
        }
    }

    /// Registers a completed pairwise session for a member.
    pub fn set_member_session(
        &mut self,
        group_id: GroupId,
        nickname: &str,
        session: TripleRatchetSession,
    ) {
        if let Some(state) = self.groups.get_mut(&group_id) {
            state.key_manager.set_session(nickname, session);
        }
    }

    /// Records that a member's P2P transport is connected.
    pub fn member_connected(
        &mut self,
        group_id: GroupId,
        nickname: &str,
        transport: EncryptedP2pTransport,
    ) {
        if let Some(state) = self.groups.get_mut(&group_id) {
            state.mesh.member_connected(nickname.to_owned(), transport);
        }
    }

    /// Records that a member's P2P connection has degraded.
    pub fn member_degraded(&mut self, group_id: GroupId, nickname: &str, reason: String) {
        if let Some(state) = self.groups.get_mut(&group_id) {
            state.mesh.member_degraded(nickname, reason);
        }
    }

    /// Records that a member has disconnected from P2P.
    pub fn member_disconnected(&mut self, group_id: GroupId, nickname: &str) {
        if let Some(state) = self.groups.get_mut(&group_id) {
            state.mesh.member_disconnected(nickname);
        }
    }

    // ── Query ────────────────────────────────────────────────────────

    /// Returns the encryption state for a member.
    #[must_use]
    pub fn member_encryption_state(
        &self,
        group_id: GroupId,
        nickname: &str,
    ) -> Option<GroupEncryptionState> {
        self.groups
            .get(&group_id)
            .and_then(|s| s.key_manager.member_state(nickname))
    }

    /// Returns the P2P connection state for a member.
    #[must_use]
    pub fn member_connection_state(
        &self,
        group_id: GroupId,
        nickname: &str,
    ) -> Option<PeerConnectionState> {
        self.groups
            .get(&group_id)
            .and_then(|s| s.mesh.member_state(nickname))
    }

    /// Returns nicknames of all P2P-connected members in a group.
    #[must_use]
    pub fn connected_members(&self, group_id: GroupId) -> Vec<String> {
        self.groups
            .get(&group_id)
            .map(|s| s.mesh.connected_members())
            .unwrap_or_default()
    }

    /// Returns nicknames of all degraded (relay-needed) members.
    #[must_use]
    pub fn degraded_members(&self, group_id: GroupId) -> Vec<String> {
        self.groups
            .get(&group_id)
            .map(|s| s.mesh.degraded_members())
            .unwrap_or_default()
    }

    /// Drains mesh events for a group.
    pub fn drain_mesh_events(&mut self, group_id: GroupId) -> Vec<GroupMeshEvent> {
        self.groups
            .get_mut(&group_id)
            .map(|s| s.mesh.drain_events())
            .unwrap_or_default()
    }

    /// Returns whether all members in a group have ready encryption sessions.
    #[must_use]
    pub fn all_encryption_ready(&self, group_id: GroupId) -> bool {
        self.groups
            .get(&group_id)
            .is_some_and(|s| s.key_manager.all_ready())
    }

    // ── Message sending ─────────────────────────────────────────────

    /// Sends an encrypted group message to all ready members.
    ///
    /// The plaintext is wrapped in a [`MessageEnvelope`] with a sequence
    /// number and timestamp, then encrypted individually for each member
    /// with a ready pairwise session.
    ///
    /// For P2P-connected members, the encrypted payload is sent directly
    /// via their transport. For degraded or non-P2P members, a
    /// [`RelayMessage`] is returned for the caller to send through the
    /// server.
    ///
    /// # Errors
    ///
    /// Returns an error if the group is unknown or encryption fails.
    pub async fn send_message(
        &mut self,
        group_id: GroupId,
        plaintext: &[u8],
    ) -> Result<(Vec<DeliveryResult>, Vec<RelayMessage>), String> {
        let state = self
            .groups
            .get_mut(&group_id)
            .ok_or_else(|| format!("group {} not found", group_id.as_u64()))?;

        // Build envelope with sequence number and timestamp
        let envelope = MessageEnvelope {
            sequence_number: state.next_sequence,
            timestamp_ms: current_timestamp_ms(),
            plaintext: plaintext.to_vec(),
        };
        state.next_sequence += 1;

        let envelope_bytes = envelope.to_bytes();

        // Encrypt for all ready members
        let encrypted_map = state
            .key_manager
            .encrypt_for_group(&envelope_bytes)
            .map_err(|e| format!("encryption failed: {e}"))?;

        let mut delivery_results = Vec::with_capacity(encrypted_map.len());
        let mut relay_messages = Vec::new();

        for (nickname, encrypted_msg) in &encrypted_map {
            let payload = encrypted_msg.to_bytes();

            // Try P2P first, fall back to relay
            let path = if let Some(transport) = state.mesh.get_transport(nickname) {
                match transport.send(&payload).await {
                    Ok(()) => {
                        debug!(
                            group_id = group_id.as_u64(),
                            recipient = %nickname,
                            "sent group message via P2P"
                        );
                        DeliveryPath::P2p
                    }
                    Err(e) => {
                        warn!(
                            group_id = group_id.as_u64(),
                            recipient = %nickname,
                            error = %e,
                            "P2P send failed, falling back to relay"
                        );
                        relay_messages.push(RelayMessage {
                            group_id,
                            target: nickname.clone(),
                            encrypted_payload: payload.clone(),
                        });
                        DeliveryPath::Relay
                    }
                }
            } else {
                // No P2P transport — relay required
                debug!(
                    group_id = group_id.as_u64(),
                    recipient = %nickname,
                    "no P2P transport, using relay"
                );
                relay_messages.push(RelayMessage {
                    group_id,
                    target: nickname.clone(),
                    encrypted_payload: payload.clone(),
                });
                DeliveryPath::Relay
            };

            delivery_results.push(DeliveryResult {
                recipient: nickname.clone(),
                path,
                encrypted_payload: payload,
            });
        }

        info!(
            group_id = group_id.as_u64(),
            total = delivery_results.len(),
            p2p = delivery_results.iter().filter(|d| d.path == DeliveryPath::P2p).count(),
            relay = relay_messages.len(),
            "group message fan-out complete"
        );

        Ok((delivery_results, relay_messages))
    }

    // ── Message receiving ───────────────────────────────────────────

    /// Decrypts a received group message from a specific sender.
    ///
    /// The encrypted payload is decrypted using the pairwise session with
    /// the sender, then the [`MessageEnvelope`] is unpacked to extract
    /// the sequence number, timestamp, and plaintext.
    ///
    /// # Errors
    ///
    /// Returns an error if the group is unknown, no session exists with
    /// the sender, or decryption/deserialization fails.
    pub fn receive_message(
        &mut self,
        group_id: GroupId,
        sender: &str,
        encrypted_payload: &[u8],
    ) -> Result<ReceivedGroupMessage, String> {
        let state = self
            .groups
            .get_mut(&group_id)
            .ok_or_else(|| format!("group {} not found", group_id.as_u64()))?;

        // Deserialize the encrypted message
        let encrypted_msg = EncryptedMessage::from_bytes(encrypted_payload)
            .map_err(|e| format!("failed to deserialize encrypted message: {e}"))?;

        // Decrypt using the pairwise session
        let envelope_bytes = state
            .key_manager
            .decrypt_from_member(sender, &encrypted_msg)
            .map_err(|e| format!("decryption failed: {e}"))?;

        // Unpack the envelope
        let envelope = MessageEnvelope::from_bytes(&envelope_bytes)?;

        debug!(
            group_id = group_id.as_u64(),
            sender,
            seq = envelope.sequence_number,
            "received and decrypted group message"
        );

        Ok(ReceivedGroupMessage {
            group_id,
            sender: sender.to_owned(),
            sequence_number: envelope.sequence_number,
            timestamp_ms: envelope.timestamp_ms,
            plaintext: envelope.plaintext,
        })
    }
}

impl Default for GroupChatManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the current time as milliseconds since the Unix epoch.
fn current_timestamp_ms() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch");
    // Use seconds * 1000 + subsec millis to stay within u64.
    duration.as_secs() * 1000 + u64::from(duration.subsec_millis())
}
