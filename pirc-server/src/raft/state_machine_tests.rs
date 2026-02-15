use crate::raft::cluster_command::{ClusterCommand, TopicInfo};
use crate::raft::snapshot::StateMachine;
use crate::raft::state_machine::ClusterStateMachine;
use crate::raft::types::NodeId;

fn new_sm() -> ClusterStateMachine {
    ClusterStateMachine::new()
}

fn register_alice(sm: &mut ClusterStateMachine) {
    sm.apply(&ClusterCommand::UserRegistered {
        connection_id: 1,
        nickname: "Alice".into(),
        username: "alice".into(),
        realname: "Alice Wonderland".into(),
        hostname: "example.com".into(),
        signon_time: 1_700_000_000,
        home_node: None,
    });
}

fn register_bob(sm: &mut ClusterStateMachine) {
    sm.apply(&ClusterCommand::UserRegistered {
        connection_id: 2,
        nickname: "Bob".into(),
        username: "bob".into(),
        realname: "Bob Builder".into(),
        hostname: "bob.example.com".into(),
        signon_time: 1_700_000_001,
        home_node: None,
    });
}

// ---- UserRegistered ----

#[test]
fn apply_user_registered() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    assert_eq!(sm.user_count(), 1);
    let user = sm.get_user("alice").unwrap();
    assert_eq!(user.nickname, "Alice");
    assert_eq!(user.username, "alice");
    assert_eq!(user.realname, "Alice Wonderland");
    assert_eq!(user.hostname, "example.com");
    assert_eq!(user.signon_time, 1_700_000_000);
    assert!(user.modes.is_empty());
    assert!(user.away_message.is_none());
    assert!(!user.is_oper);
}

#[test]
fn user_lookup_is_case_insensitive() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    assert!(sm.get_user("Alice").is_some());
    assert!(sm.get_user("alice").is_some());
    assert!(sm.get_user("ALICE").is_some());
}

// ---- NickChanged ----

#[test]
fn apply_nick_changed() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::NickChanged {
        old_nick: "Alice".into(),
        new_nick: "Alicia".into(),
    });

    assert!(sm.get_user("alice").is_none());
    assert_eq!(sm.user_count(), 1);
    let user = sm.get_user("alicia").unwrap();
    assert_eq!(user.nickname, "Alicia");
    assert_eq!(user.username, "alice");
}

#[test]
fn nick_changed_updates_channel_members() {
    let mut sm = new_sm();
    register_alice(&mut sm);
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::NickChanged {
        old_nick: "Alice".into(),
        new_nick: "Alicia".into(),
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(ch.members.contains_key("alicia"));
    assert!(!ch.members.contains_key("alice"));
}

// ---- UserQuit ----

#[test]
fn apply_user_quit() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::UserQuit {
        nickname: "Alice".into(),
        reason: Some("Leaving".into()),
    });

    assert_eq!(sm.user_count(), 0);
    assert!(sm.get_user("alice").is_none());
}

#[test]
fn user_quit_removes_from_channels() {
    let mut sm = new_sm();
    register_alice(&mut sm);
    register_bob(&mut sm);

    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Bob".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::UserQuit {
        nickname: "Alice".into(),
        reason: None,
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(!ch.members.contains_key("alice"));
    assert!(ch.members.contains_key("bob"));
}

#[test]
fn user_quit_cleans_up_empty_channels() {
    let mut sm = new_sm();
    register_alice(&mut sm);
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#lonely".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::UserQuit {
        nickname: "Alice".into(),
        reason: None,
    });

    assert!(sm.get_channel("#lonely").is_none());
}

// ---- UserAway ----

#[test]
fn apply_user_away() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::UserAway {
        nickname: "Alice".into(),
        message: Some("Gone fishing".into()),
    });

    let user = sm.get_user("alice").unwrap();
    assert_eq!(user.away_message.as_deref(), Some("Gone fishing"));

    sm.apply(&ClusterCommand::UserAway {
        nickname: "Alice".into(),
        message: None,
    });

    let user = sm.get_user("alice").unwrap();
    assert!(user.away_message.is_none());
}

