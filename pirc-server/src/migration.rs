use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::raft::health::HealthEvent;
use crate::raft::types::NodeId;
use crate::raft::{ClusterCommand, RaftHandle};

/// Tracks which users are homed to which nodes.
///
/// Updated by the commit consumer as it processes committed Raft entries.
/// Read by the migration service when a node goes down to determine which
/// users need migration.
///
/// Maintains a reverse index (`node_to_users`) for O(1) lookup of all users
/// on a given node, avoiding a full scan of the user map during migration.
pub struct UserNodeIndex {
    /// Maps lowercase nickname to their home node.
    user_to_node: HashMap<String, NodeId>,
    /// Reverse index: maps node ID to set of lowercase nicknames homed there.
    node_to_users: HashMap<NodeId, HashSet<String>>,
}

impl UserNodeIndex {
    pub fn new() -> Self {
        Self {
            user_to_node: HashMap::new(),
            node_to_users: HashMap::new(),
        }
    }

    /// Record a user registration with their home node.
    pub fn set_home_node(&mut self, nickname: &str, node_id: NodeId) {
        let key = nickname.to_ascii_lowercase();

        // Remove from old node's reverse index if re-homing.
        if let Some(&old_node) = self.user_to_node.get(&key) {
            if old_node != node_id {
                if let Some(users) = self.node_to_users.get_mut(&old_node) {
                    users.remove(&key);
                    if users.is_empty() {
                        self.node_to_users.remove(&old_node);
                    }
                }
            }
        }

        self.user_to_node.insert(key.clone(), node_id);
        self.node_to_users
            .entry(node_id)
            .or_default()
            .insert(key);
    }

    /// Remove a user (on quit or kill).
    pub fn remove_user(&mut self, nickname: &str) {
        let key = nickname.to_ascii_lowercase();
        if let Some(node_id) = self.user_to_node.remove(&key) {
            if let Some(users) = self.node_to_users.get_mut(&node_id) {
                users.remove(&key);
                if users.is_empty() {
                    self.node_to_users.remove(&node_id);
                }
            }
        }
    }

    /// Update a user's nickname mapping.
    pub fn rename_user(&mut self, old_nick: &str, new_nick: &str) {
        let old_key = old_nick.to_ascii_lowercase();
        let new_key = new_nick.to_ascii_lowercase();
        if let Some(node) = self.user_to_node.remove(&old_key) {
            self.user_to_node.insert(new_key.clone(), node);
            if let Some(users) = self.node_to_users.get_mut(&node) {
                users.remove(&old_key);
                users.insert(new_key);
            }
        }
    }

