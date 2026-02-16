//! Types used by the group chat manager.

use pirc_common::types::GroupId;

/// Delivery path chosen for a specific recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPath {
    /// Message sent directly via P2P transport.
    P2p,
    /// Message must be relayed through the server.
    Relay,
}

/// Result of sending a group message to one recipient.
#[derive(Debug)]
pub struct DeliveryResult {
    /// Nickname of the recipient.
    pub recipient: String,
    /// How the message was (or should be) delivered.
    pub path: DeliveryPath,
    /// The encrypted payload for this recipient (base bytes).
    pub encrypted_payload: Vec<u8>,
}

/// An outbound relay message that the caller must send to the server.
#[derive(Debug)]
pub struct RelayMessage {
    /// The group this message belongs to.
    pub group_id: GroupId,
    /// The target recipient (nickname).
    pub target: String,
    /// The encrypted payload bytes to relay.
    pub encrypted_payload: Vec<u8>,
}

/// Received group message after decryption.
#[derive(Debug, Clone)]
pub struct ReceivedGroupMessage {
    /// The group the message was received in.
    pub group_id: GroupId,
    /// Nickname of the sender.
    pub sender: String,
    /// Per-sender sequence number.
    pub sequence_number: u64,
    /// Sender's timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// The decrypted plaintext content.
    pub plaintext: Vec<u8>,
}
