use pirc_common::{ChannelName, Nickname};
use tokio::sync::mpsc;

use super::*;
use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::raft::cluster_command::TopicInfo;
use crate::raft::types::{LogEntry, LogIndex, Term};
use crate::raft::ClusterCommand;
use crate::registry::UserRegistry;

fn nick(s: &str) -> Nickname {
    Nickname::new(s).unwrap()
}

fn chan(s: &str) -> ChannelName {
    ChannelName::new(s).unwrap()
}

/// Helper: apply a command directly to the registries.
fn apply(cmd: &ClusterCommand, registry: &UserRegistry, channels: &ChannelRegistry) {
    apply_command(cmd, registry, channels, None);
}

// ---- UserRegistered ----

#[test]
fn user_registered_creates_session() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Alice".into(),
            username: "alice".into(),
            realname: "Alice W".into(),
            hostname: "host.example.com".into(),
            signon_time: 1_700_000_000,
            home_node: None,
        },
        &registry,
        &channels,
    );

    let session_arc = registry.get_by_nick(&nick("Alice")).unwrap();
    let session = session_arc.read().unwrap();
    assert_eq!(session.connection_id, 1);
    assert_eq!(session.username, "alice");
    assert_eq!(session.realname, "Alice W");
    assert_eq!(session.hostname, "host.example.com");
    assert_eq!(session.signon_time, 1_700_000_000);
    assert!(session.registered);
}

#[test]
fn user_registered_idempotent_skips_duplicate() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    let cmd = ClusterCommand::UserRegistered {
        connection_id: 1,
        nickname: "Alice".into(),
        username: "alice".into(),
        realname: "Alice W".into(),
        hostname: "host".into(),
        signon_time: 100,
        home_node: None,
    };

    apply(&cmd, &registry, &channels);
    // Apply again — should not panic or overwrite.
    apply(&cmd, &registry, &channels);

    assert_eq!(registry.connection_count(), 1);
}

// ---- NickChanged ----

#[test]
fn nick_changed_updates_registry() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Alice".into(),
            username: "alice".into(),
            realname: "Alice".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::NickChanged {
            old_nick: "Alice".into(),
            new_nick: "Bob".into(),
        },
        &registry,
        &channels,
    );

    assert!(registry.get_by_nick(&nick("Alice")).is_none());
    assert!(registry.get_by_nick(&nick("Bob")).is_some());
}

#[test]
fn nick_changed_idempotent_when_already_changed() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Bob".into(),
            username: "alice".into(),
            realname: "Alice".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
        &registry,
        &channels,
    );

    // Old nick doesn't exist — this is a no-op.
    apply(
        &ClusterCommand::NickChanged {
            old_nick: "Alice".into(),
            new_nick: "Bob".into(),
        },
        &registry,
        &channels,
    );

    assert!(registry.get_by_nick(&nick("Bob")).is_some());
    assert_eq!(registry.connection_count(), 1);
}

// ---- UserQuit ----

#[test]
fn user_quit_removes_from_registry_and_channels() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Alice".into(),
            username: "alice".into(),
            realname: "Alice".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::ChannelJoined {
            nickname: "Alice".into(),
            channel: "#test".into(),
            status: "normal".into(),
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::UserQuit {
            nickname: "Alice".into(),
            reason: Some("Leaving".into()),
        },
        &registry,
        &channels,
    );

    assert!(registry.get_by_nick(&nick("Alice")).is_none());
    assert_eq!(registry.connection_count(), 0);
    // Channel should be removed because it's now empty.
    assert_eq!(channels.channel_count(), 0);
}

#[test]
fn user_quit_idempotent() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    // Quit for a user that doesn't exist — should be a no-op.
    apply(
        &ClusterCommand::UserQuit {
            nickname: "Ghost".into(),
            reason: None,
        },
        &registry,
        &channels,
    );

    assert_eq!(registry.connection_count(), 0);
}

// ---- UserAway ----

#[test]
fn user_away_sets_and_clears_message() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Alice".into(),
            username: "alice".into(),
            realname: "Alice".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::UserAway {
            nickname: "Alice".into(),
            message: Some("Gone fishing".into()),
        },
        &registry,
        &channels,
    );

    {
        let session = registry.get_by_nick(&nick("Alice")).unwrap();
        let s = session.read().unwrap();
        assert_eq!(s.away_message.as_deref(), Some("Gone fishing"));
    }

    // Clear away.
    apply(
        &ClusterCommand::UserAway {
            nickname: "Alice".into(),
            message: None,
        },
        &registry,
        &channels,
    );

    {
        let session = registry.get_by_nick(&nick("Alice")).unwrap();
        let s = session.read().unwrap();
        assert!(s.away_message.is_none());
    }
}

// ---- ChannelJoined ----

#[test]
fn channel_joined_creates_channel_and_adds_member() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::ChannelJoined {
            nickname: "Alice".into(),
            channel: "#test".into(),
            status: "operator".into(),
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert_eq!(ch.members.get(&nick("Alice")), Some(&MemberStatus::Operator));
}