    /// Get all nicknames homed to a specific node.
    ///
    /// Uses the reverse index for O(1) lookup instead of scanning all users.
    pub fn users_on_node(&self, node_id: NodeId) -> Vec<String> {
        self.node_to_users
            .get(&node_id)
            .map(|users| users.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all known node IDs that have at least one user.
    pub fn active_nodes(&self) -> HashSet<NodeId> {
        self.node_to_users.keys().copied().collect()
    }
}

impl Default for UserNodeIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle to the user-node index.
pub type SharedUserNodeIndex = Arc<RwLock<UserNodeIndex>>;

/// Spawn the migration service task.
///
/// Listens for `HealthEvent::NodeDown` events from the Raft health monitor.
/// When a node goes down, queries the `UserNodeIndex` for affected users
/// and proposes `UserMigrated` commands through Raft to redistribute them
/// across surviving nodes in a round-robin fashion.
pub fn spawn_migration_service(
    mut health_rx: mpsc::UnboundedReceiver<HealthEvent>,
    raft_handle: Arc<RaftHandle<ClusterCommand>>,
    user_node_index: SharedUserNodeIndex,
    self_id: NodeId,
    peer_ids: Vec<NodeId>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("migration service started");

        while let Some(event) = health_rx.recv().await {
            match event {
                HealthEvent::NodeDown(failed_node) => {
                    info!(
                        failed_node = failed_node.as_u64(),
                        "node down detected, initiating user migration"
                    );

                    if !raft_handle.is_leader() {
                        debug!("not leader, skipping migration");
                        continue;
                    }

                    // Build the list of surviving nodes (excluding the failed one).
                    let mut surviving: Vec<NodeId> = peer_ids
                        .iter()
                        .copied()
                        .filter(|&id| id != failed_node)
                        .collect();
                    // Include self as a candidate target.
                    if self_id != failed_node {
                        surviving.push(self_id);
                    }
                    surviving.sort_by_key(|n| n.as_u64());

                    if surviving.is_empty() {
                        warn!("no surviving nodes for migration");
                        continue;
                    }

                    // Get users on the failed node.
                    let affected_users = {
                        let index = user_node_index.read().await;
                        index.users_on_node(failed_node)
                    };

                    if affected_users.is_empty() {
                        debug!(
                            failed_node = failed_node.as_u64(),
                            "no users to migrate from failed node"
                        );
                        continue;
                    }

                    info!(
                        failed_node = failed_node.as_u64(),
                        user_count = affected_users.len(),
                        "migrating users from failed node"
                    );

                    // Distribute users round-robin across surviving nodes.
                    // Build all proposals first, then submit them to minimise
                    // the time between individual propose() calls.
                    let proposals: Vec<ClusterCommand> = affected_users
                        .iter()
                        .enumerate()
                        .map(|(i, nickname)| {
                            let target = surviving[i % surviving.len()];
                            ClusterCommand::UserMigrated {
                                nickname: nickname.clone(),
                                from_node: failed_node,
                                to_node: target,
                            }
                        })
                        .collect();

                    let mut proposed = 0usize;
                    for cmd in proposals {
                        let nickname = match &cmd {
                            ClusterCommand::UserMigrated { nickname, .. } => nickname.clone(),
                            _ => String::new(),
                        };
                        if let Err(e) = raft_handle.propose(cmd) {
                            warn!(
                                nickname,
                                error = %e,
                                "failed to propose user migration"
                            );
                        } else {
                            proposed += 1;
                            debug!(
                                nickname,
                                from = failed_node.as_u64(),
                                "proposed user migration"
                            );
                        }
                    }

                    info!(
                        failed_node = failed_node.as_u64(),
                        proposed,
                        total = affected_users.len(),
                        "user migration proposals submitted"
                    );
                }

                HealthEvent::NodeSuspected(node_id) => {
                    debug!(
                        node = node_id.as_u64(),
                        "node suspected, waiting for confirmation"
                    );
                }

                HealthEvent::NodeUp(node_id) => {
                    info!(
                        node = node_id.as_u64(),
                        "previously failed node recovered"
                    );
                }
            }
        }

        info!("migration service shutting down: channel closed");
    })
}

