use tracing::{debug, warn};

use super::rpc::{AppendEntries, RaftMessage};
use super::storage::RaftStorage;
use super::types::{LogIndex, NodeId, RaftState, Term};

use super::node::RaftNode;

impl<T, S> RaftNode<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    /// Send `AppendEntries` to a specific peer with entries from `next_index` onward.
    ///
    /// This is the core replication mechanism: the leader sends log entries
    /// that the peer is missing. If the peer has no missing entries, this
    /// acts as a heartbeat.
    pub fn replicate_to_peer(&self, peer_id: NodeId) {
        if self.state != RaftState::Leader {
            warn!(
                node = %self.node_id(),
                "replicate_to_peer called but not leader"
            );
            return;
        }

        let Some(leader) = &self.leader_state else {
            return;
        };

        let next_idx = leader
            .next_index
            .get(&peer_id)
            .copied()
            .unwrap_or(LogIndex::new(1));

        // The entry just before next_idx for the consistency check.
        let prev_log_index = if next_idx.as_u64() > 0 {
            next_idx - 1
        } else {
            LogIndex::new(0)
        };
        let prev_log_term = if prev_log_index.as_u64() > 0 {
            self.log.term_at(prev_log_index).unwrap_or_default()
        } else {
            Term::default()
        };

        let entries = self.log.entries_from(next_idx).to_vec();

        let ae = AppendEntries {
            term: self.current_term,
            leader_id: self.node_id(),
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit: self.volatile.commit_index,
        };

        let _ = self.outbound.send((peer_id, RaftMessage::AppendEntries(ae)));
    }

    /// Send `AppendEntries` to all peers, including any outstanding log entries.
    ///
    /// This replaces the simple heartbeat-only approach: on each tick the leader
    /// sends entries from `next_index` for each peer, which doubles as a heartbeat
    /// when there are no new entries.
    pub fn send_append_entries_to_all(&self) {
        if self.state != RaftState::Leader {
            warn!(
                node = %self.node_id(),
                state = %self.state,
                "send_append_entries_to_all called but not leader"
            );
            return;
        }

        for &peer in &self.config.peers {
            self.replicate_to_peer(peer);
        }
    }

    /// Advance the commit index based on majority replication.
    ///
    /// For each index N > `commit_index`: if a majority of `match_index[i] >= N`
    /// and `log[N].term == current_term`, set `commit_index = N`.
    ///
    /// Per Raft safety, only entries from the current term are committed directly.
    /// Earlier-term entries are committed indirectly when a current-term entry
    /// is committed at a higher index.
    pub fn advance_commit_index(&mut self) {
        if self.state != RaftState::Leader {
            return;
        }

        let Some(leader) = &self.leader_state else {
            return;
        };

        let last_index = self.log.last_index();
        let current_commit = self.volatile.commit_index;

        // Scan from last_index down to current_commit+1 to find the highest N
        // that a majority has replicated and that is from the current term.
        let mut new_commit = current_commit;
        let mut n = last_index.as_u64();
        while n > current_commit.as_u64() {
            let idx = LogIndex::new(n);

            // Only commit entries from the current term.
            if let Some(entry_term) = self.log.term_at(idx) {
                if entry_term == self.current_term {
                    // Count replicas: leader itself counts as 1.
                    let mut replication_count: usize = 1;
                    for match_idx in leader.match_index.values() {
                        if *match_idx >= idx {
                            replication_count += 1;
                        }
                    }

                    if replication_count >= self.quorum_size() {
                        new_commit = idx;
                        break; // Found the highest committed index.
                    }
                }
            }

            n -= 1;
        }

        if new_commit > current_commit {
            debug!(
                node = %self.node_id(),
                old_commit = %current_commit,
                new_commit = %new_commit,
                "advancing commit index"
            );
            self.volatile.commit_index = new_commit;
        }
    }

    /// Apply committed entries to the state machine.
    ///
    /// Invokes the callback for each entry from `last_applied + 1` up to
    /// `commit_index`, then updates `last_applied`.
    ///
    /// Returns the list of applied entries (useful for the caller to propagate
    /// results back to clients).
    pub fn apply_committed<F>(&mut self, mut apply_fn: F) -> Vec<T>
    where
        F: FnMut(&T),
    {
        let mut applied = Vec::new();
        let commit = self.volatile.commit_index.as_u64();
        let mut last = self.volatile.last_applied.as_u64();

        while last < commit {
            last += 1;
            let idx = LogIndex::new(last);
            if let Some(entry) = self.log.get(idx) {
                apply_fn(&entry.command);
                applied.push(entry.command.clone());
            }
        }

        self.volatile.last_applied = LogIndex::new(last);
        applied
    }

    /// Append a new command to the leader's log and start replication.
    ///
    /// Returns the log index of the new entry, or `None` if this node
    /// is not the leader.
    pub fn client_request(&mut self, command: T) -> Option<LogIndex> {
        if self.state != RaftState::Leader {
            return None;
        }

        let index = self.log.last_index() + 1;
        let entry = super::types::LogEntry {
            term: self.current_term,
            index,
            command,
        };
        self.log.append(entry);

        // Immediately replicate to all peers.
        self.send_append_entries_to_all();

        Some(index)
    }
}

#[cfg(test)]
#[path = "replication_tests.rs"]
mod replication_tests;
