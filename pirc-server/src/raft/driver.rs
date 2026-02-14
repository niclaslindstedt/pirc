use tokio::sync::{mpsc, watch};
use tokio::time::{self, Instant};
use tracing::{debug, info, warn};

use super::node::RaftNode;
use super::rpc::RaftMessage;
use super::storage::RaftStorage;
use super::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

/// A handle for interacting with a running Raft node.
///
/// Uses channels and watch to communicate with the driver loop.
pub struct RaftHandle<T: Send + 'static> {
    proposal_tx: mpsc::UnboundedSender<T>,
    commit_rx: mpsc::UnboundedReceiver<LogEntry<T>>,
    state_rx: watch::Receiver<(RaftState, Term, Option<NodeId>)>,
}

impl<T: Send + 'static> RaftHandle<T> {
    /// Propose a command to the Raft cluster.
    ///
    /// Returns `Ok(())` if the proposal was submitted to the leader.
    /// Returns `Err` if this node is not the leader or the driver has shut down.
    pub fn propose(&self, command: T) -> Result<(), RaftError> {
        if !self.is_leader() {
            return Err(RaftError::NotLeader);
        }
        self.proposal_tx
            .send(command)
            .map_err(|_| RaftError::Shutdown)
    }

    /// Check if this node is the current leader.
    pub fn is_leader(&self) -> bool {
        self.state_rx.borrow().0 == RaftState::Leader
    }

    /// Get the current leader node ID, if known.
    pub fn current_leader(&self) -> Option<NodeId> {
        self.state_rx.borrow().2
    }

    /// Get the current term.
    pub fn current_term(&self) -> Term {
        self.state_rx.borrow().1
    }

    /// Get the current Raft state (Follower, Candidate, or Leader).
    pub fn state(&self) -> RaftState {
        self.state_rx.borrow().0
    }

    /// Take the commit receiver to consume committed log entries.
    ///
    /// This can only be called once; subsequent calls return an empty receiver.
    pub fn take_commit_rx(&mut self) -> mpsc::UnboundedReceiver<LogEntry<T>> {
        let (_, empty_rx) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.commit_rx, empty_rx)
    }
}

/// Errors returned by [`RaftHandle`] operations.
#[derive(Debug, thiserror::Error)]
pub enum RaftError {
    #[error("not the leader")]
    NotLeader,
    #[error("raft driver has shut down")]
    Shutdown,
}

/// Signal used to request graceful shutdown of the driver loop.
pub struct ShutdownSignal {
    rx: watch::Receiver<bool>,
}

impl ShutdownSignal {
    async fn wait(&mut self) {
        let _ = self.rx.wait_for(|&v| v).await;
    }
}

/// Sender half of the shutdown signal.
pub struct ShutdownSender {
    tx: watch::Sender<bool>,
}

impl ShutdownSender {
    /// Signal the driver to shut down.
    pub fn shutdown(&self) {
        let _ = self.tx.send(true);
    }
}

/// Create a paired shutdown signal and sender.
pub fn shutdown_channel() -> (ShutdownSender, ShutdownSignal) {
    let (tx, rx) = watch::channel(false);
    (ShutdownSender { tx }, ShutdownSignal { rx })
}

/// The async event loop that drives a Raft node.
///
/// Owns the [`RaftNode`] and processes timers (election timeout, heartbeat
/// interval), inbound messages, and client proposals via `tokio::select!`.
pub struct RaftDriver<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    node: RaftNode<T, S>,
    /// Receives messages from other Raft nodes (delivered by the transport layer).
    inbound_rx: mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
    /// Receives outbound messages from the node (to be delivered by transport).
    outbound_rx: mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
    /// Forwards outbound messages to the transport layer.
    outbound_fwd: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
    /// Client proposals to be appended to the log.
    proposal_rx: mpsc::UnboundedReceiver<T>,
    /// Sender for committed entries (consumed by the application).
    commit_tx: mpsc::UnboundedSender<LogEntry<T>>,
    /// Publishes state changes for external queries.
    state_tx: watch::Sender<(RaftState, Term, Option<NodeId>)>,
    /// Graceful shutdown signal.
    shutdown: ShutdownSignal,
    /// Whether an election timeout reset was requested.
    election_reset: bool,
}

