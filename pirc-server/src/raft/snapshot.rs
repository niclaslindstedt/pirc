use serde::{Deserialize, Serialize};

use super::types::{LogIndex, NodeId, Term};

/// A point-in-time snapshot of the state machine plus metadata about the
/// log position it covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Index of the last log entry included in this snapshot.
    pub last_included_index: LogIndex,
    /// Term of the last log entry included in this snapshot.
    pub last_included_term: Term,
    /// Serialized state machine state.
    pub data: Vec<u8>,
}

/// `InstallSnapshot` RPC sent by the leader to transfer a snapshot to a
/// follower that is too far behind to catch up via log replication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSnapshot {
    /// Leader's current term.
    pub term: Term,
    /// Leader's node ID (so follower can redirect clients).
    pub leader_id: NodeId,
    /// Index of the last log entry in the snapshot.
    pub last_included_index: LogIndex,
    /// Term of the last log entry in the snapshot.
    pub last_included_term: Term,
    /// Byte offset into the snapshot data for this chunk.
    pub offset: u64,
    /// Raw snapshot data bytes (chunk).
    pub data: Vec<u8>,
    /// True if this is the final chunk.
    pub done: bool,
}

/// Response to an `InstallSnapshot` RPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSnapshotResponse {
    /// Current term of the responding node (for leader to update itself).
    pub term: Term,
}

/// Trait for the application-level state machine that Raft replicates.
///
/// The Raft module calls these methods to apply committed commands,
/// take snapshots of the current state, and restore from snapshots.
pub trait StateMachine<T> {
    /// Apply a committed command to the state machine.
    fn apply(&mut self, command: &T);

    /// Serialize the current state machine state to a byte vector.
    fn snapshot(&self) -> Vec<u8>;

    /// Restore the state machine from a previously serialized snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot data is invalid or corrupted.
    fn restore(&mut self, snapshot: &[u8]) -> Result<(), SnapshotError>;
}

/// A no-op state machine for use when the application handles state
/// management externally (e.g. via the commit channel).
pub struct NullStateMachine;

impl<T> StateMachine<T> for NullStateMachine {
    fn apply(&mut self, _command: &T) {}
    fn snapshot(&self) -> Vec<u8> {
        Vec::new()
    }
    fn restore(&mut self, _snapshot: &[u8]) -> Result<(), SnapshotError> {
        Ok(())
    }
}

/// Errors that can occur during snapshot operations.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("invalid snapshot data: {0}")]
    InvalidData(String),
    #[error("storage error: {0}")]
    Storage(#[from] super::storage::StorageError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Snapshot ----

    #[test]
    fn snapshot_construction() {
        let snap = Snapshot {
            last_included_index: LogIndex::new(10),
            last_included_term: Term::new(3),
            data: vec![1, 2, 3],
        };
        assert_eq!(snap.last_included_index, LogIndex::new(10));
        assert_eq!(snap.last_included_term, Term::new(3));
        assert_eq!(snap.data, vec![1, 2, 3]);
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let snap = Snapshot {
            last_included_index: LogIndex::new(5),
            last_included_term: Term::new(2),
            data: b"state-data".to_vec(),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let deserialized: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, deserialized);
    }

    // ---- InstallSnapshot ----

    #[test]
    fn install_snapshot_serde_roundtrip() {
        let req = InstallSnapshot {
            term: Term::new(5),
            leader_id: NodeId::new(1),
            last_included_index: LogIndex::new(10),
            last_included_term: Term::new(4),
            offset: 0,
            data: b"chunk-data".to_vec(),
            done: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: InstallSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(req, deserialized);
    }

    #[test]
    fn install_snapshot_final_chunk() {
        let req = InstallSnapshot {
            term: Term::new(3),
            leader_id: NodeId::new(2),
            last_included_index: LogIndex::new(20),
            last_included_term: Term::new(3),
            offset: 1024,
            data: b"final-chunk".to_vec(),
            done: true,
        };
        assert!(req.done);
        assert_eq!(req.offset, 1024);
    }

    // ---- InstallSnapshotResponse ----

    #[test]
    fn install_snapshot_response_serde_roundtrip() {
        let resp = InstallSnapshotResponse {
            term: Term::new(5),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: InstallSnapshotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, deserialized);
    }

    // ---- SnapshotError ----

    #[test]
    fn snapshot_error_display() {
        let err = SnapshotError::InvalidData("bad data".into());
        assert_eq!(err.to_string(), "invalid snapshot data: bad data");
    }
}
