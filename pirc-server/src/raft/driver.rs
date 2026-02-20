use std::collections::HashMap;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{self, Instant};
use tracing::{debug, info, warn};

use super::health::{HealthConfig, HealthEvent, NodeHealthMonitor, PeerStatus};
use super::membership::{MembershipChange, MembershipError};
use super::node::RaftNode;
use super::rpc::RaftMessage;
use super::snapshot::StateMachine;
use super::storage::RaftStorage;
use super::types::{LogEntry, LogIndex, NodeId, RaftConfig, RaftState, Term};

/// A membership change proposal sent from [`RaftHandle`] to [`RaftDriver`].
///
/// Includes a oneshot channel to notify the caller of the result.
pub(crate) struct MembershipProposal<T> {
    pub change: MembershipChange,
    pub noop_command: T,
    pub respond: oneshot::Sender<Result<LogIndex, MembershipError>>,
}

/// A handle for interacting with a running Raft node.
///
/// Uses channels and watch to communicate with the driver loop.
pub struct RaftHandle<T: Send + 'static> {
    proposal_tx: mpsc::UnboundedSender<T>,
    membership_tx: mpsc::UnboundedSender<MembershipProposal<T>>,
    commit_rx: mpsc::UnboundedReceiver<LogEntry<T>>,
    state_rx: watch::Receiver<(RaftState, Term, Option<NodeId>)>,
    health_event_rx: mpsc::UnboundedReceiver<HealthEvent>,
    peer_status_rx: watch::Receiver<HashMap<NodeId, PeerStatus>>,
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

    /// Subscribe to Raft state changes.
    ///
    /// Returns a cloned watch receiver that yields `(RaftState, Term, Option<NodeId>)`
    /// whenever the Raft driver publishes a state update.
    pub fn subscribe_state(&self) -> watch::Receiver<(RaftState, Term, Option<NodeId>)> {
        self.state_rx.clone()
    }

    /// Propose a membership change to the Raft cluster.
    ///
    /// Sends the change to the driver loop, which applies it via
    /// [`RaftNode::propose_membership_change`]. Returns the log index of
    /// the membership entry on success, or a [`MembershipError`] on failure.
    ///
    /// The `noop_command` is the log entry payload that carries the
    /// membership change (the actual membership metadata is tracked
    /// internally by the Raft node).
    pub async fn propose_membership_change(
        &self,
        change: MembershipChange,
        noop_command: T,
    ) -> Result<LogIndex, MembershipError> {
        let (tx, rx) = oneshot::channel();
        let proposal = MembershipProposal {
            change,
            noop_command,
            respond: tx,
        };
        self.membership_tx
            .send(proposal)
            .map_err(|_| MembershipError::NotLeader)?;
        rx.await.unwrap_or(Err(MembershipError::NotLeader))
    }

    /// Take the commit receiver to consume committed log entries.
    ///
    /// This can only be called once; subsequent calls return an empty receiver.
    pub fn take_commit_rx(&mut self) -> mpsc::UnboundedReceiver<LogEntry<T>> {
        let (_, empty_rx) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.commit_rx, empty_rx)
    }

    /// Take the health event receiver to consume peer status change events.
    ///
    /// This can only be called once; subsequent calls return an empty receiver.
    pub fn take_health_event_rx(&mut self) -> mpsc::UnboundedReceiver<HealthEvent> {
        let (_, empty_rx) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.health_event_rx, empty_rx)
    }

    /// Get the current peer statuses as reported by the health monitor.
    ///
    /// Returns an empty map when this node is not the leader.
    pub fn peer_statuses(&self) -> HashMap<NodeId, PeerStatus> {
        self.peer_status_rx.borrow().clone()
    }

    /// Create a handle from raw channels (for testing only).
    #[cfg(test)]
    #[allow(private_interfaces)]
    pub fn new_for_test(
        proposal_tx: mpsc::UnboundedSender<T>,
        membership_tx: mpsc::UnboundedSender<MembershipProposal<T>>,
        commit_rx: mpsc::UnboundedReceiver<LogEntry<T>>,
        state_rx: watch::Receiver<(RaftState, Term, Option<NodeId>)>,
        health_event_rx: mpsc::UnboundedReceiver<HealthEvent>,
        peer_status_rx: watch::Receiver<HashMap<NodeId, PeerStatus>>,
    ) -> Self {
        Self {
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        }
    }
}

