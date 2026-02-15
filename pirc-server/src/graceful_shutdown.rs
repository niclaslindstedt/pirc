use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_protocol::{Command, Message, Prefix};
use tracing::{debug, info, warn};

use crate::handler::SERVER_NAME;
use crate::migration::SharedUserNodeIndex;
use crate::raft::transport::SharedPeerMap;
use crate::raft::types::NodeId;
use crate::raft::{ClusterCommand, MembershipChange, RaftHandle};
use crate::registry::UserRegistry;

/// Default timeout for waiting on migration commands to commit.
const MIGRATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Orchestrates graceful server shutdown with user pre-migration.
///
/// When triggered, this handler:
/// 1. Proposes `UserMigrated` commands for all locally-connected users
/// 2. Sends server notices to clients with redirect addresses
/// 3. Waits for migration to commit (with timeout)
/// 4. Removes self from Raft membership
/// 5. Signals Raft driver shutdown
pub struct GracefulShutdown {
    raft_handle: Arc<RaftHandle<ClusterCommand>>,
    registry: Arc<UserRegistry>,
    user_node_index: SharedUserNodeIndex,
    shared_peer_map: SharedPeerMap,
    self_id: NodeId,
    peer_ids: Vec<NodeId>,
    raft_shutdown: Option<crate::raft::ShutdownSender>,
}

/// Result of running the graceful shutdown sequence.
#[derive(Debug)]
pub struct ShutdownResult {
    /// Number of users whose migration was proposed.
    pub users_migrated: usize,
    /// Whether migration proposals were committed before the timeout.
    pub migration_committed: bool,
    /// Whether membership removal was successful.
    pub membership_removed: bool,
}

