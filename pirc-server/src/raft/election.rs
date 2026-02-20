use std::collections::HashSet;
use std::time::Duration;

use super::types::{NodeId, RaftConfig};

/// Tracks election-specific state: votes received, timeout computation.
#[derive(Debug)]
pub struct ElectionTracker {
    votes: HashSet<NodeId>,
    cluster_size: usize,
}

impl ElectionTracker {
    /// Create a new election tracker for a cluster of the given size.
    pub fn new(cluster_size: usize) -> Self {
        Self {
            votes: HashSet::new(),
            cluster_size,
        }
    }

    /// Record a vote from a node. Returns `true` if quorum is now reached.
    pub fn record_vote(&mut self, from: NodeId) -> bool {
        self.votes.insert(from);
        self.has_quorum()
    }

    /// Whether the quorum (majority) has been reached.
    pub fn has_quorum(&self) -> bool {
        self.votes.len() >= self.quorum_size()
    }

    /// Required number of votes for a majority.
    pub fn quorum_size(&self) -> usize {
        self.cluster_size / 2 + 1
    }

    /// How many votes have been received.
    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }

    /// Reset votes for a new election.
    pub fn reset(&mut self) {
        self.votes.clear();
    }

    /// The set of nodes that have voted.
    pub fn voters(&self) -> &HashSet<NodeId> {
        &self.votes
    }
}

/// Compute the deterministic election timeout for a node based on its priority.
///
/// Nodes are sorted by ID; lower IDs get shorter timeouts (higher priority),
/// which implements a deterministic pre-planned succession order.
pub fn compute_election_timeout(config: &RaftConfig) -> Duration {
    let min = config.election_timeout_min;
    let max = config.election_timeout_max;
    let range = max.saturating_sub(min);

    // Count how many peers have a lower ID than us (our position in sorted order).
    let our_id = config.node_id.as_u64();
    let mut position: usize = 0;
    for peer in &config.peers {
        if peer.as_u64() < our_id {
            position += 1;
        }
    }

    let total = config.peers.len() + 1; // peers + self
    if total <= 1 {
        return min;
    }

    #[allow(clippy::cast_precision_loss)]
    let fraction = position as f64 / (total - 1) as f64;
    let offset = Duration::from_secs_f64(range.as_secs_f64() * fraction);

    min + offset
}