/// Errors returned by [`RaftHandle`] operations.
#[derive(Debug, thiserror::Error)]
pub enum RaftError {
    #[error("not the leader")]
    NotLeader,
    #[error("raft driver has shut down")]
    Shutdown,
    #[error("membership change failed: {0}")]
    Membership(#[from] MembershipError),
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
    /// The watch channel sender for signalling shutdown.
    pub tx: watch::Sender<bool>,
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
pub struct RaftDriver<T, S, M>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
    M: StateMachine<T>,
{
    node: RaftNode<T, S>,
    /// Application-level state machine for applying commands and snapshots.
    state_machine: M,
    /// Receives messages from other Raft nodes (delivered by the transport layer).
    inbound_rx: mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
    /// Receives outbound messages from the node (to be delivered by transport).
    outbound_rx: mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
    /// Forwards outbound messages to the transport layer.
    outbound_fwd: mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
    /// Client proposals to be appended to the log.
    proposal_rx: mpsc::UnboundedReceiver<T>,
    /// Membership change proposals from the handle.
    membership_rx: mpsc::UnboundedReceiver<MembershipProposal<T>>,
    /// Sender for committed entries (consumed by the application).
    commit_tx: mpsc::UnboundedSender<LogEntry<T>>,
    /// Publishes state changes for external queries.
    state_tx: watch::Sender<(RaftState, Term, Option<NodeId>)>,
    /// Graceful shutdown signal.
    shutdown: ShutdownSignal,
    /// Whether an election timeout reset was requested.
    election_reset: bool,
    /// Health monitor for tracking peer liveness (active only when leader).
    health_monitor: Option<NodeHealthMonitor>,
    /// Forwards health events from the monitor to the handle.
    health_event_fwd: mpsc::UnboundedSender<HealthEvent>,
    /// Publishes peer status snapshots for external queries.
    peer_status_tx: watch::Sender<HashMap<NodeId, PeerStatus>>,
    /// Configuration for health monitoring thresholds.
    health_config: HealthConfig,
}

impl<T, S, M> RaftDriver<T, S, M>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
    M: StateMachine<T>,
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
                    let is_leader = self.node.state() == RaftState::Leader;
                    // If we just became leader, reset the heartbeat interval
                    // and start the health monitor.
                    if !was_leader && is_leader {
                        heartbeat_tick.reset();
                        self.start_health_monitor();
                    }
                    // If we just lost leadership, stop the health monitor.
                    if was_leader && !is_leader {
                        self.stop_health_monitor();
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

                // Membership change proposal.
                Some(proposal) = self.membership_rx.recv() => {
                    self.handle_membership_proposal(proposal);
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

            // Trigger snapshot if log exceeds threshold.
            self.maybe_snapshot().await;
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
        // Evaluate peer health on each heartbeat tick.
        if let Some(ref mut monitor) = self.health_monitor {
            monitor.evaluate();
            if monitor.is_dirty() {
                let _ = self.peer_status_tx.send(monitor.peer_statuses());
                monitor.clear_dirty();
            }
        }
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
                // Any response (including success=false for log mismatch) proves
                // the peer is alive and should reset its health timer.
                if let Some(ref mut monitor) = self.health_monitor {
                    monitor.record_response(from);
                }
            }
            RaftMessage::InstallSnapshot(req) => self.handle_install_snapshot(from, req).await,
            RaftMessage::InstallSnapshotResponse(resp) => {
                if let Err(e) = self
                    .node
                    .handle_install_snapshot_response(from, resp)
                    .await
                {
                    warn!(
                        node = %self.node.node_id(),
                        from = %from,
                        error = %e,
                        "error handling InstallSnapshotResponse"
                    );
                }
                // Any snapshot response means the peer is alive.
                if let Some(ref mut monitor) = self.health_monitor {
                    monitor.record_response(from);
                }
            }
        }
    }

