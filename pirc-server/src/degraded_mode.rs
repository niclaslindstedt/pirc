use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::handler::SERVER_NAME;
use crate::raft::types::{NodeId, RaftState, Term};
use crate::registry::UserRegistry;

/// Shared degraded-mode state accessible from connection handlers.
///
/// When the Raft cluster loses quorum (no leader elected), the server enters
/// degraded mode: local mutations continue but are not replicated. Clients
/// are notified via server NOTICE when the mode changes.
///
/// Single-node clusters never enter degraded mode because the solo node is
/// always its own leader.
pub struct DegradedModeState {
    degraded: AtomicBool,
}

impl Default for DegradedModeState {
    fn default() -> Self {
        Self::new()
    }
}

impl DegradedModeState {
    pub fn new() -> Self {
        Self {
            degraded: AtomicBool::new(false),
        }
    }

    /// Returns `true` when the cluster has no leader (quorum lost).
    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    fn set_degraded(&self, value: bool) {
        self.degraded.store(value, Ordering::Relaxed);
    }
}

/// Shared reference to degraded-mode state.
pub type SharedDegradedState = Arc<DegradedModeState>;

/// Spawn a background task that monitors the Raft state watch channel
/// and detects transitions into and out of degraded mode (no leader).
///
/// When the cluster is a single-node cluster (no peers), degraded mode is
/// never entered because the solo node is always the leader.
///
/// Returns a `JoinHandle` so the caller can track the task lifetime.
pub fn spawn_degraded_mode_monitor(
    mut state_rx: watch::Receiver<(RaftState, Term, Option<NodeId>)>,
    registry: Arc<UserRegistry>,
    degraded_state: SharedDegradedState,
    is_single_node: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Track whether a leader was known in the previous observation.
        let mut prev_leader_known = {
            let current = state_rx.borrow();
            current.2.is_some()
        };

        loop {
            // Wait for the next state change.
            if state_rx.changed().await.is_err() {
                // Channel closed — Raft driver shut down.
                info!("degraded mode monitor shutting down: state channel closed");
                break;
            }

            let (raft_state, _term, leader) = *state_rx.borrow_and_update();
            let leader_known = leader.is_some();

            // Single-node clusters should never be degraded; the solo node
            // always elects itself. Skip any spurious transitions.
            if is_single_node {
                continue;
            }

            if prev_leader_known && !leader_known {
                // Transition: normal -> degraded.
                warn!(
                    raft_state = %raft_state,
                    "cluster quorum lost — entering degraded mode"
                );
                degraded_state.set_degraded(true);
                broadcast_notice(
                    &registry,
                    "*** Cluster quorum lost — operating in degraded mode. \
                     State changes are local only and will be reconciled when quorum is restored.",
                );
            } else if !prev_leader_known && leader_known {
                // Transition: degraded -> normal.
                if degraded_state.is_degraded() {
                    info!(
                        leader = leader.map(NodeId::as_u64),
                        "cluster quorum restored — leaving degraded mode"
                    );
                    degraded_state.set_degraded(false);
                    broadcast_notice(
                        &registry,
                        "*** Cluster quorum restored — state is being reconciled via Raft log replay.",
                    );
                }
            }

            prev_leader_known = leader_known;
        }
    })
}

/// Send a server NOTICE to all connected clients.
fn broadcast_notice(registry: &UserRegistry, text: &str) {
    let msg = Message::builder(Command::Notice)
        .prefix(Prefix::server(SERVER_NAME))
        .param("*")
        .trailing(text)
        .build();

    for session_arc in registry.iter_sessions() {
        let session = session_arc.read().expect("session lock poisoned");
        let _ = session.sender.send(msg.clone());
    }
}

#[cfg(test)]
#[path = "degraded_mode_tests.rs"]
mod tests;
