use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{LogEntry, LogIndex, NodeId, Term};

/// Raft persistent state (must be persisted to stable storage before responding to RPCs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState<T> {
    /// Latest term this server has seen.
    pub current_term: Term,
    /// Candidate that received this server's vote in the current term.
    pub voted_for: Option<NodeId>,
    /// The replicated log entries.
    pub log: Vec<LogEntry<T>>,
}

impl<T> Default for PersistentState<T> {
    fn default() -> Self {
        Self {
            current_term: Term::default(),
            voted_for: None,
            log: Vec::new(),
        }
    }
}

/// Raft volatile state (kept on all servers, rebuilt after restart).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolatileState {
    /// Index of the highest log entry known to be committed.
    pub commit_index: LogIndex,
    /// Index of the highest log entry applied to the state machine.
    pub last_applied: LogIndex,
}

impl Default for VolatileState {
    fn default() -> Self {
        Self {
            commit_index: LogIndex::new(0),
            last_applied: LogIndex::new(0),
        }
    }
}

/// Raft volatile state kept only on the leader (reinitialized after election).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderState {
    /// For each peer, index of the next log entry to send.
    pub next_index: HashMap<NodeId, LogIndex>,
    /// For each peer, index of the highest log entry known to be replicated.
    pub match_index: HashMap<NodeId, LogIndex>,
}

impl LeaderState {
    /// Initialize leader state for the given set of peers.
    ///
    /// `next_index` is set to `last_log_index + 1` for each peer,
    /// and `match_index` is set to 0.
    pub fn new(peers: &[NodeId], last_log_index: LogIndex) -> Self {
        let next = last_log_index + 1;
        let mut next_index = HashMap::new();
        let mut match_index = HashMap::new();
        for &peer in peers {
            next_index.insert(peer, next);
            match_index.insert(peer, LogIndex::new(0));
        }
        Self {
            next_index,
            match_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- PersistentState ----

    #[test]
    fn persistent_state_default() {
        let state: PersistentState<String> = PersistentState::default();
        assert_eq!(state.current_term, Term::default());
        assert_eq!(state.voted_for, None);
        assert!(state.log.is_empty());
    }

    #[test]
    fn persistent_state_construction() {
        let state = PersistentState {
            current_term: Term::new(3),
            voted_for: Some(NodeId::new(1)),
            log: vec![LogEntry {
                term: Term::new(1),
                index: LogIndex::new(1),
                command: "cmd".to_owned(),
            }],
        };
        assert_eq!(state.current_term, Term::new(3));
        assert_eq!(state.voted_for, Some(NodeId::new(1)));
        assert_eq!(state.log.len(), 1);
    }

    #[test]
    fn persistent_state_serde_roundtrip() {
        let state = PersistentState {
            current_term: Term::new(5),
            voted_for: Some(NodeId::new(2)),
            log: vec![
                LogEntry {
                    term: Term::new(1),
                    index: LogIndex::new(1),
                    command: "a".to_owned(),
                },
                LogEntry {
                    term: Term::new(2),
                    index: LogIndex::new(2),
                    command: "b".to_owned(),
                },
            ],
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: PersistentState<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.current_term, state.current_term);
        assert_eq!(deserialized.voted_for, state.voted_for);
        assert_eq!(deserialized.log.len(), state.log.len());
    }

    // ---- VolatileState ----

    #[test]
    fn volatile_state_default() {
        let state = VolatileState::default();
        assert_eq!(state.commit_index, LogIndex::new(0));
        assert_eq!(state.last_applied, LogIndex::new(0));
    }

    #[test]
    fn volatile_state_construction() {
        let state = VolatileState {
            commit_index: LogIndex::new(5),
            last_applied: LogIndex::new(3),
        };
        assert_eq!(state.commit_index, LogIndex::new(5));
        assert_eq!(state.last_applied, LogIndex::new(3));
    }

    #[test]
    fn volatile_state_serde_roundtrip() {
        let state = VolatileState {
            commit_index: LogIndex::new(10),
            last_applied: LogIndex::new(8),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: VolatileState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    // ---- LeaderState ----

    #[test]
    fn leader_state_new() {
        let peers = vec![NodeId::new(2), NodeId::new(3)];
        let state = LeaderState::new(&peers, LogIndex::new(5));

        assert_eq!(state.next_index[&NodeId::new(2)], LogIndex::new(6));
        assert_eq!(state.next_index[&NodeId::new(3)], LogIndex::new(6));
        assert_eq!(state.match_index[&NodeId::new(2)], LogIndex::new(0));
        assert_eq!(state.match_index[&NodeId::new(3)], LogIndex::new(0));
    }

    #[test]
    fn leader_state_empty_peers() {
        let state = LeaderState::new(&[], LogIndex::new(0));
        assert!(state.next_index.is_empty());
        assert!(state.match_index.is_empty());
    }

    #[test]
    fn leader_state_serde_roundtrip() {
        let peers = vec![NodeId::new(1), NodeId::new(2)];
        let state = LeaderState::new(&peers, LogIndex::new(3));
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: LeaderState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }
}
