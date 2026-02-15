use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use super::cluster_command::ClusterCommand;
use super::snapshot::{SnapshotError, StateMachine};
use super::types::NodeId;

/// Serializable representation of a user's state for cluster replication.
///
/// Mirrors the fields of [`crate::user::UserSession`] that are relevant
/// for cross-node state replication, omitting non-serializable fields
/// like connection handles and `Instant` timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedUser {
    pub nickname: String,
    pub username: String,
    pub realname: String,
    pub hostname: String,
    pub modes: HashSet<String>,
    pub away_message: Option<String>,
    pub signon_time: u64,
    pub home_node: Option<NodeId>,
    pub is_oper: bool,
}

/// Serializable representation of a channel's ban list entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedBanEntry {
    pub mask: String,
    pub who_set: String,
    pub timestamp: u64,
}

/// Serializable representation of a channel's state for cluster replication.
///
/// Mirrors the fields of [`crate::channel::Channel`] that are relevant
/// for cross-node state replication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedChannel {
    pub name: String,
    pub topic: Option<(String, String, u64)>,
    pub modes: HashSet<String>,
    pub members: HashMap<String, String>,
    pub ban_list: Vec<ReplicatedBanEntry>,
    pub invite_list: HashSet<String>,
    pub key: Option<String>,
    pub user_limit: Option<u32>,
    pub created_at: u64,
}

/// Serializable representation of a cluster server node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedServer {
    pub node_id: NodeId,
    pub addr: SocketAddr,
}

/// Full replicated state snapshot for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplicatedState {
    users: HashMap<String, ReplicatedUser>,
    channels: HashMap<String, ReplicatedChannel>,
    servers: HashMap<u64, ReplicatedServer>,
}

/// The cluster state machine that applies committed Raft log entries
/// to replicated IRC state.
///
/// Owns serializable copies of user and channel state — NOT the live
/// registries with connection handles. The live registries are updated
/// separately via the commit consumer.
pub struct ClusterStateMachine {
    users: HashMap<String, ReplicatedUser>,
    channels: HashMap<String, ReplicatedChannel>,
    servers: HashMap<u64, ReplicatedServer>,
}

impl ClusterStateMachine {
    /// Creates a new empty state machine.
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            channels: HashMap::new(),
            servers: HashMap::new(),
        }
    }

    /// Returns the replicated user state, if the user exists.
    pub fn get_user(&self, nickname: &str) -> Option<&ReplicatedUser> {
        self.users.get(&nickname.to_ascii_lowercase())
    }

    /// Returns the replicated channel state, if the channel exists.
    pub fn get_channel(&self, name: &str) -> Option<&ReplicatedChannel> {
        self.channels.get(&name.to_ascii_lowercase())
    }

    /// Returns the number of replicated users.
    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// Returns the number of replicated channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Returns the number of replicated servers.
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// Ensure a channel exists, creating it with defaults if needed.
    fn ensure_channel(&mut self, channel: &str) -> &mut ReplicatedChannel {
        let key = channel.to_ascii_lowercase();
        self.channels.entry(key).or_insert_with(|| ReplicatedChannel {
            name: channel.to_string(),
            topic: None,
            modes: HashSet::new(),
            members: HashMap::new(),
            ban_list: Vec::new(),
            invite_list: HashSet::new(),
            key: None,
            user_limit: None,
            created_at: 0,
        })
    }

    /// Remove a user from all channels and clean up empty channels.
    fn remove_user_from_all_channels(&mut self, nick_key: &str) {
        for ch in self.channels.values_mut() {
            ch.members.remove(nick_key);
        }
        self.channels.retain(|_, ch| !ch.members.is_empty());
    }

    /// Remove a member from a specific channel, cleaning up if empty.
    fn remove_member_from_channel(&mut self, channel: &str, nickname: &str) {
        let chan_key = channel.to_ascii_lowercase();
        let nick_key = nickname.to_ascii_lowercase();
        if let Some(ch) = self.channels.get_mut(&chan_key) {
            ch.members.remove(&nick_key);
            if ch.members.is_empty() {
                self.channels.remove(&chan_key);
            }
        }
    }

    fn apply_user_registered(
        &mut self,
        nickname: &str,
        username: &str,
        realname: &str,
        hostname: &str,
        signon_time: u64,
        home_node: Option<NodeId>,
    ) {
        let key = nickname.to_ascii_lowercase();
        self.users.insert(
            key,
            ReplicatedUser {
                nickname: nickname.to_string(),
                username: username.to_string(),
                realname: realname.to_string(),
                hostname: hostname.to_string(),
                modes: HashSet::new(),
                away_message: None,
                signon_time,
                home_node,
                is_oper: false,
            },
        );
    }

    fn apply_nick_changed(&mut self, old_nick: &str, new_nick: &str) {
        let old_key = old_nick.to_ascii_lowercase();
        let new_key = new_nick.to_ascii_lowercase();
        if let Some(mut user) = self.users.remove(&old_key) {
            user.nickname.clone_from(&new_nick.to_string());
            self.users.insert(new_key.clone(), user);
        }
        for ch in self.channels.values_mut() {
            if let Some(status) = ch.members.remove(&old_key) {
                ch.members.insert(new_key.clone(), status);
            }
        }
    }

    fn apply_channel_mode_changed(
        &mut self,
        channel: &str,
        modes_added: &[String],
        modes_removed: &[String],
        key: Option<&String>,
        user_limit: Option<u32>,
        member_status_changes: &[(String, String)],
    ) {
        let ch = self.ensure_channel(channel);
        for mode in modes_added {
            ch.modes.insert(mode.clone());
        }
        for mode in modes_removed {
            ch.modes.remove(mode);
        }
        if let Some(k) = key {
            ch.key = Some(k.clone());
        }
        if let Some(limit) = user_limit {
            ch.user_limit = Some(limit);
        }
        for (nick, status) in member_status_changes {
            let nick_key = nick.to_ascii_lowercase();
            if ch.members.contains_key(&nick_key) {
                ch.members.insert(nick_key, status.clone());
            }
        }
    }
}

