use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use super::types::NodeId;

/// Represents all IRC state mutations replicated across the cluster via Raft.
///
/// Each variant captures a discrete state change that must be applied
/// consistently on every node. All fields are serializable (no `Instant`,
/// no channel senders).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterCommand {
    /// A new user has completed registration.
    UserRegistered {
        connection_id: u64,
        nickname: String,
        username: String,
        realname: String,
        hostname: String,
        /// Unix timestamp (seconds since epoch).
        signon_time: u64,
        /// The node where this user is homed. Used for migration tracking.
        #[serde(default)]
        home_node: Option<NodeId>,
    },

    /// A user changed their nickname.
    NickChanged {
        old_nick: String,
        new_nick: String,
    },

    /// A user disconnected.
    UserQuit {
        nickname: String,
        reason: Option<String>,
    },

    /// A user set or cleared their away status.
    UserAway {
        nickname: String,
        /// `None` means the user is no longer away.
        message: Option<String>,
    },

    /// A user's server-level modes changed.
    UserModeChanged {
        nickname: String,
        modes_added: Vec<String>,
        modes_removed: Vec<String>,
    },

    /// A user joined a channel.
    ChannelJoined {
        nickname: String,
        channel: String,
        /// Initial member status (e.g. "normal", "operator").
        status: String,
    },

    /// A user left a channel.
    ChannelParted {
        nickname: String,
        channel: String,
        reason: Option<String>,
    },

    /// A channel's topic was set or cleared.
    TopicSet {
        channel: String,
        /// `None` means the topic was cleared.
        topic: Option<TopicInfo>,
    },

    /// A channel's modes changed.
    ChannelModeChanged {
        channel: String,
        modes_added: Vec<String>,
        modes_removed: Vec<String>,
        key: Option<String>,
        user_limit: Option<u32>,
        /// Changes to member status within the channel (nick, status).
        member_status_changes: Vec<(String, String)>,
    },

    /// A ban was added to a channel.
    BanAdded {
        channel: String,
        mask: String,
        who_set: String,
        /// Unix timestamp.
        timestamp: u64,
    },

    /// A ban was removed from a channel.
    BanRemoved {
        channel: String,
        mask: String,
    },

    /// A user was invited to a channel.
    InviteAdded {
        channel: String,
        nickname: String,
    },

    /// A user was kicked from a channel.
    UserKicked {
        channel: String,
        nickname: String,
        who: String,
        reason: Option<String>,
    },

    /// A user was killed (forcibly disconnected by an operator).
    UserKilled {
        nickname: String,
        reason: String,
    },

    /// A user was granted IRC operator privileges.
    OperGranted {
        nickname: String,
    },

    /// A server was added to the cluster.
    ServerAdded {
        node_id: NodeId,
        addr: SocketAddr,
    },

    /// A server was removed from the cluster.
    ServerRemoved {
        node_id: NodeId,
    },

    /// A user was migrated from one server to another (failover).
    UserMigrated {
        nickname: String,
        from_node: NodeId,
        to_node: NodeId,
    },

    /// A no-op command used for membership change log entries.
    Noop {
        description: String,
    },
}

