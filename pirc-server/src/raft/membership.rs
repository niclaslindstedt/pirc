use std::collections::HashSet;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use super::types::{LogIndex, NodeId};

/// A membership change operation for the Raft cluster.
///
/// Only one membership change can be in progress at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MembershipChange {
    AddServer(NodeId, SocketAddr),
    RemoveServer(NodeId),
}

/// Tracks the current cluster membership and pending changes.
#[derive(Debug, Clone)]
pub struct ClusterMembership {
    /// The current set of voting members (including self).
    members: HashSet<NodeId>,
    /// Generation counter, incremented on each committed membership change.
    generation: u64,
    /// The log index of a pending (uncommitted) membership change, if any.
    pending_change: Option<PendingChange>,
}

/// A membership change that has been appended to the log but not yet committed.
#[derive(Debug, Clone)]
pub struct PendingChange {
    /// The log index where this change was appended.
    pub index: LogIndex,
    /// The change itself.
    pub change: MembershipChange,
}

/// Errors that can occur when proposing a membership change.
#[derive(Debug, thiserror::Error)]
pub enum MembershipError {
    #[error("not the leader")]
    NotLeader,
    #[error("a membership change is already pending at index {0}")]
    ChangePending(LogIndex),
    #[error("leader has not committed an entry in the current term yet")]
    NoCurrentTermCommit,
    #[error("server {0} is already a member")]
    AlreadyMember(NodeId),
    #[error("server {0} is not a member")]
    NotMember(NodeId),
    #[error("cannot remove the last member")]
    CannotRemoveLastMember,
}

impl ClusterMembership {
    /// Create a new membership from the initial cluster configuration.
    ///
    /// `self_id` is this node's ID, `peers` are the other voting members.
    pub fn new(self_id: NodeId, peers: &[NodeId]) -> Self {
        let mut members = HashSet::new();
        members.insert(self_id);
        for &peer in peers {
            members.insert(peer);
        }
        Self {
            members,
            generation: 0,
            pending_change: None,
        }
    }

    /// The current set of voting members.
    pub fn members(&self) -> &HashSet<NodeId> {
        &self.members
    }

    /// The number of voting members.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Majority quorum size for the current membership.
    pub fn quorum_size(&self) -> usize {
        self.members.len() / 2 + 1
    }

    /// The current generation counter.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether a membership change is currently pending (uncommitted).
    pub fn has_pending_change(&self) -> bool {
        self.pending_change.is_some()
    }

    /// Get the pending change, if any.
    pub fn pending_change(&self) -> Option<&PendingChange> {
        self.pending_change.as_ref()
    }

    /// Check if a node is a voting member.
    pub fn is_member(&self, node_id: NodeId) -> bool {
        self.members.contains(&node_id)
    }

    /// Record that a membership change has been appended to the log.
    ///
    /// Per Raft single-server changes, the new configuration takes effect
    /// immediately when the entry is appended (not when committed).
    pub fn begin_change(&mut self, index: LogIndex, change: MembershipChange) {
        match &change {
            MembershipChange::AddServer(node_id, _) => {
                self.members.insert(*node_id);
            }
            MembershipChange::RemoveServer(node_id) => {
                self.members.remove(node_id);
            }
        }
        self.pending_change = Some(PendingChange { index, change });
    }

    /// Mark the pending membership change as committed.
    ///
    /// Increments the generation counter and clears the pending state.
    pub fn commit_change(&mut self) {
        if self.pending_change.is_some() {
            self.generation += 1;
            self.pending_change = None;
        }
    }

    /// Roll back the pending membership change (e.g. on leader step-down).
    ///
    /// Reverts the membership to the state before `begin_change` was called.
    pub fn rollback_change(&mut self) {
        if let Some(pending) = self.pending_change.take() {
            match &pending.change {
                MembershipChange::AddServer(node_id, _) => {
                    self.members.remove(node_id);
                }
                MembershipChange::RemoveServer(node_id) => {
                    self.members.insert(*node_id);
                }
            }
        }
    }

    /// Get the list of peers (all members except `self_id`).
    pub fn peers(&self, self_id: NodeId) -> Vec<NodeId> {
        self.members
            .iter()
            .filter(|&&id| id != self_id)
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64) -> NodeId {
        NodeId::new(id)
    }

    #[test]
    fn new_membership_includes_self_and_peers() {
        let m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        assert_eq!(m.member_count(), 3);
        assert!(m.is_member(node(1)));
        assert!(m.is_member(node(2)));
        assert!(m.is_member(node(3)));
        assert!(!m.is_member(node(4)));
    }

    #[test]
    fn quorum_size_three_node_cluster() {
        let m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        assert_eq!(m.quorum_size(), 2);
    }

    #[test]
    fn quorum_size_five_node_cluster() {
        let m = ClusterMembership::new(node(1), &[node(2), node(3), node(4), node(5)]);
        assert_eq!(m.quorum_size(), 3);
    }

    #[test]
    fn quorum_size_single_node() {
        let m = ClusterMembership::new(node(1), &[]);
        assert_eq!(m.quorum_size(), 1);
    }

