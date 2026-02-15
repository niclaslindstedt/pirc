use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::types::NodeId;

/// Status of a peer node as observed by the health monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PeerStatus {
    /// The peer is responding to heartbeats normally.
    Online,
    /// The peer has not responded within the suspect threshold but is not
    /// yet considered down.
    Suspected,
    /// The peer has not responded for longer than the failure threshold
    /// and is considered offline.
    Down,
}

impl std::fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "Online"),
            Self::Suspected => write!(f, "Suspected"),
            Self::Down => write!(f, "Down"),
        }
    }
}

/// Events emitted when a peer's health status changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthEvent {
    /// A peer is now suspected of being offline.
    NodeSuspected(NodeId),
    /// A peer has been confirmed as down (no response beyond failure threshold).
    NodeDown(NodeId),
    /// A previously down or suspected peer is back online.
    NodeUp(NodeId),
}

/// Configuration for the health monitor's failure detection thresholds.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Duration after last heartbeat response before a peer is suspected.
    /// Default: 3x heartbeat interval.
    pub suspect_threshold: Duration,
    /// Duration after last heartbeat response before a peer is marked down.
    /// Default: 6x heartbeat interval.
    pub failure_threshold: Duration,
}

impl HealthConfig {
    /// Create a health config derived from the heartbeat interval.
    ///
    /// Uses 3x heartbeat for suspect and 6x for failure thresholds.
    pub fn from_heartbeat_interval(heartbeat: Duration) -> Self {
        Self {
            suspect_threshold: heartbeat * 3,
            failure_threshold: heartbeat * 6,
        }
    }
}

/// Per-peer tracking state within the health monitor.
#[derive(Debug, Clone)]
struct PeerState {
    status: PeerStatus,
    last_seen: Instant,
}

/// Monitors the health of peer nodes by tracking heartbeat responses.
///
/// The monitor maintains a status for each known peer and emits
/// [`HealthEvent`]s when status transitions occur. It is designed to
/// be driven by the Raft driver: the driver notifies the monitor
/// when a peer responds, and periodically asks it to re-evaluate
/// peer statuses.
pub struct NodeHealthMonitor {
    config: HealthConfig,
    peers: HashMap<NodeId, PeerState>,
    event_tx: mpsc::UnboundedSender<HealthEvent>,
}

impl NodeHealthMonitor {
    /// Create a new health monitor for the given peers.
    ///
    /// All peers start in `Online` status with `last_seen` set to now.
    /// Health events are sent on the returned receiver.
    pub fn new(
        peer_ids: &[NodeId],
        config: HealthConfig,
    ) -> (Self, mpsc::UnboundedReceiver<HealthEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let now = Instant::now();
        let peers = peer_ids
            .iter()
            .map(|&id| {
                (
                    id,
                    PeerState {
                        status: PeerStatus::Online,
                        last_seen: now,
                    },
                )
            })
            .collect();

        (
            Self {
                config,
                peers,
                event_tx,
            },
            event_rx,
        )
    }

    /// Record that a peer has responded (e.g. to an `AppendEntries` RPC).
    ///
    /// If the peer was previously suspected or down, emits a `NodeUp` event.
    pub fn record_response(&mut self, peer_id: NodeId) {
        let now = Instant::now();
        if let Some(peer) = self.peers.get_mut(&peer_id) {
            let old_status = peer.status;
            peer.last_seen = now;
            peer.status = PeerStatus::Online;

            if old_status != PeerStatus::Online {
                let _ = self.event_tx.send(HealthEvent::NodeUp(peer_id));
            }
        }
    }