// ---- UserModeChanged ----

#[test]
fn apply_user_mode_changed() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::UserModeChanged {
        nickname: "Alice".into(),
        modes_added: vec!["i".into(), "w".into()],
        modes_removed: vec![],
    });

    let user = sm.get_user("alice").unwrap();
    assert!(user.modes.contains("i"));
    assert!(user.modes.contains("w"));

    sm.apply(&ClusterCommand::UserModeChanged {
        nickname: "Alice".into(),
        modes_added: vec![],
        modes_removed: vec!["i".into()],
    });

    let user = sm.get_user("alice").unwrap();
    assert!(!user.modes.contains("i"));
    assert!(user.modes.contains("w"));
}

// ---- ChannelJoined ----

#[test]
fn apply_channel_joined() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });

    assert_eq!(sm.channel_count(), 1);
    let ch = sm.get_channel("#test").unwrap();
    assert_eq!(ch.name, "#test");
    assert_eq!(ch.members.get("alice"), Some(&"operator".to_string()));
}

#[test]
fn channel_lookup_is_case_insensitive() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#Test".into(),
        status: "normal".into(),
    });

    assert!(sm.get_channel("#test").is_some());
    assert!(sm.get_channel("#Test").is_some());
    assert!(sm.get_channel("#TEST").is_some());
}

// ---- ChannelParted ----

#[test]
fn apply_channel_parted() {
    let mut sm = new_sm();
    register_alice(&mut sm);
    register_bob(&mut sm);
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Bob".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::ChannelParted {
        nickname: "Alice".into(),
        channel: "#test".into(),
        reason: Some("Bye".into()),
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(!ch.members.contains_key("alice"));
    assert!(ch.members.contains_key("bob"));
}

#[test]
fn channel_parted_removes_empty_channel() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#solo".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::ChannelParted {
        nickname: "Alice".into(),
        channel: "#solo".into(),
        reason: None,
    });

    assert!(sm.get_channel("#solo").is_none());
}

// ---- TopicSet ----

#[test]
fn apply_topic_set() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::TopicSet {
        channel: "#test".into(),
        topic: Some(TopicInfo {
            text: "Welcome!".into(),
            who: "Alice".into(),
            timestamp: 1_700_000_000,
        }),
    });

    let ch = sm.get_channel("#test").unwrap();
    let (text, who, ts) = ch.topic.as_ref().unwrap();
    assert_eq!(text, "Welcome!");
    assert_eq!(who, "Alice");
    assert_eq!(*ts, 1_700_000_000);
}

#[test]
fn apply_topic_cleared() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });
    sm.apply(&ClusterCommand::TopicSet {
        channel: "#test".into(),
        topic: Some(TopicInfo {
            text: "Hello".into(),
            who: "Alice".into(),
            timestamp: 100,
        }),
    });

    sm.apply(&ClusterCommand::TopicSet {
        channel: "#test".into(),
        topic: None,
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(ch.topic.is_none());
}

// ---- ChannelModeChanged ----

#[test]
fn apply_channel_mode_changed() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::ChannelModeChanged {
        channel: "#test".into(),
        modes_added: vec!["n".into(), "t".into()],
        modes_removed: vec![],
        key: Some("secret".into()),
        user_limit: Some(50),
        member_status_changes: vec![],
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(ch.modes.contains("n"));
    assert!(ch.modes.contains("t"));
    assert_eq!(ch.key.as_deref(), Some("secret"));
    assert_eq!(ch.user_limit, Some(50));
}

#[test]
fn apply_channel_mode_changed_member_status() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::ChannelModeChanged {
        channel: "#test".into(),
        modes_added: vec![],
        modes_removed: vec![],
        key: None,
        user_limit: None,
        member_status_changes: vec![("Alice".into(), "operator".into())],
    });

    let ch = sm.get_channel("#test").unwrap();
    assert_eq!(ch.members.get("alice"), Some(&"operator".to_string()));
}