/// Check whether a candidate's log is at least as up-to-date as ours.
///
/// Per Raft §5.4.1: compare `last_log_term` first, then `last_log_index`.
pub fn is_log_up_to_date(
    candidate_last_term: super::types::Term,
    candidate_last_index: super::types::LogIndex,
    our_last_term: super::types::Term,
    our_last_index: super::types::LogIndex,
) -> bool {
    candidate_last_term > our_last_term
        || (candidate_last_term == our_last_term && candidate_last_index >= our_last_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::types::{LogIndex, Term};

    // ---- ElectionTracker ----

    #[test]
    fn tracker_new() {
        let tracker = ElectionTracker::new(3);
        assert_eq!(tracker.vote_count(), 0);
        assert_eq!(tracker.quorum_size(), 2);
        assert!(!tracker.has_quorum());
    }

    #[test]
    fn tracker_single_node_quorum() {
        let mut tracker = ElectionTracker::new(1);
        assert_eq!(tracker.quorum_size(), 1);
        assert!(tracker.record_vote(NodeId::new(1)));
    }

    #[test]
    fn tracker_three_node_quorum() {
        let mut tracker = ElectionTracker::new(3);
        assert!(!tracker.record_vote(NodeId::new(1)));
        assert!(tracker.record_vote(NodeId::new(2)));
        assert_eq!(tracker.vote_count(), 2);
    }

    #[test]
    fn tracker_five_node_quorum() {
        let mut tracker = ElectionTracker::new(5);
        assert_eq!(tracker.quorum_size(), 3);
        tracker.record_vote(NodeId::new(1));
        tracker.record_vote(NodeId::new(2));
        assert!(!tracker.has_quorum());
        assert!(tracker.record_vote(NodeId::new(3)));
    }

    #[test]
    fn tracker_duplicate_vote() {
        let mut tracker = ElectionTracker::new(3);
        tracker.record_vote(NodeId::new(1));
        tracker.record_vote(NodeId::new(1)); // duplicate
        assert_eq!(tracker.vote_count(), 1);
        assert!(!tracker.has_quorum());
    }

    #[test]
    fn tracker_reset() {
        let mut tracker = ElectionTracker::new(3);
        tracker.record_vote(NodeId::new(1));
        tracker.record_vote(NodeId::new(2));
        assert!(tracker.has_quorum());
        tracker.reset();
        assert_eq!(tracker.vote_count(), 0);
        assert!(!tracker.has_quorum());
    }

    #[test]
    fn tracker_voters() {
        let mut tracker = ElectionTracker::new(3);
        tracker.record_vote(NodeId::new(1));
        tracker.record_vote(NodeId::new(3));
        let voters = tracker.voters();
        assert!(voters.contains(&NodeId::new(1)));
        assert!(voters.contains(&NodeId::new(3)));
        assert!(!voters.contains(&NodeId::new(2)));
    }

    // ---- compute_election_timeout ----

    #[test]
    fn timeout_solo_returns_min() {
        let config = RaftConfig {
            node_id: NodeId::new(1),
            peers: vec![],
            ..RaftConfig::default()
        };
        assert_eq!(
            compute_election_timeout(&config),
            config.election_timeout_min
        );
    }

    #[test]
    fn timeout_lowest_id_gets_min() {
        let config = RaftConfig {
            node_id: NodeId::new(1),
            peers: vec![NodeId::new(2), NodeId::new(3)],
            ..RaftConfig::default()
        };
        assert_eq!(
            compute_election_timeout(&config),
            config.election_timeout_min
        );
    }

    #[test]
    fn timeout_highest_id_gets_max() {
        let config = RaftConfig {
            node_id: NodeId::new(3),
            peers: vec![NodeId::new(1), NodeId::new(2)],
            ..RaftConfig::default()
        };
        assert_eq!(
            compute_election_timeout(&config),
            config.election_timeout_max
        );
    }

    #[test]
    fn timeout_ordering_by_id() {
        let t1 = compute_election_timeout(&RaftConfig {
            node_id: NodeId::new(1),
            peers: vec![NodeId::new(2), NodeId::new(3)],
            ..RaftConfig::default()
        });
        let t2 = compute_election_timeout(&RaftConfig {
            node_id: NodeId::new(2),
            peers: vec![NodeId::new(1), NodeId::new(3)],
            ..RaftConfig::default()
        });
        let t3 = compute_election_timeout(&RaftConfig {
            node_id: NodeId::new(3),
            peers: vec![NodeId::new(1), NodeId::new(2)],
            ..RaftConfig::default()
        });

        assert!(t1 < t2);
        assert!(t2 < t3);
    }

    #[test]
    fn timeout_all_within_bounds() {
        for id in 1..=5 {
            let peers: Vec<NodeId> = (1..=5).filter(|&i| i != id).map(NodeId::new).collect();
            let config = RaftConfig {
                node_id: NodeId::new(id),
                peers,
                ..RaftConfig::default()
            };
            let timeout = compute_election_timeout(&config);
            assert!(timeout >= config.election_timeout_min);
            assert!(timeout <= config.election_timeout_max);
        }
    }

    // ---- is_log_up_to_date ----

    #[test]
    fn up_to_date_both_empty() {
        assert!(is_log_up_to_date(
            Term::new(0),
            LogIndex::new(0),
            Term::new(0),
            LogIndex::new(0),
        ));
    }

    #[test]
    fn up_to_date_higher_term() {
        assert!(is_log_up_to_date(
            Term::new(3),
            LogIndex::new(1),
            Term::new(2),
            LogIndex::new(5),
        ));
    }

    #[test]
    fn up_to_date_same_term_longer_log() {
        assert!(is_log_up_to_date(
            Term::new(2),
            LogIndex::new(5),
            Term::new(2),
            LogIndex::new(3),
        ));
    }

    #[test]
    fn up_to_date_same_term_same_index() {
        assert!(is_log_up_to_date(
            Term::new(2),
            LogIndex::new(3),
            Term::new(2),
            LogIndex::new(3),
        ));
    }

    #[test]
    fn not_up_to_date_lower_term() {
        assert!(!is_log_up_to_date(
            Term::new(1),
            LogIndex::new(10),
            Term::new(2),
            LogIndex::new(1),
        ));
    }

    #[test]
    fn not_up_to_date_same_term_shorter_log() {
        assert!(!is_log_up_to_date(
            Term::new(2),
            LogIndex::new(2),
            Term::new(2),
            LogIndex::new(5),
        ));
    }
}