impl GracefulShutdown {
    /// Create a new graceful shutdown handler.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        raft_handle: Arc<RaftHandle<ClusterCommand>>,
        registry: Arc<UserRegistry>,
        user_node_index: SharedUserNodeIndex,
        shared_peer_map: SharedPeerMap,
        self_id: NodeId,
        peer_ids: Vec<NodeId>,
        raft_shutdown: crate::raft::ShutdownSender,
    ) -> Self {
        Self {
            raft_handle,
            registry,
            user_node_index,
            shared_peer_map,
            self_id,
            peer_ids,
            raft_shutdown: Some(raft_shutdown),
        }
    }

    /// Execute the graceful shutdown sequence.
    ///
    /// This is the main entry point. It runs the full shutdown protocol
    /// with a timeout fallback for migration.
    pub async fn execute(&mut self) -> ShutdownResult {
        info!(
            node = self.self_id.as_u64(),
            "initiating graceful shutdown with pre-migration"
        );

        // Step 1: Determine target nodes for migration.
        let targets = self.select_migration_targets();
        if targets.is_empty() {
            info!("no peer nodes available for migration, proceeding with shutdown");
            self.shutdown_raft();
            return ShutdownResult {
                users_migrated: 0,
                migration_committed: false,
                membership_removed: false,
            };
        }

        // Step 2: Propose user migrations and notify clients.
        let users_migrated = self.propose_migrations_and_notify(&targets).await;

        // Step 3: Wait for migrations to commit (with timeout).
        let migration_committed = if users_migrated > 0 {
            self.wait_for_migrations(users_migrated).await
        } else {
            true
        };

        if !migration_committed {
            warn!(
                "migration timeout expired after {:?}, proceeding with shutdown",
                MIGRATION_TIMEOUT
            );
        }

        // Step 4: Remove self from Raft membership.
        let membership_removed = self.remove_self_from_membership().await;

        // Step 5: Signal Raft driver shutdown.
        self.shutdown_raft();

        info!(
            users_migrated,
            migration_committed,
            membership_removed,
            "graceful shutdown sequence complete"
        );

        ShutdownResult {
            users_migrated,
            migration_committed,
            membership_removed,
        }
    }

    /// Select available peer nodes to distribute users to.
    fn select_migration_targets(&self) -> Vec<(NodeId, SocketAddr)> {
        let peer_map = self.shared_peer_map.try_read().expect("peer_map lock");
        let mut targets: Vec<(NodeId, SocketAddr)> = self
            .peer_ids
            .iter()
            .filter_map(|&id| peer_map.get(id).map(|addr| (id, *addr)))
            .collect();
        targets.sort_by_key(|(id, _)| id.as_u64());
        targets
    }

    /// Propose `UserMigrated` commands and send server notices to clients.
    ///
    /// Returns the number of users whose migration was proposed.
    async fn propose_migrations_and_notify(
        &self,
        targets: &[(NodeId, SocketAddr)],
    ) -> usize {
        // Get users homed to this node from the user-node index.
        let local_users = {
            let index = self.user_node_index.read().await;
            index.users_on_node(self.self_id)
        };

        if local_users.is_empty() {
            info!("no local users to migrate");
            return 0;
        }

        info!(
            user_count = local_users.len(),
            target_count = targets.len(),
            "migrating local users to peer nodes"
        );

        let mut migrated = 0;

        for (i, nickname) in local_users.iter().enumerate() {
            let (target_id, target_addr) = targets[i % targets.len()];

            // Propose the migration through Raft.
            let cmd = ClusterCommand::UserMigrated {
                nickname: nickname.clone(),
                from_node: self.self_id,
                to_node: target_id,
            };

            if let Err(e) = self.raft_handle.propose(cmd) {
                warn!(
                    nickname,
                    error = %e,
                    "failed to propose user migration during shutdown"
                );
                continue;
            }

            debug!(
                nickname,
                to_node = target_id.as_u64(),
                "proposed user migration for shutdown"
            );

            // Send server notice to the client with redirect info.
            self.send_redirect_notice(nickname, target_addr);

            migrated += 1;
        }

        info!(
            migrated,
            total = local_users.len(),
            "user migration proposals submitted"
        );
        migrated
    }

    /// Send a server notice to a client telling them to reconnect elsewhere.
    fn send_redirect_notice(&self, nickname: &str, target_addr: SocketAddr) {
        let Ok(nick) = pirc_common::Nickname::new(nickname) else {
            return;
        };

        if let Some(session_arc) = self.registry.get_by_nick(&nick) {
            let session = session_arc.read().expect("session lock poisoned");

            // Send NOTICE about the shutdown and redirect.
            let notice = Message::builder(Command::Notice)
                .prefix(Prefix::server(SERVER_NAME))
                .param(nickname)
                .trailing(&format!(
                    "*** Server shutting down. Please reconnect to {target_addr}"
                ))
                .build();
            let _ = session.sender.send(notice);

            // Also send an ERROR message before the connection closes.
            let error_msg = Message::builder(Command::Error)
                .trailing(&format!(
                    "Closing Link: {} (Server shutting down, reconnect to {target_addr})",
                    session.hostname
                ))
                .build();
            let _ = session.sender.send(error_msg);
        }
    }

    /// Wait for migration proposals to be committed.
    ///
    /// Uses a simple timeout-based approach: we wait up to [`MIGRATION_TIMEOUT`]
    /// for the user-node index to reflect that users have been migrated away.
    /// Returns `true` if all migrations committed, `false` on timeout.
    async fn wait_for_migrations(&self, expected_count: usize) -> bool {
        let deadline = tokio::time::Instant::now() + MIGRATION_TIMEOUT;
        let check_interval = Duration::from_millis(100);

        loop {
            // Check if all local users have been migrated away.
            let remaining = {
                let index = self.user_node_index.read().await;
                index.users_on_node(self.self_id).len()
            };

            if remaining == 0 {
                info!(
                    expected_count,
                    "all user migrations committed successfully"
                );
                return true;
            }

            if tokio::time::Instant::now() >= deadline {
                warn!(
                    remaining,
                    expected_count,
                    "migration timeout: some users still on this node"
                );
                return false;
            }

            debug!(
                remaining,
                "waiting for migration commits..."
            );
            tokio::time::sleep(check_interval).await;
        }
    }

    /// Remove this node from the Raft cluster membership.
    async fn remove_self_from_membership(&self) -> bool {
        if !self.raft_handle.is_leader() {
            info!("not leader, skipping membership self-removal");
            return false;
        }

        let change = MembershipChange::RemoveServer(self.self_id);
        let noop = ClusterCommand::Noop {
            description: format!("remove-server:{}", self.self_id.as_u64()),
        };

        match self.raft_handle.propose_membership_change(change, noop).await {
            Ok(index) => {
                info!(
                    index = index.as_u64(),
                    "proposed self-removal from cluster membership"
                );
                true
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "failed to remove self from cluster membership"
                );
                false
            }
        }
    }

    /// Signal the Raft driver to shut down.
    fn shutdown_raft(&mut self) {
        if let Some(sender) = self.raft_shutdown.take() {
            info!("signalling Raft driver shutdown");
            sender.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use pirc_common::Nickname;
    use pirc_protocol::Message;
    use tokio::sync::{mpsc, RwLock};
    use tokio::time::Instant;

    use crate::migration::UserNodeIndex;
    use crate::raft::transport::PeerMap;
    use crate::raft::types::NodeId;
    use crate::registry::UserRegistry;
    use crate::user::UserSession;

    use super::*;

    /// Helper: create a `UserSession` with a working sender.
    fn make_session(
        conn_id: u64,
        nick: &str,
    ) -> (UserSession, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let now = Instant::now();
        let session = UserSession {
            connection_id: conn_id,
            nickname: Nickname::new(nick).unwrap(),
            username: format!("user{conn_id}"),
            realname: format!("Real Name {conn_id}"),
            hostname: "127.0.0.1".to_owned(),
            modes: HashSet::new(),
            away_message: None,
            connected_at: now,
            signon_time: 0,
            last_active: now,
            registered: true,
            sender: tx,
        };
        (session, rx)
    }

    #[test]
    fn select_migration_targets_returns_sorted_peers() {
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let node3 = NodeId::new(3);

        let addr2: SocketAddr = "10.0.0.2:6667".parse().unwrap();
        let addr3: SocketAddr = "10.0.0.3:6667".parse().unwrap();

        let peer_map = PeerMap::new(vec![(node2, addr2), (node3, addr3)]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

        let registry = Arc::new(UserRegistry::new());
        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        let (raft_shutdown_tx, _raft_shutdown_rx) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        // We can't easily create a real RaftHandle, so we test the target
        // selection logic via the struct method. We create the handle's
        // channels manually.
        let (proposal_tx, _proposal_rx) = mpsc::unbounded_channel();
        let (membership_tx, _membership_rx) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_state_tx, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![node2, node3],
            raft_shutdown,
        );

        let targets = shutdown.select_migration_targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], (node2, addr2));
        assert_eq!(targets[1], (node3, addr3));
    }

    #[test]
    fn select_migration_targets_empty_when_no_peers() {
        let node1 = NodeId::new(1);

        let peer_map = PeerMap::new(vec![]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

        let registry = Arc::new(UserRegistry::new());
        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        let (raft_shutdown_tx, _raft_shutdown_rx) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        let (proposal_tx, _proposal_rx) = mpsc::unbounded_channel();
        let (membership_tx, _membership_rx) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![],
            raft_shutdown,
        );

        let targets = shutdown.select_migration_targets();
        assert!(targets.is_empty());
    }

    #[tokio::test]
    async fn propose_migrations_distributes_users_round_robin() {
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let node3 = NodeId::new(3);

        let addr2: SocketAddr = "10.0.0.2:6667".parse().unwrap();
        let addr3: SocketAddr = "10.0.0.3:6667".parse().unwrap();

        let peer_map = PeerMap::new(vec![(node2, addr2), (node3, addr3)]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

        // Set up registry with local users.
        let registry = Arc::new(UserRegistry::new());
        let (session1, mut rx1) = make_session(1, "alice");
        let (session2, mut rx2) = make_session(2, "bob");
        let (session3, mut rx3) = make_session(3, "carol");
        registry.register(session1).unwrap();
        registry.register(session2).unwrap();
        registry.register(session3).unwrap();

        // Set up user-node index with all users on node1.
        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        {
            let mut idx = user_node_index.write().await;
            idx.set_home_node("alice", node1);
            idx.set_home_node("bob", node1);
            idx.set_home_node("carol", node1);
        }

        let (proposal_tx, mut proposal_rx) = mpsc::unbounded_channel();
        let (membership_tx, _membership_rx) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, _) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![node2, node3],
            raft_shutdown,
        );

        let targets = vec![(node2, addr2), (node3, addr3)];
        let count = shutdown.propose_migrations_and_notify(&targets).await;
        assert_eq!(count, 3);

        // Verify proposals were sent.
        let mut proposals = Vec::new();
        while let Ok(cmd) = proposal_rx.try_recv() {
            proposals.push(cmd);
        }
        assert_eq!(proposals.len(), 3);

        // Verify all are UserMigrated commands from node1.
        for cmd in &proposals {
            match cmd {
                ClusterCommand::UserMigrated { from_node, .. } => {
                    assert_eq!(*from_node, node1);
                }
                _ => panic!("expected UserMigrated command"),
            }
        }

        // Verify clients received NOTICE and ERROR messages.
        // Each client should have received 2 messages (NOTICE + ERROR).
        let msgs1: Vec<_> = std::iter::from_fn(|| rx1.try_recv().ok()).collect();
        assert_eq!(msgs1.len(), 2, "alice should get NOTICE + ERROR");
        assert_eq!(msgs1[0].command, Command::Notice);
        assert_eq!(msgs1[1].command, Command::Error);

        let msgs2: Vec<_> = std::iter::from_fn(|| rx2.try_recv().ok()).collect();
        assert_eq!(msgs2.len(), 2, "bob should get NOTICE + ERROR");

        let msgs3: Vec<_> = std::iter::from_fn(|| rx3.try_recv().ok()).collect();
        assert_eq!(msgs3.len(), 2, "carol should get NOTICE + ERROR");
    }

    #[tokio::test]
    async fn wait_for_migrations_returns_true_when_all_migrated() {
        let node1 = NodeId::new(1);

        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        // Start empty — all migrations already committed.

        let peer_map = PeerMap::new(vec![]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));
        let registry = Arc::new(UserRegistry::new());

        let (proposal_tx, _) = mpsc::unbounded_channel();
        let (membership_tx, _) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, _) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![],
            raft_shutdown,
        );

        let result = shutdown.wait_for_migrations(3).await;
        assert!(result);
    }

    #[tokio::test]
    async fn wait_for_migrations_times_out_when_users_remain() {
        let node1 = NodeId::new(1);

        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        {
            let mut idx = user_node_index.write().await;
            idx.set_home_node("stuck_user", node1);
        }

        let peer_map = PeerMap::new(vec![]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));
        let registry = Arc::new(UserRegistry::new());

        let (proposal_tx, _) = mpsc::unbounded_channel();
        let (membership_tx, _) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, _) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        // Use a very short timeout for testing.
        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![],
            raft_shutdown,
        );

        // Override the timeout by running with a short-lived tokio timeout.
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            shutdown.wait_for_migrations(1),
        )
        .await;

        // The wait should still be running (timeout), meaning the function
        // itself will eventually return false. But since we used a 200ms
        // outer timeout and the inner is 10s, the outer fires first.
        assert!(result.is_err(), "should timeout waiting for stuck user");
    }

    #[tokio::test]
    async fn execute_no_peers_shuts_down_immediately() {
        let node1 = NodeId::new(1);

        let peer_map = PeerMap::new(vec![]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));
        let registry = Arc::new(UserRegistry::new());
        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));

        let (proposal_tx, _) = mpsc::unbounded_channel();
        let (membership_tx, _) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, raft_shutdown_rx) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        let mut shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![],
            raft_shutdown,
        );

        let result = shutdown.execute().await;
        assert_eq!(result.users_migrated, 0);
        assert!(!result.migration_committed);
        assert!(!result.membership_removed);

        // Raft shutdown should have been signalled.
        assert!(*raft_shutdown_rx.borrow());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn execute_with_users_proposes_migration() {
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        let addr2: SocketAddr = "10.0.0.2:6667".parse().unwrap();
        let peer_map = PeerMap::new(vec![(node2, addr2)]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

        let registry = Arc::new(UserRegistry::new());
        let (session1, _rx1) = make_session(1, "alice");
        registry.register(session1).unwrap();

        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));
        {
            let mut idx = user_node_index.write().await;
            idx.set_home_node("alice", node1);
        }

        let (proposal_tx, mut proposal_rx) = mpsc::unbounded_channel();
        // Drop membership_rx immediately so membership proposals fail cleanly
        // (no Raft driver to consume them in tests).
        let (membership_tx, membership_rx) = mpsc::unbounded_channel();
        drop(membership_rx);
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, raft_shutdown_rx) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        // Simulate that migration commits quickly by clearing the index.
        let idx_clone = Arc::clone(&user_node_index);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut idx = idx_clone.write().await;
            idx.remove_user("alice");
        });

        let mut shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![node2],
            raft_shutdown,
        );

        let result = shutdown.execute().await;
        assert_eq!(result.users_migrated, 1);
        assert!(result.migration_committed);

        // Verify proposal was sent.
        let cmd = proposal_rx.try_recv().unwrap();
        match cmd {
            ClusterCommand::UserMigrated {
                nickname,
                from_node,
                to_node,
            } => {
                assert_eq!(nickname, "alice");
                assert_eq!(from_node, node1);
                assert_eq!(to_node, node2);
            }
            _ => panic!("expected UserMigrated"),
        }

        // Raft shutdown should have been signalled.
        assert!(*raft_shutdown_rx.borrow());
    }

    #[test]
    fn send_redirect_notice_sends_notice_and_error() {
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let addr2: SocketAddr = "10.0.0.2:6667".parse().unwrap();

        let peer_map = PeerMap::new(vec![(node2, addr2)]);
        let shared_peer_map: SharedPeerMap = Arc::new(RwLock::new(peer_map));

        let registry = Arc::new(UserRegistry::new());
        let (session, mut rx) = make_session(1, "alice");
        registry.register(session).unwrap();

        let user_node_index = Arc::new(RwLock::new(UserNodeIndex::new()));

        let (proposal_tx, _) = mpsc::unbounded_channel();
        let (membership_tx, _) = mpsc::unbounded_channel();
        let (_, commit_rx) = mpsc::unbounded_channel();
        let (_, state_rx) = tokio::sync::watch::channel((
            crate::raft::types::RaftState::Leader,
            crate::raft::types::Term::new(1),
            Some(node1),
        ));
        let (_, health_event_rx) = mpsc::unbounded_channel();
        let (_, peer_status_rx) = tokio::sync::watch::channel(std::collections::HashMap::new());

        let handle = Arc::new(RaftHandle::new_for_test(
            proposal_tx,
            membership_tx,
            commit_rx,
            state_rx,
            health_event_rx,
            peer_status_rx,
        ));

        let (raft_shutdown_tx, _) = tokio::sync::watch::channel(false);
        let raft_shutdown = crate::raft::ShutdownSender { tx: raft_shutdown_tx };

        let shutdown = GracefulShutdown::new(
            handle,
            registry,
            user_node_index,
            shared_peer_map,
            node1,
            vec![node2],
            raft_shutdown,
        );

        shutdown.send_redirect_notice("alice", addr2);

        let msgs: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].command, Command::Notice);
        assert!(msgs[0].trailing().as_ref().unwrap().contains("reconnect to 10.0.0.2:6667"));
        assert_eq!(msgs[1].command, Command::Error);
        assert!(msgs[1].trailing().as_ref().unwrap().contains("10.0.0.2:6667"));
    }
}
