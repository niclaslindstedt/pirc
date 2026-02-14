use std::fmt;
use std::ops::{Add, Sub};
use std::time::Duration;

use pirc_common::ServerId;
use serde::{Deserialize, Serialize};

/// Type alias for Raft node identifiers (same as `ServerId`).
pub type NodeId = ServerId;

/// The role of a Raft node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}

impl fmt::Display for RaftState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Follower => write!(f, "Follower"),
            Self::Candidate => write!(f, "Candidate"),
            Self::Leader => write!(f, "Leader"),
        }
    }
}

/// A Raft election term (monotonically increasing epoch).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct Term(u64);

impl Term {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Increment the term by one and return the new value.
    #[must_use]
    pub fn increment(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<u64> for Term {
    type Output = Self;
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<u64> for Term {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

/// A position in the Raft log (1-indexed, 0 means "no entries").
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct LogIndex(u64);

impl LogIndex {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Increment the index by one and return the new value.
    #[must_use]
    pub fn increment(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for LogIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<u64> for LogIndex {
    type Output = Self;
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<u64> for LogIndex {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

/// A single entry in the Raft replicated log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry<T> {
    pub term: Term,
    pub index: LogIndex,
    pub command: T,
}

/// Configuration for Raft consensus behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftConfig {
    /// Minimum election timeout (randomized between min and max).
    #[serde(with = "duration_millis")]
    pub election_timeout_min: Duration,
    /// Maximum election timeout.
    #[serde(with = "duration_millis")]
    pub election_timeout_max: Duration,
    /// Interval between leader heartbeats.
    #[serde(with = "duration_millis")]
    pub heartbeat_interval: Duration,
    /// This node's identifier.
    pub node_id: NodeId,
    /// Peer node identifiers.
    pub peers: Vec<NodeId>,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            election_timeout_min: Duration::from_millis(150),
            election_timeout_max: Duration::from_millis(300),
            heartbeat_interval: Duration::from_millis(50),
            node_id: NodeId::new(0),
            peers: Vec::new(),
        }
    }
}

/// Serde helper for Duration as milliseconds.
mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, s: S) -> Result<S::Ok, S::Error> {
        duration.as_millis().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(d)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RaftState ----

    #[test]
    fn raft_state_display() {
        assert_eq!(RaftState::Follower.to_string(), "Follower");
        assert_eq!(RaftState::Candidate.to_string(), "Candidate");
        assert_eq!(RaftState::Leader.to_string(), "Leader");
    }

    #[test]
    fn raft_state_equality() {
        assert_eq!(RaftState::Follower, RaftState::Follower);
        assert_ne!(RaftState::Follower, RaftState::Leader);
    }

    #[test]
    fn raft_state_serde_roundtrip() {
        for state in [RaftState::Follower, RaftState::Candidate, RaftState::Leader] {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: RaftState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }

    // ---- Term ----

    #[test]
    fn term_construction() {
        let term = Term::new(5);
        assert_eq!(term.as_u64(), 5);
    }

    #[test]
    fn term_default() {
        assert_eq!(Term::default().as_u64(), 0);
    }

    #[test]
    fn term_display() {
        assert_eq!(Term::new(42).to_string(), "42");
    }

    #[test]
    fn term_ordering() {
        assert!(Term::new(1) < Term::new(2));
        assert!(Term::new(5) > Term::new(3));
        assert_eq!(Term::new(1), Term::new(1));
    }

    #[test]
    fn term_increment() {
        assert_eq!(Term::new(3).increment(), Term::new(4));
    }

    #[test]
    fn term_add() {
        assert_eq!(Term::new(3) + 2, Term::new(5));
    }

    #[test]
    fn term_sub() {
        assert_eq!(Term::new(5) - 2, Term::new(3));
    }

    #[test]
    fn term_serde_roundtrip() {
        let term = Term::new(99);
        let json = serde_json::to_string(&term).unwrap();
        let deserialized: Term = serde_json::from_str(&json).unwrap();
        assert_eq!(term, deserialized);
    }

    // ---- LogIndex ----

    #[test]
    fn log_index_construction() {
        let idx = LogIndex::new(10);
        assert_eq!(idx.as_u64(), 10);
    }

    #[test]
    fn log_index_default() {
        assert_eq!(LogIndex::default().as_u64(), 0);
    }

    #[test]
    fn log_index_display() {
        assert_eq!(LogIndex::new(7).to_string(), "7");
    }

    #[test]
    fn log_index_ordering() {
        assert!(LogIndex::new(1) < LogIndex::new(2));
        assert!(LogIndex::new(5) > LogIndex::new(3));
    }

    #[test]
    fn log_index_increment() {
        assert_eq!(LogIndex::new(10).increment(), LogIndex::new(11));
    }

    #[test]
    fn log_index_add() {
        assert_eq!(LogIndex::new(3) + 2, LogIndex::new(5));
    }

    #[test]
    fn log_index_sub() {
        assert_eq!(LogIndex::new(5) - 2, LogIndex::new(3));
    }

    #[test]
    fn log_index_serde_roundtrip() {
        let idx = LogIndex::new(42);
        let json = serde_json::to_string(&idx).unwrap();
        let deserialized: LogIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(idx, deserialized);
    }

    // ---- LogEntry ----

    #[test]
    fn log_entry_construction() {
        let entry = LogEntry {
            term: Term::new(1),
            index: LogIndex::new(1),
            command: "set x 42".to_owned(),
        };
        assert_eq!(entry.term, Term::new(1));
        assert_eq!(entry.index, LogIndex::new(1));
        assert_eq!(entry.command, "set x 42");
    }

    #[test]
    fn log_entry_serde_roundtrip() {
        let entry = LogEntry {
            term: Term::new(3),
            index: LogIndex::new(7),
            command: "hello".to_owned(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: LogEntry<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }

    // ---- RaftConfig ----

    #[test]
    fn raft_config_default() {
        let config = RaftConfig::default();
        assert_eq!(config.election_timeout_min, Duration::from_millis(150));
        assert_eq!(config.election_timeout_max, Duration::from_millis(300));
        assert_eq!(config.heartbeat_interval, Duration::from_millis(50));
        assert_eq!(config.node_id, NodeId::new(0));
        assert!(config.peers.is_empty());
    }

    #[test]
    fn raft_config_serde_roundtrip() {
        let config = RaftConfig {
            election_timeout_min: Duration::from_millis(200),
            election_timeout_max: Duration::from_millis(400),
            heartbeat_interval: Duration::from_millis(75),
            node_id: NodeId::new(1),
            peers: vec![NodeId::new(2), NodeId::new(3)],
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: RaftConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.election_timeout_min,
            config.election_timeout_min
        );
        assert_eq!(
            deserialized.election_timeout_max,
            config.election_timeout_max
        );
        assert_eq!(deserialized.heartbeat_interval, config.heartbeat_interval);
        assert_eq!(deserialized.node_id, config.node_id);
        assert_eq!(deserialized.peers, config.peers);
    }
}