// ---- BanAdded / BanRemoved ----

#[test]
fn apply_ban_added() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::BanAdded {
        channel: "#test".into(),
        mask: "*!*@bad.host".into(),
        who_set: "Alice".into(),
        timestamp: 1_700_000_000,
    });

    let ch = sm.get_channel("#test").unwrap();
    assert_eq!(ch.ban_list.len(), 1);
    assert_eq!(ch.ban_list[0].mask, "*!*@bad.host");
    assert_eq!(ch.ban_list[0].who_set, "Alice");
}

#[test]
fn apply_ban_removed() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });
    sm.apply(&ClusterCommand::BanAdded {
        channel: "#test".into(),
        mask: "*!*@bad.host".into(),
        who_set: "Alice".into(),
        timestamp: 100,
    });

    sm.apply(&ClusterCommand::BanRemoved {
        channel: "#test".into(),
        mask: "*!*@bad.host".into(),
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(ch.ban_list.is_empty());
}

// ---- InviteAdded ----

#[test]
fn apply_invite_added() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#secret".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::InviteAdded {
        channel: "#secret".into(),
        nickname: "Bob".into(),
    });

    let ch = sm.get_channel("#secret").unwrap();
    assert!(ch.invite_list.contains("bob"));
}

// ---- UserKicked ----

#[test]
fn apply_user_kicked() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "operator".into(),
    });
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Bob".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::UserKicked {
        channel: "#test".into(),
        nickname: "Bob".into(),
        who: "Alice".into(),
        reason: Some("Misbehaving".into()),
    });

    let ch = sm.get_channel("#test").unwrap();
    assert!(!ch.members.contains_key("bob"));
    assert!(ch.members.contains_key("alice"));
}

#[test]
fn user_kicked_removes_empty_channel() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#solo".into(),
        status: "operator".into(),
    });

    sm.apply(&ClusterCommand::UserKicked {
        channel: "#solo".into(),
        nickname: "Alice".into(),
        who: "server".into(),
        reason: None,
    });

    assert!(sm.get_channel("#solo").is_none());
}

// ---- UserKilled ----

#[test]
fn apply_user_killed() {
    let mut sm = new_sm();
    register_alice(&mut sm);
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#test".into(),
        status: "normal".into(),
    });

    sm.apply(&ClusterCommand::UserKilled {
        nickname: "Alice".into(),
        reason: "Spamming".into(),
    });

    assert!(sm.get_user("alice").is_none());
    assert!(sm.get_channel("#test").is_none());
}

// ---- OperGranted ----

#[test]
fn apply_oper_granted() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::OperGranted {
        nickname: "Alice".into(),
    });

    let user = sm.get_user("alice").unwrap();
    assert!(user.is_oper);
}

// ---- ServerAdded / ServerRemoved ----

#[test]
fn apply_server_added() {
    let mut sm = new_sm();

    sm.apply(&ClusterCommand::ServerAdded {
        node_id: NodeId::new(42),
        addr: "10.0.0.42:7000".parse().unwrap(),
    });

    assert_eq!(sm.server_count(), 1);
}

#[test]
fn apply_server_removed() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ServerAdded {
        node_id: NodeId::new(42),
        addr: "10.0.0.42:7000".parse().unwrap(),
    });

    sm.apply(&ClusterCommand::ServerRemoved {
        node_id: NodeId::new(42),
    });

    assert_eq!(sm.server_count(), 0);
}

// ---- UserMigrated ----

#[test]
fn apply_user_migrated() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.apply(&ClusterCommand::UserMigrated {
        nickname: "Alice".into(),
        from_node: NodeId::new(1),
        to_node: NodeId::new(2),
    });

    let user = sm.get_user("alice").unwrap();
    assert_eq!(user.home_node, Some(NodeId::new(2)));
}

// ---- Noop ----

#[test]
fn apply_noop() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::Noop {
        description: "test".into(),
    });
    assert_eq!(sm.user_count(), 0);
    assert_eq!(sm.channel_count(), 0);
}

// ---- Snapshot / Restore ----