    #[test]
    fn initial_generation_is_zero() {
        let m = ClusterMembership::new(node(1), &[node(2)]);
        assert_eq!(m.generation(), 0);
    }

    #[test]
    fn no_pending_change_initially() {
        let m = ClusterMembership::new(node(1), &[node(2)]);
        assert!(!m.has_pending_change());
        assert!(m.pending_change().is_none());
    }

    #[test]
    fn begin_add_server_takes_effect_immediately() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        assert!(!m.is_member(node(4)));

        let addr: SocketAddr = "10.0.0.4:7000".parse().unwrap();
        m.begin_change(LogIndex::new(5), MembershipChange::AddServer(node(4), addr));

        assert!(m.is_member(node(4)));
        assert_eq!(m.member_count(), 4);
        assert!(m.has_pending_change());
        assert_eq!(m.quorum_size(), 3);
    }

    #[test]
    fn begin_remove_server_takes_effect_immediately() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        assert!(m.is_member(node(3)));

        m.begin_change(LogIndex::new(5), MembershipChange::RemoveServer(node(3)));

        assert!(!m.is_member(node(3)));
        assert_eq!(m.member_count(), 2);
        assert!(m.has_pending_change());
    }

    #[test]
    fn commit_change_increments_generation() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        let addr: SocketAddr = "10.0.0.4:7000".parse().unwrap();
        m.begin_change(LogIndex::new(5), MembershipChange::AddServer(node(4), addr));
        m.commit_change();

        assert_eq!(m.generation(), 1);
        assert!(!m.has_pending_change());
        assert!(m.is_member(node(4)));
    }

    #[test]
    fn rollback_add_server() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        let addr: SocketAddr = "10.0.0.4:7000".parse().unwrap();
        m.begin_change(LogIndex::new(5), MembershipChange::AddServer(node(4), addr));
        assert!(m.is_member(node(4)));

        m.rollback_change();
        assert!(!m.is_member(node(4)));
        assert_eq!(m.member_count(), 3);
        assert!(!m.has_pending_change());
    }

    #[test]
    fn rollback_remove_server() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        m.begin_change(LogIndex::new(5), MembershipChange::RemoveServer(node(3)));
        assert!(!m.is_member(node(3)));

        m.rollback_change();
        assert!(m.is_member(node(3)));
        assert_eq!(m.member_count(), 3);
        assert!(!m.has_pending_change());
    }

    #[test]
    fn rollback_noop_when_no_pending() {
        let mut m = ClusterMembership::new(node(1), &[node(2)]);
        m.rollback_change(); // should not panic
        assert_eq!(m.member_count(), 2);
    }

    #[test]
    fn commit_noop_when_no_pending() {
        let mut m = ClusterMembership::new(node(1), &[node(2)]);
        m.commit_change(); // should not panic
        assert_eq!(m.generation(), 0);
    }

    #[test]
    fn peers_excludes_self() {
        let m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        let peers = m.peers(node(1));
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&node(2)));
        assert!(peers.contains(&node(3)));
        assert!(!peers.contains(&node(1)));
    }

    #[test]
    fn pending_change_has_correct_index() {
        let mut m = ClusterMembership::new(node(1), &[node(2)]);
        let addr: SocketAddr = "10.0.0.3:7000".parse().unwrap();
        m.begin_change(
            LogIndex::new(10),
            MembershipChange::AddServer(node(3), addr.clone()),
        );

        let pending = m.pending_change().unwrap();
        assert_eq!(pending.index, LogIndex::new(10));
        assert_eq!(
            pending.change,
            MembershipChange::AddServer(node(3), addr)
        );
    }

    #[test]
    fn membership_change_serde_roundtrip() {
        let addr: SocketAddr = "10.0.0.1:7000".parse().unwrap();
        let add = MembershipChange::AddServer(node(5), addr);
        let json = serde_json::to_string(&add).unwrap();
        let deserialized: MembershipChange = serde_json::from_str(&json).unwrap();
        assert_eq!(add, deserialized);

        let remove = MembershipChange::RemoveServer(node(3));
        let json = serde_json::to_string(&remove).unwrap();
        let deserialized: MembershipChange = serde_json::from_str(&json).unwrap();
        assert_eq!(remove, deserialized);
    }

    #[test]
    fn sequential_changes() {
        let mut m = ClusterMembership::new(node(1), &[node(2), node(3)]);
        let addr: SocketAddr = "10.0.0.4:7000".parse().unwrap();

        // Add node 4
        m.begin_change(LogIndex::new(5), MembershipChange::AddServer(node(4), addr));
        m.commit_change();
        assert_eq!(m.member_count(), 4);
        assert_eq!(m.generation(), 1);

        // Remove node 3
        m.begin_change(LogIndex::new(8), MembershipChange::RemoveServer(node(3)));
        m.commit_change();
        assert_eq!(m.member_count(), 3);
        assert_eq!(m.generation(), 2);
        assert!(!m.is_member(node(3)));
        assert!(m.is_member(node(4)));
    }
}