impl<T, S> RaftDriver<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    /// Run the event loop until shutdown is signalled.
    pub async fn run(&mut self) {
        info!(
            node = %self.node.node_id(),
            "raft driver starting"
        );

        let mut election_deadline = Instant::now() + self.node.election_timeout();
        let heartbeat_interval = self.node.config().heartbeat_interval;

        // Heartbeat ticker — only ticks when we're leader.
        let mut heartbeat_tick = time::interval(heartbeat_interval);
        heartbeat_tick.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        // Consume the first immediate tick.
        heartbeat_tick.tick().await;

        loop {
            let election_sleep = time::sleep_until(election_deadline);

            tokio::select! {
                // Election timeout fires (non-leader only).
                () = election_sleep, if self.node.state() != RaftState::Leader => {
                    self.handle_election_timeout().await;
                    election_deadline = Instant::now() + self.node.election_timeout();
                }

                // Heartbeat interval fires (leader only).
                _ = heartbeat_tick.tick(), if self.node.state() == RaftState::Leader => {
                    self.handle_heartbeat_tick();
                }

                // Inbound message from another Raft node.
                Some((from, msg)) = self.inbound_rx.recv() => {
                    let was_leader = self.node.state() == RaftState::Leader;
                    self.handle_inbound_message(from, msg).await;
                    if self.election_reset {
                        election_deadline = Instant::now() + self.node.election_timeout();
                        self.election_reset = false;
                    }
                    // If we just became leader, reset the heartbeat interval.
                    if !was_leader && self.node.state() == RaftState::Leader {
                        heartbeat_tick.reset();
                    }
                }

                // Outbound message produced by the node — forward to transport.
                Some(msg) = self.outbound_rx.recv() => {
                    let _ = self.outbound_fwd.send(msg);
                }

                // Client proposal.
                Some(command) = self.proposal_rx.recv() => {
                    self.handle_proposal(command);
                }

                // Shutdown signal.
                () = self.shutdown.wait() => {
                    info!(
                        node = %self.node.node_id(),
                        "raft driver shutting down"
                    );
                    break;
                }
            }

            // Publish updated state after each event.
            self.publish_state();

            // Apply any newly committed entries.
            self.apply_committed_entries();
        }
    }

    async fn handle_election_timeout(&mut self) {
        info!(
            node = %self.node.node_id(),
            state = %self.node.state(),
            term = %self.node.current_term(),
            "election timeout fired"
        );

        if let Err(e) = self.node.start_election().await {
            warn!(
                node = %self.node.node_id(),
                error = %e,
                "failed to start election"
            );
        }
    }

    fn handle_heartbeat_tick(&mut self) {
        debug!(
            node = %self.node.node_id(),
            term = %self.node.current_term(),
            "heartbeat tick"
        );
        self.node.send_heartbeats();
    }

    async fn handle_inbound_message(&mut self, from: NodeId, msg: RaftMessage<T>) {
        match msg {
            RaftMessage::RequestVote(req) => {
                match self.node.handle_request_vote(req).await {
                    Ok(resp) => {
                        if resp.vote_granted {
                            self.election_reset = true;
                        }
                        let _ = self
                            .node
                            .outbound
                            .send((from, RaftMessage::RequestVoteResponse(resp)));
                    }
                    Err(e) => {
                        warn!(
                            node = %self.node.node_id(),
                            from = %from,
                            error = %e,
                            "error handling RequestVote"
                        );
                    }
                }
            }
            RaftMessage::RequestVoteResponse(resp) => {
                if let Err(e) = self.node.handle_request_vote_response(from, resp).await {
                    warn!(
                        node = %self.node.node_id(),
                        from = %from,
                        error = %e,
                        "error handling RequestVoteResponse"
                    );
                }
            }
            RaftMessage::AppendEntries(req) => {
                let req_term = req.term;
                match self.node.handle_append_entries(req).await {
                    Ok(resp) => {
                        // Reset election timer on valid AppendEntries from current
                        // or higher term leader (even if log match failed).
                        if req_term >= self.node.current_term() {
                            self.election_reset = true;
                        }
                        let _ = self
                            .node
                            .outbound
                            .send((from, RaftMessage::AppendEntriesResponse(resp)));
                    }
                    Err(e) => {
                        warn!(
                            node = %self.node.node_id(),
                            from = %from,
                            error = %e,
                            "error handling AppendEntries"
                        );
                    }
                }
            }
            RaftMessage::AppendEntriesResponse(resp) => {
                if let Err(e) = self.node.handle_append_entries_response(from, resp).await {
                    warn!(
                        node = %self.node.node_id(),
                        from = %from,
                        error = %e,
                        "error handling AppendEntriesResponse"
                    );
                }
            }
        }
    }

    fn handle_proposal(&mut self, command: T) {
        if self.node.state() != RaftState::Leader {
            debug!(
                node = %self.node.node_id(),
                "rejecting proposal: not leader"
            );
            return;
        }

        if let Some(index) = self.node.client_request(command) {
            debug!(
                node = %self.node.node_id(),
                index = %index,
                "appended client proposal"
            );
            // Check if we can immediately commit (e.g. solo node).
            self.node.advance_commit_index();
        }
    }

    fn apply_committed_entries(&mut self) {
        let last_applied = self.node.volatile_state().last_applied;
        let commit_index = self.node.volatile_state().commit_index;

        if last_applied >= commit_index {
            return;
        }

        // Collect entries before applying (to get full LogEntry with correct index/term).
        let mut entries_to_send = Vec::new();
        let mut idx = last_applied.as_u64() + 1;
        while idx <= commit_index.as_u64() {
            let log_idx = LogIndex::new(idx);
            if let Some(entry) = self.node.log().get(log_idx) {
                entries_to_send.push(entry.clone());
            }
            idx += 1;
        }

        // Now apply them (this updates last_applied).
        self.node.apply_committed(|_| {});

        // Send entries to the commit channel.
        for entry in entries_to_send {
            let _ = self.commit_tx.send(entry);
        }
    }

    fn publish_state(&self) {
        let _ = self.state_tx.send((
            self.node.state(),
            self.node.current_term(),
            self.node.current_leader(),
        ));
    }

    #[cfg(test)]
    pub(crate) fn node(&self) -> &RaftNode<T, S> {
        &self.node
    }

    #[cfg(test)]
    pub(crate) fn node_mut(&mut self) -> &mut RaftNode<T, S> {
        &mut self.node
    }
}