impl Default for ClusterStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl StateMachine<ClusterCommand> for ClusterStateMachine {
    fn apply(&mut self, command: &ClusterCommand) {
        match command {
            ClusterCommand::UserRegistered {
                nickname, username, realname, hostname, signon_time, home_node, ..
            } => self.apply_user_registered(nickname, username, realname, hostname, *signon_time, *home_node),

            ClusterCommand::NickChanged { old_nick, new_nick } => {
                self.apply_nick_changed(old_nick, new_nick);
            }

            ClusterCommand::UserQuit { nickname, .. }
            | ClusterCommand::UserKilled { nickname, .. } => {
                let key = nickname.to_ascii_lowercase();
                self.users.remove(&key);
                self.remove_user_from_all_channels(&key);
            }

            ClusterCommand::UserAway { nickname, message } => {
                let key = nickname.to_ascii_lowercase();
                if let Some(user) = self.users.get_mut(&key) {
                    user.away_message.clone_from(message);
                }
            }

            ClusterCommand::UserModeChanged { nickname, modes_added, modes_removed } => {
                let key = nickname.to_ascii_lowercase();
                if let Some(user) = self.users.get_mut(&key) {
                    for mode in modes_added {
                        user.modes.insert(mode.clone());
                    }
                    for mode in modes_removed {
                        user.modes.remove(mode);
                    }
                }
            }

            ClusterCommand::ChannelJoined { nickname, channel, status } => {
                let nick_key = nickname.to_ascii_lowercase();
                let ch = self.ensure_channel(channel);
                ch.members.insert(nick_key, status.clone());
            }

            ClusterCommand::ChannelParted { nickname, channel, .. }
            | ClusterCommand::UserKicked { channel, nickname, .. } => {
                self.remove_member_from_channel(channel, nickname);
            }

            ClusterCommand::TopicSet { channel, topic } => {
                let ch = self.ensure_channel(channel);
                ch.topic = topic.as_ref().map(|t| (t.text.clone(), t.who.clone(), t.timestamp));
            }

            ClusterCommand::ChannelModeChanged {
                channel, modes_added, modes_removed, key, user_limit, member_status_changes,
            } => self.apply_channel_mode_changed(
                channel, modes_added, modes_removed, key.as_ref(), *user_limit, member_status_changes,
            ),

            ClusterCommand::BanAdded { channel, mask, who_set, timestamp } => {
                let ch = self.ensure_channel(channel);
                ch.ban_list.push(ReplicatedBanEntry {
                    mask: mask.clone(),
                    who_set: who_set.clone(),
                    timestamp: *timestamp,
                });
            }

            ClusterCommand::BanRemoved { channel, mask } => {
                let chan_key = channel.to_ascii_lowercase();
                if let Some(ch) = self.channels.get_mut(&chan_key) {
                    ch.ban_list.retain(|b| b.mask != *mask);
                }
            }

            ClusterCommand::InviteAdded { channel, nickname } => {
                let ch = self.ensure_channel(channel);
                ch.invite_list.insert(nickname.to_ascii_lowercase());
            }

            ClusterCommand::OperGranted { nickname } => {
                let key = nickname.to_ascii_lowercase();
                if let Some(user) = self.users.get_mut(&key) {
                    user.is_oper = true;
                }
            }

            ClusterCommand::ServerAdded { node_id, addr } => {
                self.servers.insert(node_id.as_u64(), ReplicatedServer {
                    node_id: *node_id,
                    addr: *addr,
                });
            }

            ClusterCommand::ServerRemoved { node_id } => {
                self.servers.remove(&node_id.as_u64());
            }

            ClusterCommand::UserMigrated { nickname, to_node, .. } => {
                let key = nickname.to_ascii_lowercase();
                if let Some(user) = self.users.get_mut(&key) {
                    user.home_node = Some(*to_node);
                }
            }

            ClusterCommand::Noop { .. } => {}
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let state = ReplicatedState {
            users: self.users.clone(),
            channels: self.channels.clone(),
            servers: self.servers.clone(),
        };
        serde_json::to_vec(&state).expect("state serialization should not fail")
    }

    fn restore(&mut self, snapshot: &[u8]) -> Result<(), SnapshotError> {
        if snapshot.is_empty() {
            self.users.clear();
            self.channels.clear();
            self.servers.clear();
            return Ok(());
        }
        let state: ReplicatedState = serde_json::from_slice(snapshot)
            .map_err(|e| SnapshotError::InvalidData(e.to_string()))?;
        self.users = state.users;
        self.channels = state.channels;
        self.servers = state.servers;
        Ok(())
    }
}

#[cfg(test)]
#[path = "state_machine_tests.rs"]
mod state_machine_tests;