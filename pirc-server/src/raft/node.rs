use std::collections::HashSet;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::election::{compute_election_timeout, is_log_up_to_date, ElectionTracker};
use super::log::RaftLog;
use super::rpc::{
    AppendEntries, AppendEntriesResponse, RaftMessage, RequestVote, RequestVoteResponse,
};
use super::snapshot::{
    InstallSnapshot, InstallSnapshotResponse, Snapshot, SnapshotError, StateMachine,
};
use super::state::{LeaderState, VolatileState};
use super::storage::RaftStorage;
use super::types::{LogIndex, NodeId, RaftConfig, RaftState, Term};

/// A Raft consensus node that is transport-agnostic.
///
/// Outbound messages are sent via an `mpsc::UnboundedSender`. The caller is
/// responsible for delivering them over the network.
pub struct RaftNode<T: Send + Sync, S: RaftStorage<T>> {
    pub(crate) config: RaftConfig,
    pub(crate) state: RaftState,
    pub(crate) current_term: Term,
    pub(crate) voted_for: Option<NodeId>,
    pub(crate) log: RaftLog<T>,
    pub(crate) volatile: VolatileState,
    pub(crate) leader_state: Option<LeaderState>,
    pub(crate) current_leader: Option<NodeId>,
    pub(crate) election_tracker: ElectionTracker,
    pub(crate) storage: S,
    pub(crate) outbound: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
    /// The most recent snapshot, if any.
    pub(crate) last_snapshot: Option<Snapshot>,
    /// Buffer for receiving chunked snapshot data from the leader.
    pub(crate) pending_snapshot: Option<Vec<u8>>,
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
        let mut log = RaftLog::from_entries(entries);

        // Load snapshot metadata and restore log offset if a snapshot exists.
        let last_snapshot =
            if let Some((data, last_included_index, last_included_term)) =
                storage.load_snapshot().await?
            {
                log.compact_to(last_included_index);
                // If log was empty (snapshot is ahead), set offset directly.
                if log.offset() < last_included_index {
                    log.reset_to_snapshot(last_included_index, last_included_term);
                }
                Some(Snapshot {
                    last_included_index,
                    last_included_term,
                    data,
                })
            } else {
                None
            };

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
            last_snapshot,
            pending_snapshot: None,
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

    /// Send heartbeat/replication `AppendEntries` to all peers.
    ///
    /// Each peer receives entries starting from their `next_index`,
    /// which acts as a heartbeat when there are no pending entries.
    pub fn send_heartbeats(&self) {
        self.send_append_entries_to_all();
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

        // Check if we can advance the commit index after updating match_index.
        self.advance_commit_index();

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

    // ---- Snapshot operations ----

    /// Create a snapshot of the current state machine state and compact the log.
    ///
    /// This calls `state_machine.snapshot()` to get the serialized state,
    /// saves it to persistent storage, and then compacts the log up to
    /// `last_applied`.
    pub async fn create_snapshot<M: StateMachine<T>>(
        &mut self,
        state_machine: &M,
    ) -> Result<(), SnapshotError> {
        let last_applied = self.volatile.last_applied;
        if last_applied.as_u64() == 0 {
            return Ok(()); // Nothing to snapshot.
        }

        // If we already have a snapshot at this index, skip.
        if let Some(ref snap) = self.last_snapshot {
            if snap.last_included_index >= last_applied {
                return Ok(());
            }
        }

        let last_applied_term = self
            .log
            .term_at(last_applied)
            .unwrap_or(self.log.snapshot_term());

        let data = state_machine.snapshot();

        // Persist snapshot to storage.
        self.storage
            .save_snapshot(&data, last_applied, last_applied_term)
            .await?;

        // Compact the log.
        self.log.compact_to(last_applied);

        info!(
            node = %self.config.node_id,
            last_included_index = %last_applied,
            last_included_term = %last_applied_term,
            "created snapshot and compacted log"
        );

        self.last_snapshot = Some(Snapshot {
            last_included_index: last_applied,
            last_included_term: last_applied_term,
            data,
        });

        Ok(())
    }

    /// Check if the log has grown past the snapshot threshold and a
    /// snapshot should be created.
    pub fn should_snapshot(&self) -> bool {
        self.log.len() >= self.config.snapshot_threshold
    }

    /// Get the most recent snapshot, if any.
    pub fn last_snapshot(&self) -> Option<&Snapshot> {
        self.last_snapshot.as_ref()
    }

    /// Handle an incoming `InstallSnapshot` RPC.
    ///
    /// Implements the chunked snapshot transfer protocol:
    /// - Rejects if sender's term is stale.
    /// - Accumulates chunks until the final one arrives.
    /// - On completion, restores the state machine and compacts the log.
    pub async fn handle_install_snapshot<M: StateMachine<T>>(
        &mut self,
        req: InstallSnapshot,
        state_machine: &mut M,
    ) -> Result<InstallSnapshotResponse, SnapshotError> {
        // Step down if the sender has a higher term.
        self.handle_term_update(req.term).await?;

        // Reply with current term if request term < our term.
        if req.term < self.current_term {
            return Ok(InstallSnapshotResponse {
                term: self.current_term,
            });
        }

        // Valid leader — reset election state.
        self.state = RaftState::Follower;
        self.current_leader = Some(req.leader_id);
        self.leader_state = None;
        self.election_tracker.reset();

        // Handle chunked transfer.
        if req.offset == 0 {
            // First chunk: start a new pending snapshot buffer.
            self.pending_snapshot = Some(req.data);
        } else if let Some(ref mut buf) = self.pending_snapshot {
            // Subsequent chunk: append to existing buffer.
            buf.extend_from_slice(&req.data);
        } else {
            // Received a non-first chunk without a pending buffer — skip.
            warn!(
                node = %self.config.node_id,
                offset = req.offset,
                "received InstallSnapshot chunk without pending buffer"
            );
            return Ok(InstallSnapshotResponse {
                term: self.current_term,
            });
        }

        if !req.done {
            // More chunks to come.
            return Ok(InstallSnapshotResponse {
                term: self.current_term,
            });
        }

        // All chunks received — install the snapshot.
        let snapshot_data = self.pending_snapshot.take().unwrap_or_default();

        info!(
            node = %self.config.node_id,
            last_included_index = %req.last_included_index,
            last_included_term = %req.last_included_term,
            size = snapshot_data.len(),
            "installing snapshot from leader"
        );

        // Check if our log has an entry at last_included_index with matching term.
        let keep_suffix =
            self.log.term_at(req.last_included_index) == Some(req.last_included_term);

        if keep_suffix {
            // Keep log entries after last_included_index, discard the rest.
            self.log.compact_to(req.last_included_index);
        } else {
            // Discard entire log.
            self.log
                .reset_to_snapshot(req.last_included_index, req.last_included_term);
        }

        // Restore state machine from snapshot.
        state_machine.restore(&snapshot_data)?;

        // Save snapshot to persistent storage.
        self.storage
            .save_snapshot(
                &snapshot_data,
                req.last_included_index,
                req.last_included_term,
            )
            .await?;

        // Update volatile state.
        if req.last_included_index > self.volatile.commit_index {
            self.volatile.commit_index = req.last_included_index;
        }
        if req.last_included_index > self.volatile.last_applied {
            self.volatile.last_applied = req.last_included_index;
        }

        self.last_snapshot = Some(Snapshot {
            last_included_index: req.last_included_index,
            last_included_term: req.last_included_term,
            data: snapshot_data,
        });

        Ok(InstallSnapshotResponse {
            term: self.current_term,
        })
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