    /// Evaluate all peers and emit events for any status transitions.
    ///
    /// Should be called periodically (e.g. on each heartbeat tick).
    pub fn evaluate(&mut self) {
        let now = Instant::now();

        for (&peer_id, peer) in &mut self.peers {
            let elapsed = now.duration_since(peer.last_seen);
            let old_status = peer.status;

            let new_status = if elapsed >= self.config.failure_threshold {
                PeerStatus::Down
            } else if elapsed >= self.config.suspect_threshold {
                PeerStatus::Suspected
            } else {
                PeerStatus::Online
            };

            if new_status != old_status {
                peer.status = new_status;
                let event = match new_status {
                    PeerStatus::Down => HealthEvent::NodeDown(peer_id),
                    PeerStatus::Suspected => HealthEvent::NodeSuspected(peer_id),
                    PeerStatus::Online => HealthEvent::NodeUp(peer_id),
                };
                let _ = self.event_tx.send(event);
            }
        }
    }

    /// Get the current status of all tracked peers.
    pub fn peer_statuses(&self) -> HashMap<NodeId, PeerStatus> {
        self.peers
            .iter()
            .map(|(&id, state)| (id, state.status))
            .collect()
    }

    /// Get the status of a specific peer.
    pub fn peer_status(&self, peer_id: NodeId) -> Option<PeerStatus> {
        self.peers.get(&peer_id).map(|s| s.status)
    }

    /// Add a new peer to the monitor with `Online` status.
    pub fn add_peer(&mut self, peer_id: NodeId) {
        self.peers.entry(peer_id).or_insert_with(|| PeerState {
            status: PeerStatus::Online,
            last_seen: Instant::now(),
        });
    }

    /// Remove a peer from the monitor.
    pub fn remove_peer(&mut self, peer_id: NodeId) {
        self.peers.remove(&peer_id);
    }