/// Builder for constructing a [`RaftDriver`] and [`RaftHandle`] pair.
pub struct RaftBuilder<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    config: Option<RaftConfig>,
    storage: Option<S>,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S> Default for RaftBuilder<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, S> RaftBuilder<T, S>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
{
    pub fn new() -> Self {
        Self {
            config: None,
            storage: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Set the Raft configuration.
    #[must_use]
    pub fn config(mut self, config: RaftConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the storage backend.
    #[must_use]
    pub fn storage(mut self, storage: S) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Build the driver and handle.
    ///
    /// Returns:
    /// - [`RaftDriver`] to run with `.run()` in a tokio task
    /// - [`RaftHandle`] for proposing commands and querying state
    /// - [`ShutdownSender`] to signal graceful shutdown
    /// - Inbound sender for feeding messages from other nodes
    /// - Outbound receiver for consuming messages to send to other nodes
    ///
    /// # Panics
    ///
    /// Panics if `config` or `storage` was not set.
    pub async fn build(
        self,
    ) -> Result<
        (
            RaftDriver<T, S>,
            RaftHandle<T>,
            ShutdownSender,
            mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
            mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
        ),
        super::storage::StorageError,
    > {
        let config = self.config.expect("RaftBuilder: config is required");
        let storage = self.storage.expect("RaftBuilder: storage is required");

        let (node, outbound_rx) = RaftNode::new(config, storage).await?;

        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (proposal_tx, proposal_rx) = mpsc::unbounded_channel();
        let (commit_tx, commit_rx) = mpsc::unbounded_channel();
        let (outbound_fwd, outbound_ext_rx) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_signal) = shutdown_channel();

        let initial_state = (node.state(), node.current_term(), node.current_leader());
        let (state_tx, state_rx) = watch::channel(initial_state);

        let driver = RaftDriver {
            node,
            inbound_rx,
            outbound_rx,
            outbound_fwd,
            proposal_rx,
            commit_tx,
            state_tx,
            shutdown: shutdown_signal,
            election_reset: false,
        };

        let handle = RaftHandle {
            proposal_tx,
            commit_rx,
            state_rx,
        };

        Ok((driver, handle, shutdown_sender, inbound_tx, outbound_ext_rx))
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod driver_tests;