/// Topic metadata for [`ClusterCommand::TopicSet`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicInfo {
    pub text: String,
    pub who: String,
    /// Unix timestamp.
    pub timestamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: serialize to JSON and deserialize back, asserting equality.
    fn roundtrip(cmd: &ClusterCommand) {
        let json = serde_json::to_string(cmd).expect("serialize");
        let deserialized: ClusterCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(*cmd, deserialized, "roundtrip failed for: {json}");
    }

    #[test]
    fn user_registered_roundtrip() {
        roundtrip(&ClusterCommand::UserRegistered {
            connection_id: 42,
            nickname: "alice".into(),
            username: "alice".into(),
            realname: "Alice Wonderland".into(),
            hostname: "example.com".into(),
            signon_time: 1_700_000_000,
            home_node: Some(NodeId::new(1)),
        });
        // Also test with None home_node for backward compatibility.
        roundtrip(&ClusterCommand::UserRegistered {
            connection_id: 43,
            nickname: "bob".into(),
            username: "bob".into(),
            realname: "Bob".into(),
            hostname: "example.com".into(),
            signon_time: 1_700_000_000,
            home_node: None,
        });
    }

    #[test]
    fn nick_changed_roundtrip() {
        roundtrip(&ClusterCommand::NickChanged {
            old_nick: "alice".into(),
            new_nick: "bob".into(),
        });
    }

    #[test]
    fn user_quit_roundtrip() {
        roundtrip(&ClusterCommand::UserQuit {
            nickname: "alice".into(),
            reason: Some("Leaving".into()),
        });
        roundtrip(&ClusterCommand::UserQuit {
            nickname: "bob".into(),
            reason: None,
        });
    }

    #[test]
    fn user_away_roundtrip() {
        roundtrip(&ClusterCommand::UserAway {
            nickname: "alice".into(),
            message: Some("Gone fishing".into()),
        });
        roundtrip(&ClusterCommand::UserAway {
            nickname: "alice".into(),
            message: None,
        });
    }

    #[test]
    fn user_mode_changed_roundtrip() {
        roundtrip(&ClusterCommand::UserModeChanged {
            nickname: "alice".into(),
            modes_added: vec!["i".into(), "w".into()],
            modes_removed: vec!["o".into()],
        });
    }

    #[test]
    fn channel_joined_roundtrip() {
        roundtrip(&ClusterCommand::ChannelJoined {
            nickname: "alice".into(),
            channel: "#test".into(),
            status: "normal".into(),
        });
    }

    #[test]
    fn channel_parted_roundtrip() {
        roundtrip(&ClusterCommand::ChannelParted {
            nickname: "alice".into(),
            channel: "#test".into(),
            reason: Some("Bye".into()),
        });
        roundtrip(&ClusterCommand::ChannelParted {
            nickname: "bob".into(),
            channel: "#general".into(),
            reason: None,
        });
    }

    #[test]
    fn topic_set_roundtrip() {
        roundtrip(&ClusterCommand::TopicSet {
            channel: "#test".into(),
            topic: Some(TopicInfo {
                text: "Welcome!".into(),
                who: "alice".into(),
                timestamp: 1_700_000_000,
            }),
        });
        roundtrip(&ClusterCommand::TopicSet {
            channel: "#test".into(),
            topic: None,
        });
    }

    #[test]
    fn channel_mode_changed_roundtrip() {
        roundtrip(&ClusterCommand::ChannelModeChanged {
            channel: "#test".into(),
            modes_added: vec!["n".into(), "t".into()],
            modes_removed: vec![],
            key: Some("secret".into()),
            user_limit: Some(50),
            member_status_changes: vec![("alice".into(), "operator".into())],
        });
    }

    #[test]
    fn ban_added_roundtrip() {
        roundtrip(&ClusterCommand::BanAdded {
            channel: "#test".into(),
            mask: "*!*@bad.host".into(),
            who_set: "alice".into(),
            timestamp: 1_700_000_000,
        });
    }

    #[test]
    fn ban_removed_roundtrip() {
        roundtrip(&ClusterCommand::BanRemoved {
            channel: "#test".into(),
            mask: "*!*@bad.host".into(),
        });
    }

    #[test]
    fn invite_added_roundtrip() {
        roundtrip(&ClusterCommand::InviteAdded {
            channel: "#secret".into(),
            nickname: "bob".into(),
        });
    }

    #[test]
    fn user_kicked_roundtrip() {
        roundtrip(&ClusterCommand::UserKicked {
            channel: "#test".into(),
            nickname: "bob".into(),
            who: "alice".into(),
            reason: Some("Misbehaving".into()),
        });
        roundtrip(&ClusterCommand::UserKicked {
            channel: "#test".into(),
            nickname: "bob".into(),
            who: "alice".into(),
            reason: None,
        });
    }

    #[test]
    fn user_killed_roundtrip() {
        roundtrip(&ClusterCommand::UserKilled {
            nickname: "troll".into(),
            reason: "Spamming".into(),
        });
    }

    #[test]
    fn oper_granted_roundtrip() {
        roundtrip(&ClusterCommand::OperGranted {
            nickname: "admin".into(),
        });
    }

    #[test]
    fn server_added_roundtrip() {
        roundtrip(&ClusterCommand::ServerAdded {
            node_id: NodeId::new(42),
            addr: "10.0.0.42:7000".parse().unwrap(),
        });
    }

    #[test]
    fn server_removed_roundtrip() {
        roundtrip(&ClusterCommand::ServerRemoved {
            node_id: NodeId::new(42),
        });
    }

    #[test]
    fn user_migrated_roundtrip() {
        roundtrip(&ClusterCommand::UserMigrated {
            nickname: "alice".into(),
            from_node: NodeId::new(1),
            to_node: NodeId::new(2),
        });
    }

    #[test]
    fn noop_roundtrip() {
        roundtrip(&ClusterCommand::Noop {
            description: "add-server:42".into(),
        });
    }
}
