use std::collections::HashSet;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::election::{compute_election_timeout, is_log_up_to_date, ElectionTracker};
use super::log::RaftLog;
use super::rpc::{
    AppendEntries, AppendEntriesResponse, RaftMessage, RequestVote, RequestVoteResponse,
};
use super::state::{LeaderState, VolatileState};
use super::storage::RaftStorage;
use super::types::{LogIndex, NodeId, RaftConfig, RaftState, Term};

/// A Raft consensus node that is transport-agnostic.
///
/// Outbound messages are sent via an `mpsc::UnboundedSender`. The caller is
/// responsible for delivering them over the network.
pub struct RaftNode<T: Send + Sync, S: RaftStorage<T>> {
    config: RaftConfig,
    pub(crate) state: RaftState,
    pub(crate) current_term: Term,
    voted_for: Option<NodeId>,
    pub(crate) log: RaftLog<T>,
    volatile: VolatileState,
    leader_state: Option<LeaderState>,
    current_leader: Option<NodeId>,
    election_tracker: ElectionTracker,
    pub(crate) storage: S,
    outbound: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
}

impl<T, S> RaftNode<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    /// Create a new `RaftNode` with the given config and storage backend.
    ///
    /// Returns the node and a receiver for outbound messages.
    pub async fn new(
        config: RaftConfig,
        storage: S,
    ) -> Result<
        (Self, mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>),
        super::storage::StorageError,
    > {
        let current_term = storage.load_term().await?;
        let voted_for = storage.load_voted_for().await?;
        let entries = storage.load_log().await?;
        let log = RaftLog::from_entries(entries);

        let (tx, rx) = mpsc::unbounded_channel();

        let cluster_size = 1 + config.peers.len();
        let node = Self {
            config,
            state: RaftState::Follower,
            current_term,
            voted_for,
            log,
            volatile: VolatileState::default(),
            leader_state: None,
            current_leader: None,
            election_tracker: ElectionTracker::new(cluster_size),
            storage,
            outbound: tx,
        };

        Ok((node, rx))
    }

    // ---- Accessors ----

    pub fn node_id(&self) -> NodeId {
        self.config.node_id
    }

    pub fn state(&self) -> RaftState {
        self.state
    }

    pub fn current_term(&self) -> Term {
        self.current_term
    }

    pub fn voted_for(&self) -> Option<NodeId> {
        self.voted_for
    }

    pub fn current_leader(&self) -> Option<NodeId> {
        self.current_leader
    }

    pub fn log(&self) -> &RaftLog<T> {
        &self.log
    }

    pub fn volatile_state(&self) -> &VolatileState {
        &self.volatile
    }

    pub fn leader_state(&self) -> Option<&LeaderState> {
        self.leader_state.as_ref()
    }

    pub fn config(&self) -> &RaftConfig {
        &self.config
    }

    /// Total number of nodes in the cluster (self + peers).
    pub fn cluster_size(&self) -> usize {
        1 + self.config.peers.len()
    }

    /// Majority quorum size.
    pub fn quorum_size(&self) -> usize {
        self.cluster_size() / 2 + 1
    }

    // ---- Term management ----

    /// Check an incoming term. If it's greater than ours, step down to Follower.
    /// Returns `true` if the term was updated (i.e. we stepped down).
    pub async fn handle_term_update(
        &mut self,
        received_term: Term,
    ) -> Result<bool, super::storage::StorageError> {
        if received_term > self.current_term {
            info!(
                node = %self.config.node_id,
                old_term = %self.current_term,
                new_term = %received_term,
                "received higher term, stepping down to Follower"
            );
            self.current_term = received_term;
            self.voted_for = None;
            self.state = RaftState::Follower;
            self.leader_state = None;
            self.election_tracker.reset();
            self.storage.save_term(self.current_term).await?;
            self.storage.save_voted_for(None).await?;
            return Ok(true);
        }
        Ok(false)
    }

    // ---- Election ----

    /// Start an election: increment term, vote for self, send `RequestVote` RPCs.
    pub async fn start_election(&mut self) -> Result<(), super::storage::StorageError> {
        self.current_term = self.current_term.increment();
        self.state = RaftState::Candidate;
        self.voted_for = Some(self.config.node_id);
        self.current_leader = None;
        self.leader_state = None;
        self.election_tracker.reset();
        self.election_tracker.record_vote(self.config.node_id);

        info!(
            node = %self.config.node_id,
            term = %self.current_term,
            "starting election"
        );

        self.storage.save_term(self.current_term).await?;
        self.storage.save_voted_for(self.voted_for).await?;

        // Check if we already have a majority (solo cluster).
        if self.election_tracker.has_quorum() {
            self.become_leader();
            return Ok(());
        }

        let request = RequestVote {
            term: self.current_term,
            candidate_id: self.config.node_id,
            last_log_index: self.log.last_index(),
            last_log_term: self.log.last_term(),
        };

        for &peer in &self.config.peers {
            let _ = self
                .outbound
                .send((peer, RaftMessage::RequestVote(request.clone())));
        }

        Ok(())
    }

    /// Handle an incoming `RequestVote` RPC.
    pub async fn handle_request_vote(
        &mut self,
        req: RequestVote,
    ) -> Result<RequestVoteResponse, super::storage::StorageError> {
        // Step down if the requester has a higher term.
        self.handle_term_update(req.term).await?;

        // Reply false if request term < our current term.
        if req.term < self.current_term {
            debug!(
                node = %self.config.node_id,
                req_term = %req.term,
                our_term = %self.current_term,
                candidate = %req.candidate_id,
                "rejecting vote: stale term"
            );
            return Ok(RequestVoteResponse {
                term: self.current_term,
                vote_granted: false,
            });
        }

        // Check if we can grant the vote.
        let can_vote = match self.voted_for {
            None => true,
            Some(id) => id == req.candidate_id,
        };

        if !can_vote {
            debug!(
                node = %self.config.node_id,
                candidate = %req.candidate_id,
                voted_for = ?self.voted_for,
                "rejecting vote: already voted for someone else"
            );
            return Ok(RequestVoteResponse {
                term: self.current_term,
                vote_granted: false,
            });
        }

        // Log up-to-date check (§5.4.1): candidate's log must be at least as
        // up-to-date as ours. Compare last_log_term first, then last_log_index.
        let log_ok = is_log_up_to_date(
            req.last_log_term,
            req.last_log_index,
            self.log.last_term(),
            self.log.last_index(),
        );

        if !log_ok {
            debug!(
                node = %self.config.node_id,
                candidate = %req.candidate_id,
                "rejecting vote: candidate log not up-to-date"
            );
            return Ok(RequestVoteResponse {
                term: self.current_term,
                vote_granted: false,
            });
        }

        // Grant the vote.
        self.voted_for = Some(req.candidate_id);
        self.storage.save_voted_for(self.voted_for).await?;

        info!(
            node = %self.config.node_id,
            candidate = %req.candidate_id,
            term = %self.current_term,
            "granting vote"
        );

        Ok(RequestVoteResponse {
            term: self.current_term,
            vote_granted: true,
        })
    }

    /// Handle an incoming `RequestVoteResponse`.
    pub async fn handle_request_vote_response(
        &mut self,
        from: NodeId,
        resp: RequestVoteResponse,
    ) -> Result<(), super::storage::StorageError> {
        self.handle_term_update(resp.term).await?;

        // Only process if we're still a Candidate in the same term.
        if self.state != RaftState::Candidate {
            return Ok(());
        }

        if resp.vote_granted {
            self.election_tracker.record_vote(from);
            debug!(
                node = %self.config.node_id,
                from = %from,
                votes = self.election_tracker.vote_count(),
                needed = self.quorum_size(),
                "received vote"
            );

            if self.election_tracker.has_quorum() {
                self.become_leader();
            }
        }

        Ok(())
    }

    /// Transition to Leader state and send initial heartbeats.
    fn become_leader(&mut self) {
        info!(
            node = %self.config.node_id,
            term = %self.current_term,
            "became leader"
        );

        self.state = RaftState::Leader;
        self.current_leader = Some(self.config.node_id);
        self.leader_state = Some(LeaderState::new(&self.config.peers, self.log.last_index()));

        // Send initial empty AppendEntries (heartbeats) to all peers.
        self.send_heartbeats();
    }

    /// Send heartbeat (empty `AppendEntries`) to all peers.
    pub fn send_heartbeats(&self) {
        if self.state != RaftState::Leader {
            warn!(
                node = %self.config.node_id,
                state = %self.state,
                "send_heartbeats called but not leader"
            );
            return;
        }

        for &peer in &self.config.peers {
            let prev_log_index = self.log.last_index();
            let prev_log_term = self.log.last_term();

            let heartbeat = AppendEntries {
                term: self.current_term,
                leader_id: self.config.node_id,
                prev_log_index,
                prev_log_term,
                entries: vec![],
                leader_commit: self.volatile.commit_index,
            };

            let _ = self
                .outbound
                .send((peer, RaftMessage::AppendEntries(heartbeat)));
        }
    }

    /// Handle an incoming `AppendEntries` RPC.
    pub async fn handle_append_entries(
        &mut self,
        req: AppendEntries<T>,
    ) -> Result<AppendEntriesResponse, super::storage::StorageError> {
        self.handle_term_update(req.term).await?;

        // Reply false if request term < our current term.
        if req.term < self.current_term {
            return Ok(AppendEntriesResponse {
                term: self.current_term,
                success: false,
                match_index: LogIndex::new(0),
            });
        }

        // Valid leader — reset election state.
        self.state = RaftState::Follower;
        self.current_leader = Some(req.leader_id);
        self.leader_state = None;
        self.election_tracker.reset();

        // Try to append entries.
        let success = self
            .log
            .append_entries(req.prev_log_index, req.prev_log_term, &req.entries);

        if !success {
            return Ok(AppendEntriesResponse {
                term: self.current_term,
                success: false,
                match_index: LogIndex::new(0),
            });
        }

        // Update commit index.
        if req.leader_commit > self.volatile.commit_index {
            self.volatile.commit_index = std::cmp::min(req.leader_commit, self.log.last_index());
        }

        Ok(AppendEntriesResponse {
            term: self.current_term,
            success: true,
            match_index: self.log.last_index(),
        })
    }

    /// Handle an incoming `AppendEntriesResponse`.
    pub async fn handle_append_entries_response(
        &mut self,
        from: NodeId,
        resp: AppendEntriesResponse,
    ) -> Result<(), super::storage::StorageError> {
        self.handle_term_update(resp.term).await?;

        if self.state != RaftState::Leader {
            return Ok(());
        }

        if let Some(ref mut leader) = self.leader_state {
            if resp.success {
                leader.match_index.insert(from, resp.match_index);
                leader.next_index.insert(from, resp.match_index + 1);
            } else {
                // Decrement next_index and retry (handled by tick/heartbeat cycle).
                let next = leader
                    .next_index
                    .get(&from)
                    .copied()
                    .unwrap_or(LogIndex::new(1));
                if next.as_u64() > 1 {
                    leader.next_index.insert(from, next - 1);
                }
            }
        }

        Ok(())
    }

    // ---- Election timeout calculation ----

    /// Compute the election timeout for this node based on its priority.
    ///
    /// Higher-priority nodes (earlier in the peer list) get shorter timeouts,
    /// implementing deterministic pre-planned succession.
    pub fn election_timeout(&self) -> std::time::Duration {
        compute_election_timeout(&self.config)
    }
}

// Provide a way to inspect the node's votes for testing.
impl<T, S> RaftNode<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    pub fn votes_received(&self) -> &HashSet<NodeId> {
        self.election_tracker.voters()
    }
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod node_tests;