    async fn handle_install_snapshot(
        &mut self,
        from: NodeId,
        req: super::snapshot::InstallSnapshot,
    ) {
        let req_term = req.term;
        match self
            .node
            .handle_install_snapshot(req, &mut self.state_machine)
            .await
        {
            Ok(resp) => {
                if req_term >= self.node.current_term() {
                    self.election_reset = true;
                }
                let _ = self
                    .node
                    .outbound
                    .send((from, RaftMessage::InstallSnapshotResponse(resp)));
            }
            Err(e) => {
                warn!(
                    node = %self.node.node_id(),
                    from = %from,
                    error = %e,
                    "error handling InstallSnapshot"
                );
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

    fn handle_membership_proposal(&mut self, proposal: MembershipProposal<T>) {
        let result = self
            .node
            .propose_membership_change(proposal.change, proposal.noop_command);

        match &result {
            Ok(index) => {
                debug!(
                    node = %self.node.node_id(),
                    index = %index,
                    "membership change proposed"
                );
                // Check if we can immediately commit (e.g. solo node).
                self.node.advance_commit_index();
            }
            Err(e) => {
                debug!(
                    node = %self.node.node_id(),
                    error = %e,
                    "membership change proposal rejected"
                );
            }
        }

        let _ = proposal.respond.send(result);
    }

    fn apply_committed_entries(&mut self) {
        let last_applied = self.node.volatile_state().last_applied;
        let commit_index = self.node.volatile_state().commit_index;

        if last_applied >= commit_index {
            return;
        }

        // Apply entries via the state machine and collect them in a single pass.
        let commit_tx = &self.commit_tx;
        self.node.apply_committed_with_entries(|cmd| {
            self.state_machine.apply(cmd);
        }, |entry| {
            let _ = commit_tx.send(entry);
        });
    }

    async fn maybe_snapshot(&mut self) {
        if self.node.should_snapshot() {
            if let Err(e) = self.node.create_snapshot(&self.state_machine).await {
                warn!(
                    node = %self.node.node_id(),
                    error = %e,
                    "failed to create snapshot"
                );
            }
        }
    }

    fn publish_state(&self) {
        let new_state = (
            self.node.state(),
            self.node.current_term(),
            self.node.current_leader(),
        );
        // Only send if the state actually changed (watch::Sender skips if unchanged).
        self.state_tx.send_if_modified(|current| {
            if *current == new_state {
                false
            } else {
                *current = new_state;
                true
            }
        });
    }

    /// Initialize the health monitor when this node becomes leader.
    fn start_health_monitor(&mut self) {
        let peers = self.node.membership().peers(self.node.node_id());
        let (monitor, mut event_rx) = NodeHealthMonitor::new(&peers, self.health_config.clone());
        self.health_monitor = Some(monitor);

        // Drain any events from the monitor's channel into our forwarding channel.
        let fwd = self.health_event_fwd.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if fwd.send(event).is_err() {
                    break;
                }
            }
        });

        info!(
            node = %self.node.node_id(),
            peers = peers.len(),
            "health monitor started"
        );
    }

    /// Stop the health monitor when this node loses leadership.
    fn stop_health_monitor(&mut self) {
        if self.health_monitor.take().is_some() {
            // Publish empty statuses since we're no longer monitoring.
            let _ = self.peer_status_tx.send(HashMap::new());
            info!(
                node = %self.node.node_id(),
                "health monitor stopped"
            );
        }
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
pub struct RaftBuilder<T, S, M>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
    M: StateMachine<T>,
{
    config: Option<RaftConfig>,
    storage: Option<S>,
    state_machine: Option<M>,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S, M> Default for RaftBuilder<T, S, M>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
    M: StateMachine<T>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, S, M> RaftBuilder<T, S, M>
where
    T: Clone + PartialEq + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    S: RaftStorage<T>,
    M: StateMachine<T>,
{
    pub fn new() -> Self {
        Self {
            config: None,
            storage: None,
            state_machine: None,
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

    /// Set the state machine.
    #[must_use]
    pub fn state_machine(mut self, sm: M) -> Self {
        self.state_machine = Some(sm);
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
    /// Panics if `config`, `storage`, or `state_machine` was not set.
    pub async fn build(
        self,
    ) -> Result<
        (
            RaftDriver<T, S, M>,
            RaftHandle<T>,
            ShutdownSender,
            mpsc::UnboundedSender<(NodeId, RaftMessage<T>)>,
            mpsc::UnboundedReceiver<(NodeId, RaftMessage<T>)>,
        ),
        super::storage::StorageError,
    > {
        let config = self.config.expect("RaftBuilder: config is required");
        let storage = self.storage.expect("RaftBuilder: storage is required");
        let state_machine = self
            .state_machine
            .expect("RaftBuilder: state_machine is required");

        let health_config = HealthConfig::from_heartbeat_interval(config.heartbeat_interval);
        let (node, outbound_rx) = RaftNode::new(config, storage).await?;

        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (proposal_tx, proposal_rx) = mpsc::unbounded_channel();
        let (membership_tx, membership_rx) = mpsc::unbounded_channel();
        let (commit_tx, commit_rx) = mpsc::unbounded_channel();
        let (outbound_fwd, outbound_ext_rx) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_signal) = shutdown_channel();
        let (health_event_fwd, health_event_rx) = mpsc::unbounded_channel();
        let (peer_status_tx, peer_status_rx) = watch::channel(HashMap::new());

        let initial_state = (node.state(), node.current_term(), node.current_leader());
        let (state_tx, state_rx) = watch::channel(initial_state);

        let driver = RaftDriver {
            node,
            state_machine,
            inbound_rx,
            outbound_rx,
            outbound_fwd,
            proposal_rx,
            membership_rx,
            commit_tx,
            state_tx,
            shutdown: shutdown_signal,
            election_reset: false,
            health_monitor: None,
            health_event_fwd,
            peer_status_tx,
            health_config,
        };

        let handle = RaftHandle {
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        };

        Ok((driver, handle, shutdown_sender, inbound_tx, outbound_ext_rx))
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod driver_tests;