    /// Return the list of peers currently marked as `Down`.
    pub fn down_peers(&self) -> Vec<NodeId> {
        self.peers
            .iter()
            .filter(|(_, s)| s.status == PeerStatus::Down)
            .map(|(&id, _)| id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(suspect_ms: u64, failure_ms: u64) -> HealthConfig {
        HealthConfig {
            suspect_threshold: Duration::from_millis(suspect_ms),
            failure_threshold: Duration::from_millis(failure_ms),
        }
    }

    #[test]
    fn peer_status_display() {
        assert_eq!(PeerStatus::Online.to_string(), "Online");
        assert_eq!(PeerStatus::Suspected.to_string(), "Suspected");
        assert_eq!(PeerStatus::Down.to_string(), "Down");
    }

    #[test]
    fn new_monitor_starts_all_peers_online() {
        let peers = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        let config = make_config(100, 200);
        let (monitor, _rx) = NodeHealthMonitor::new(&peers, config);

        let statuses = monitor.peer_statuses();
        assert_eq!(statuses.len(), 3);
        for status in statuses.values() {
            assert_eq!(*status, PeerStatus::Online);
        }
    }

    #[test]
    fn evaluate_no_change_when_recent() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(100, 200);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        monitor.evaluate();
        // No events should be emitted — peer was just created
        assert!(rx.try_recv().is_err());
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Online)
        );
    }

    #[tokio::test]
    async fn evaluate_transitions_to_suspected() {
        let peers = vec![NodeId::new(1)];
        // Very short thresholds for testing
        let config = make_config(10, 50);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Wait past suspect threshold but not failure
        tokio::time::sleep(Duration::from_millis(20)).await;
        monitor.evaluate();

        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Suspected)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeSuspected(NodeId::new(1)));
    }

    #[tokio::test]
    async fn evaluate_transitions_to_down() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(10, 30);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Wait past failure threshold
        tokio::time::sleep(Duration::from_millis(40)).await;
        monitor.evaluate();

        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Down)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeDown(NodeId::new(1)));
    }

    #[tokio::test]
    async fn record_response_resets_to_online() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(10, 30);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Wait past failure threshold to get Down status
        tokio::time::sleep(Duration::from_millis(40)).await;
        monitor.evaluate();
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Down)
        );
        // Drain the NodeDown event
        let _ = rx.try_recv().unwrap();

        // Now record a response — should transition back to Online
        monitor.record_response(NodeId::new(1));
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Online)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeUp(NodeId::new(1)));
    }

    #[tokio::test]
    async fn no_event_when_online_peer_responds() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(100, 200);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Record a response while already online — no event
        monitor.record_response(NodeId::new(1));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn suspected_peer_recovers_on_response() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(10, 50);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Go past suspect threshold
        tokio::time::sleep(Duration::from_millis(20)).await;
        monitor.evaluate();
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Suspected)
        );
        let _ = rx.try_recv().unwrap(); // Drain NodeSuspected

        // Respond and recover
        monitor.record_response(NodeId::new(1));
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Online)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeUp(NodeId::new(1)));
    }

    #[test]
    fn add_peer_starts_online() {
        let config = make_config(100, 200);
        let (mut monitor, _rx) = NodeHealthMonitor::new(&[], config);

        monitor.add_peer(NodeId::new(5));
        assert_eq!(
            monitor.peer_status(NodeId::new(5)),
            Some(PeerStatus::Online)
        );
    }

    #[test]
    fn remove_peer_removes_tracking() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(100, 200);
        let (mut monitor, _rx) = NodeHealthMonitor::new(&peers, config);

        monitor.remove_peer(NodeId::new(1));
        assert_eq!(monitor.peer_status(NodeId::new(1)), None);
        assert!(monitor.peer_statuses().is_empty());
    }

    #[tokio::test]
    async fn down_peers_returns_only_down_nodes() {
        let peers = vec![NodeId::new(1), NodeId::new(2)];
        let config = make_config(10, 30);
        let (mut monitor, _rx) = NodeHealthMonitor::new(&peers, config);

        // Wait past failure threshold
        tokio::time::sleep(Duration::from_millis(40)).await;

        // Keep node 2 alive
        monitor.record_response(NodeId::new(2));
        monitor.evaluate();

        let down = monitor.down_peers();
        assert_eq!(down.len(), 1);
        assert_eq!(down[0], NodeId::new(1));
    }

    #[test]
    fn multiple_evaluations_no_duplicate_events() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(100, 200);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Multiple evaluations while online should emit nothing
        monitor.evaluate();
        monitor.evaluate();
        monitor.evaluate();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn suspected_to_down_emits_down_event() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(10, 30);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Go to suspected
        tokio::time::sleep(Duration::from_millis(15)).await;
        monitor.evaluate();
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Suspected)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeSuspected(NodeId::new(1)));

        // Go to down
        tokio::time::sleep(Duration::from_millis(20)).await;
        monitor.evaluate();
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Down)
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event, HealthEvent::NodeDown(NodeId::new(1)));
    }

    #[tokio::test]
    async fn down_stays_down_without_response() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(10, 30);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&peers, config);

        // Go directly to down
        tokio::time::sleep(Duration::from_millis(40)).await;
        monitor.evaluate();
        let _ = rx.try_recv().unwrap(); // Drain NodeDown

        // Another evaluation — stays Down, no new event
        tokio::time::sleep(Duration::from_millis(10)).await;
        monitor.evaluate();
        assert_eq!(
            monitor.peer_status(NodeId::new(1)),
            Some(PeerStatus::Down)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn health_config_from_heartbeat_interval() {
        let config = HealthConfig::from_heartbeat_interval(Duration::from_millis(50));
        assert_eq!(config.suspect_threshold, Duration::from_millis(150));
        assert_eq!(config.failure_threshold, Duration::from_millis(300));
    }

    #[test]
    fn record_response_unknown_peer_is_noop() {
        let config = make_config(100, 200);
        let (mut monitor, mut rx) = NodeHealthMonitor::new(&[], config);

        // Should not panic or emit events
        monitor.record_response(NodeId::new(99));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn add_peer_idempotent() {
        let peers = vec![NodeId::new(1)];
        let config = make_config(100, 200);
        let (mut monitor, _rx) = NodeHealthMonitor::new(&peers, config);

        // Adding an existing peer should not reset its state
        monitor.add_peer(NodeId::new(1));
        assert_eq!(monitor.peer_statuses().len(), 1);
    }
}