#[test]
fn channel_joined_idempotent() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    let cmd = ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    };

    apply(&cmd, &registry, &channels);
    apply(&cmd, &registry, &channels);

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert_eq!(ch.member_count(), 1);
}

// ---- ChannelParted ----

#[test]
fn channel_parted_removes_member_and_cleans_empty_channel() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::ChannelJoined {
            nickname: "Alice".into(),
            channel: "#test".into(),
            status: "normal".into(),
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::ChannelParted {
            nickname: "Alice".into(),
            channel: "#test".into(),
            reason: None,
        },
        &registry,
        &channels,
    );

    assert_eq!(channels.channel_count(), 0);
}

// ---- TopicSet ----

#[test]
fn topic_set_updates_channel_topic() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    // Create the channel first.
    channels.get_or_create(chan("#test"));

    apply(
        &ClusterCommand::TopicSet {
            channel: "#test".into(),
            topic: Some(TopicInfo {
                text: "Welcome!".into(),
                who: "Alice".into(),
                timestamp: 1_700_000_000,
            }),
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    let (text, who, ts) = ch.topic.as_ref().unwrap();
    assert_eq!(text, "Welcome!");
    assert_eq!(who, "Alice");
    assert_eq!(*ts, 1_700_000_000);
}

#[test]
fn topic_set_clears_topic() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    let ch_arc = channels.get_or_create(chan("#test"));
    {
        let mut ch = ch_arc.write().unwrap();
        ch.topic = Some(("Old".into(), "Bob".into(), 100));
    }

    apply(
        &ClusterCommand::TopicSet {
            channel: "#test".into(),
            topic: None,
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert!(ch.topic.is_none());
}

// ---- ChannelModeChanged ----

#[test]
fn channel_mode_changed_adds_and_removes_modes() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    let ch_arc = channels.get_or_create(chan("#test"));
    {
        let mut ch = ch_arc.write().unwrap();
        ch.modes.insert(ChannelMode::Secret);
    }

    apply(
        &ClusterCommand::ChannelModeChanged {
            channel: "#test".into(),
            modes_added: vec!["i".into(), "m".into()],
            modes_removed: vec!["s".into()],
            key: None,
            user_limit: Some(50),
            member_status_changes: vec![],
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert!(ch.modes.contains(&ChannelMode::InviteOnly));
    assert!(ch.modes.contains(&ChannelMode::Moderated));
    assert!(!ch.modes.contains(&ChannelMode::Secret));
    assert_eq!(ch.user_limit, Some(50));
}

#[test]
fn channel_mode_changed_updates_member_status() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::ChannelJoined {
            nickname: "Alice".into(),
            channel: "#test".into(),
            status: "normal".into(),
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::ChannelModeChanged {
            channel: "#test".into(),
            modes_added: vec![],
            modes_removed: vec![],
            key: None,
            user_limit: None,
            member_status_changes: vec![("Alice".into(), "operator".into())],
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#test")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert_eq!(ch.members.get(&nick("Alice")), Some(&MemberStatus::Operator));
}

// ---- BanAdded / BanRemoved ----

#[test]
fn ban_added_and_removed() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    channels.get_or_create(chan("#test"));

    apply(
        &ClusterCommand::BanAdded {
            channel: "#test".into(),
            mask: "*!*@evil.host".into(),
            who_set: "Alice".into(),
            timestamp: 1_700_000_000,
        },
        &registry,
        &channels,
    );

    {
        let ch_arc = channels.get(&chan("#test")).unwrap();
        let ch = ch_arc.read().unwrap();
        assert_eq!(ch.ban_list.len(), 1);
        assert_eq!(ch.ban_list[0].mask, "*!*@evil.host");
    }

    // Idempotent: adding same ban again should not duplicate.
    apply(
        &ClusterCommand::BanAdded {
            channel: "#test".into(),
            mask: "*!*@evil.host".into(),
            who_set: "Alice".into(),
            timestamp: 1_700_000_000,
        },
        &registry,
        &channels,
    );

    {
        let ch_arc = channels.get(&chan("#test")).unwrap();
        let ch = ch_arc.read().unwrap();
        assert_eq!(ch.ban_list.len(), 1);
    }

    // Remove.
    apply(
        &ClusterCommand::BanRemoved {
            channel: "#test".into(),
            mask: "*!*@evil.host".into(),
        },
        &registry,
        &channels,
    );

    {
        let ch_arc = channels.get(&chan("#test")).unwrap();
        let ch = ch_arc.read().unwrap();
        assert!(ch.ban_list.is_empty());
    }
}

// ---- InviteAdded ----

#[test]
fn invite_added() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    channels.get_or_create(chan("#secret"));

    apply(
        &ClusterCommand::InviteAdded {
            channel: "#secret".into(),
            nickname: "Bob".into(),
        },
        &registry,
        &channels,
    );

    let ch_arc = channels.get(&chan("#secret")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert!(ch.invite_list.contains(&nick("Bob")));
}

// ---- UserKicked ----

#[test]
fn user_kicked_removes_from_channel() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::ChannelJoined {
            nickname: "Alice".into(),
            channel: "#test".into(),
            status: "normal".into(),
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::UserKicked {
            channel: "#test".into(),
            nickname: "Alice".into(),
            who: "Bob".into(),
            reason: Some("Misbehaving".into()),
        },
        &registry,
        &channels,
    );

    // Channel should be cleaned up since it's empty.
    assert_eq!(channels.channel_count(), 0);
}

// ---- OperGranted ----

#[test]
fn oper_granted_adds_operator_mode() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    apply(
        &ClusterCommand::UserRegistered {
            connection_id: 1,
            nickname: "Admin".into(),
            username: "admin".into(),
            realname: "Admin".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
        &registry,
        &channels,
    );

    apply(
        &ClusterCommand::OperGranted {
            nickname: "Admin".into(),
        },
        &registry,
        &channels,
    );

    let session = registry.get_by_nick(&nick("Admin")).unwrap();
    let s = session.read().unwrap();
    assert!(s.modes.contains(&pirc_common::UserMode::Operator));
}

// ---- Noop / ServerAdded / ServerRemoved / UserMigrated ----

#[test]
fn noop_and_server_commands_are_no_ops() {
    let registry = UserRegistry::new();
    let channels = ChannelRegistry::new();

    // These should not panic or change state.
    apply(
        &ClusterCommand::Noop {
            description: "test".into(),
        },
        &registry,
        &channels,
    );
    apply(
        &ClusterCommand::ServerAdded {
            node_id: crate::raft::NodeId::new(42),
            addr: "10.0.0.1:7000".parse().unwrap(),
        },
        &registry,
        &channels,
    );
    apply(
        &ClusterCommand::ServerRemoved {
            node_id: crate::raft::NodeId::new(42),
        },
        &registry,
        &channels,
    );
    apply(
        &ClusterCommand::UserMigrated {
            nickname: "Alice".into(),
            from_node: crate::raft::NodeId::new(1),
            to_node: crate::raft::NodeId::new(2),
        },
        &registry,
        &channels,
    );

    assert_eq!(registry.connection_count(), 0);
    assert_eq!(channels.channel_count(), 0);
}

// ---- spawn_commit_consumer integration ----

#[tokio::test]
async fn commit_consumer_processes_entries_from_channel() {
    let (tx, rx) = mpsc::unbounded_channel();
    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());

    let handle = spawn_commit_consumer(rx, Arc::clone(&registry), Arc::clone(&channels), None, None);

    // Send a UserRegistered entry.
    tx.send(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(1),
        command: ClusterCommand::UserRegistered {
            connection_id: 42,
            nickname: "TestUser".into(),
            username: "tuser".into(),
            realname: "Test".into(),
            hostname: "host".into(),
            signon_time: 100,
            home_node: None,
        },
    })
    .unwrap();

    // Send a ChannelJoined entry.
    tx.send(LogEntry {
        term: Term::new(1),
        index: LogIndex::new(2),
        command: ClusterCommand::ChannelJoined {
            nickname: "TestUser".into(),
            channel: "#lobby".into(),
            status: "operator".into(),
        },
    })
    .unwrap();

    // Drop sender to close the channel.
    drop(tx);

    // Wait for the consumer to finish.
    handle.await.unwrap();

    // Verify state.
    assert!(registry.get_by_nick(&nick("TestUser")).is_some());
    assert_eq!(registry.connection_count(), 1);

    let ch_arc = channels.get(&chan("#lobby")).unwrap();
    let ch = ch_arc.read().unwrap();
    assert_eq!(ch.members.get(&nick("TestUser")), Some(&MemberStatus::Operator));
}

// ---- parse helpers ----

#[test]
fn parse_member_status_variants() {
    assert_eq!(parse_member_status("operator"), MemberStatus::Operator);
    assert_eq!(parse_member_status("voiced"), MemberStatus::Voiced);
    assert_eq!(parse_member_status("normal"), MemberStatus::Normal);
    assert_eq!(parse_member_status("unknown"), MemberStatus::Normal);
}

#[test]
fn parse_channel_mode_variants() {
    assert_eq!(parse_channel_mode("i"), Some(ChannelMode::InviteOnly));
    assert_eq!(parse_channel_mode("m"), Some(ChannelMode::Moderated));
    assert_eq!(parse_channel_mode("n"), Some(ChannelMode::NoExternalMessages));
    assert_eq!(parse_channel_mode("s"), Some(ChannelMode::Secret));
    assert_eq!(parse_channel_mode("t"), Some(ChannelMode::TopicProtected));
    assert_eq!(parse_channel_mode("z"), None);
}