#[test]
fn snapshot_restore_empty() {
    let sm = new_sm();
    let snapshot = sm.snapshot();

    let mut sm2 = new_sm();
    sm2.restore(&snapshot).unwrap();
    assert_eq!(sm2.user_count(), 0);
    assert_eq!(sm2.channel_count(), 0);
}

#[test]
fn snapshot_restore_roundtrip() {
    let mut sm = new_sm();

    // Build up some state.
    register_alice(&mut sm);
    register_bob(&mut sm);

    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Alice".into(),
        channel: "#general".into(),
        status: "operator".into(),
    });
    sm.apply(&ClusterCommand::ChannelJoined {
        nickname: "Bob".into(),
        channel: "#general".into(),
        status: "normal".into(),
    });
    sm.apply(&ClusterCommand::TopicSet {
        channel: "#general".into(),
        topic: Some(TopicInfo {
            text: "Welcome!".into(),
            who: "Alice".into(),
            timestamp: 1_700_000_000,
        }),
    });
    sm.apply(&ClusterCommand::BanAdded {
        channel: "#general".into(),
        mask: "*!*@bad.host".into(),
        who_set: "Alice".into(),
        timestamp: 1_700_000_000,
    });
    sm.apply(&ClusterCommand::UserAway {
        nickname: "Bob".into(),
        message: Some("AFK".into()),
    });
    sm.apply(&ClusterCommand::OperGranted {
        nickname: "Alice".into(),
    });
    sm.apply(&ClusterCommand::ServerAdded {
        node_id: NodeId::new(1),
        addr: "10.0.0.1:7000".parse().unwrap(),
    });
    sm.apply(&ClusterCommand::UserMigrated {
        nickname: "Alice".into(),
        from_node: NodeId::new(1),
        to_node: NodeId::new(2),
    });

    // Snapshot and restore.
    let snapshot = sm.snapshot();
    let mut sm2 = new_sm();
    sm2.restore(&snapshot).unwrap();

    // Verify all state.
    assert_eq!(sm2.user_count(), 2);
    assert_eq!(sm2.channel_count(), 1);
    assert_eq!(sm2.server_count(), 1);

    let alice = sm2.get_user("alice").unwrap();
    assert_eq!(alice.nickname, "Alice");
    assert!(alice.is_oper);
    assert_eq!(alice.home_node, Some(NodeId::new(2)));

    let bob = sm2.get_user("bob").unwrap();
    assert_eq!(bob.away_message.as_deref(), Some("AFK"));

    let ch = sm2.get_channel("#general").unwrap();
    assert_eq!(ch.members.len(), 2);
    assert_eq!(ch.ban_list.len(), 1);
    let (text, _, _) = ch.topic.as_ref().unwrap();
    assert_eq!(text, "Welcome!");
}

#[test]
fn restore_empty_snapshot() {
    let mut sm = new_sm();
    register_alice(&mut sm);

    sm.restore(&[]).unwrap();
    assert_eq!(sm.user_count(), 0);
    assert_eq!(sm.channel_count(), 0);
}

#[test]
fn restore_invalid_snapshot() {
    let mut sm = new_sm();
    let result = sm.restore(b"not valid json");
    assert!(result.is_err());
}

// ---- Edge cases ----

#[test]
fn apply_to_nonexistent_user_is_noop() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::UserAway {
        nickname: "Ghost".into(),
        message: Some("Boo".into()),
    });
    assert_eq!(sm.user_count(), 0);
}

#[test]
fn nick_change_for_nonexistent_user_is_noop() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::NickChanged {
        old_nick: "Ghost".into(),
        new_nick: "Phantom".into(),
    });
    assert_eq!(sm.user_count(), 0);
}

#[test]
fn channel_parted_for_nonexistent_channel_is_noop() {
    let mut sm = new_sm();
    sm.apply(&ClusterCommand::ChannelParted {
        nickname: "Alice".into(),
        channel: "#nonexistent".into(),
        reason: None,
    });
    assert_eq!(sm.channel_count(), 0);
}