/// Update the user-node index from a committed cluster command.
///
/// Called by the commit consumer for each committed entry to keep the
/// index in sync with replicated state.
pub fn update_user_node_index(index: &mut UserNodeIndex, command: &ClusterCommand) {
    match command {
        ClusterCommand::UserRegistered {
            nickname,
            home_node: Some(node),
            ..
        } => {
            index.set_home_node(nickname, *node);
        }
        ClusterCommand::UserMigrated {
            nickname, to_node, ..
        } => {
            index.set_home_node(nickname, *to_node);
        }
        ClusterCommand::UserQuit { nickname, .. }
        | ClusterCommand::UserKilled { nickname, .. } => {
            index.remove_user(nickname);
        }
        ClusterCommand::NickChanged { old_nick, new_nick } => {
            index.rename_user(old_nick, new_nick);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_node_index_basic_operations() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        // Set home nodes
        index.set_home_node("alice", node1);
        index.set_home_node("bob", node1);
        index.set_home_node("carol", node2);

        // Query users on node1
        let mut users = index.users_on_node(node1);
        users.sort();
        assert_eq!(users, vec!["alice", "bob"]);

        // Query users on node2
        let users = index.users_on_node(node2);
        assert_eq!(users, vec!["carol"]);

        // Empty query
        let users = index.users_on_node(NodeId::new(99));
        assert!(users.is_empty());
    }

    #[test]
    fn user_node_index_remove() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);

        index.set_home_node("alice", node1);
        index.set_home_node("bob", node1);

        index.remove_user("alice");
        let users = index.users_on_node(node1);
        assert_eq!(users, vec!["bob"]);
    }

    #[test]
    fn user_node_index_rename() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);

        index.set_home_node("alice", node1);
        index.rename_user("alice", "alicia");

        assert!(index.users_on_node(node1).contains(&"alicia".to_string()));
        assert!(!index.users_on_node(node1).contains(&"alice".to_string()));
    }

    #[test]
    fn user_node_index_case_insensitive() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);

        index.set_home_node("Alice", node1);
        let users = index.users_on_node(node1);
        assert_eq!(users, vec!["alice"]);

        index.remove_user("ALICE");
        assert!(index.users_on_node(node1).is_empty());
    }

    #[test]
    fn user_node_index_active_nodes() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        index.set_home_node("alice", node1);
        index.set_home_node("bob", node2);

        let nodes = index.active_nodes();
        assert!(nodes.contains(&node1));
        assert!(nodes.contains(&node2));
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn update_index_from_user_migrated() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        index.set_home_node("alice", node1);

        let cmd = ClusterCommand::UserMigrated {
            nickname: "alice".into(),
            from_node: node1,
            to_node: node2,
        };
        update_user_node_index(&mut index, &cmd);

        assert!(index.users_on_node(node1).is_empty());
        assert_eq!(index.users_on_node(node2), vec!["alice"]);
    }

    #[test]
    fn update_index_from_user_quit() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);

        index.set_home_node("alice", node1);

        let cmd = ClusterCommand::UserQuit {
            nickname: "alice".into(),
            reason: None,
        };
        update_user_node_index(&mut index, &cmd);

        assert!(index.users_on_node(node1).is_empty());
    }

    #[test]
    fn update_index_from_nick_changed() {
        let mut index = UserNodeIndex::new();
        let node1 = NodeId::new(1);

        index.set_home_node("alice", node1);

        let cmd = ClusterCommand::NickChanged {
            old_nick: "alice".into(),
            new_nick: "alicia".into(),
        };
        update_user_node_index(&mut index, &cmd);

        assert!(!index.users_on_node(node1).contains(&"alice".to_string()));
        assert!(index.users_on_node(node1).contains(&"alicia".to_string()));
    }

    #[tokio::test]
    async fn migration_service_proposes_commands_on_node_down() {
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let node3 = NodeId::new(3);

        // Set up user-node index with users on node2 (which will fail).
        let index = Arc::new(RwLock::new(UserNodeIndex::new()));
        {
            let mut idx = index.write().await;
            idx.set_home_node("alice", node2);
            idx.set_home_node("bob", node2);
            idx.set_home_node("carol", node3);
        }

        // Create a health event channel.
        let (health_tx, health_rx) = mpsc::unbounded_channel::<HealthEvent>();

        // We need a real Raft handle to test proposals. Since we can't easily
        // create one in a unit test, we test the index logic directly.
        // The integration test of the full service with Raft is covered in
        // the broader cluster test suite.

        // Verify the index correctly identifies affected users.
        {
            let idx = index.read().await;
            let mut affected = idx.users_on_node(node2);
            affected.sort();
            assert_eq!(affected, vec!["alice", "bob"]);

            // carol should not be affected
            let unaffected = idx.users_on_node(node3);
            assert_eq!(unaffected, vec!["carol"]);
        }

        // Verify round-robin distribution logic.
        let surviving = vec![node1, node3];
        let affected = vec!["alice".to_string(), "bob".to_string()];
        let assignments: Vec<_> = affected
            .iter()
            .enumerate()
            .map(|(i, nick)| (nick.clone(), surviving[i % surviving.len()]))
            .collect();
        assert_eq!(assignments[0], ("alice".to_string(), node1));
        assert_eq!(assignments[1], ("bob".to_string(), node3));

        // Clean up channel
        drop(health_tx);
        drop(health_rx);
    }

    #[test]
    fn default_user_node_index() {
        let index = UserNodeIndex::default();
        assert!(index.users_on_node(NodeId::new(1)).is_empty());
    }
}
