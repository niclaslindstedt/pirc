use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::{ChannelMode, ChannelName, Nickname};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::channel::{BanEntry, MemberStatus};
use crate::channel_registry::ChannelRegistry;
use crate::raft::types::LogEntry;
use crate::raft::ClusterCommand;
use crate::registry::UserRegistry;
use crate::user::UserSession;

/// Spawn the commit consumer task that reads committed Raft log entries
/// and applies them to the local `UserRegistry` and `ChannelRegistry`.
///
/// Returns a `JoinHandle` so the caller can track the task lifetime.
pub fn spawn_commit_consumer(
    mut commit_rx: mpsc::UnboundedReceiver<LogEntry<ClusterCommand>>,
    registry: Arc<UserRegistry>,
    channels: Arc<ChannelRegistry>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(entry) = commit_rx.recv().await {
            debug!(
                index = entry.index.as_u64(),
                term = entry.term.as_u64(),
                "applying committed entry to local state"
            );
            apply_command(&entry.command, &registry, &channels);
        }
        debug!("commit consumer shutting down: channel closed");
    })
}

/// Apply a single `ClusterCommand` to the local registries.
///
/// This function is idempotent: if the state already reflects the command
/// (e.g., the leader already applied it), the operation is a no-op.
fn apply_command(
    command: &ClusterCommand,
    registry: &UserRegistry,
    channels: &ChannelRegistry,
) {
    match command {
        ClusterCommand::UserRegistered {
            connection_id,
            nickname,
            username,
            realname,
            hostname,
            signon_time,
        } => {
            apply_user_registered(
                registry,
                *connection_id,
                nickname,
                username,
                realname,
                hostname,
                *signon_time,
            );
        }

        ClusterCommand::NickChanged { old_nick, new_nick } => {
            apply_nick_changed(registry, old_nick, new_nick);
        }

        ClusterCommand::UserQuit { nickname, .. } | ClusterCommand::UserKilled { nickname, .. } => {
            apply_user_quit(registry, channels, nickname);
        }

        ClusterCommand::UserAway { nickname, message } => {
            apply_user_away(registry, nickname, message.as_deref());
        }

        ClusterCommand::UserModeChanged {
            nickname,
            modes_added,
            modes_removed,
        } => {
            apply_user_mode_changed(registry, nickname, modes_added, modes_removed);
        }

        ClusterCommand::ChannelJoined {
            nickname,
            channel,
            status,
        } => {
            apply_channel_joined(channels, nickname, channel, status);
        }

        ClusterCommand::ChannelParted { nickname, channel, .. }
        | ClusterCommand::UserKicked {
            nickname, channel, ..
        } => {
            apply_channel_parted(channels, nickname, channel);
        }

        ClusterCommand::TopicSet { channel, topic } => {
            apply_topic_set(channels, channel, topic.as_ref());
        }

        ClusterCommand::ChannelModeChanged {
            channel,
            modes_added,
            modes_removed,
            key,
            user_limit,
            member_status_changes,
        } => {
            apply_channel_mode_changed(
                channels,
                channel,
                modes_added,
                modes_removed,
                key.as_deref(),
                *user_limit,
                member_status_changes,
            );
        }

        ClusterCommand::BanAdded {
            channel,
            mask,
            who_set,
            timestamp,
        } => {
            apply_ban_added(channels, channel, mask, who_set, *timestamp);
        }

        ClusterCommand::BanRemoved { channel, mask } => {
            apply_ban_removed(channels, channel, mask);
        }

        ClusterCommand::InviteAdded { channel, nickname } => {
            apply_invite_added(channels, channel, nickname);
        }

        ClusterCommand::OperGranted { nickname } => {
            apply_oper_granted(registry, nickname);
        }

        // Server topology and user migration are handled by other subsystems.
        ClusterCommand::ServerAdded { .. }
        | ClusterCommand::ServerRemoved { .. }
        | ClusterCommand::UserMigrated { .. }
        | ClusterCommand::Noop { .. } => {}
    }
}

fn apply_user_registered(
    registry: &UserRegistry,
    connection_id: u64,
    nickname: &str,
    username: &str,
    realname: &str,
    hostname: &str,
    signon_time: u64,
) {
    let Ok(nick) = Nickname::new(nickname) else {
        warn!(nickname, "commit consumer: invalid nickname in UserRegistered");
        return;
    };

    // Idempotent: skip if the user already exists (leader already registered).
    if registry.nick_in_use(&nick) {
        return;
    }

    // Create a dummy sender for remote users (follower path).
    // Messages sent to this channel are dropped.
    let (tx, _rx) = mpsc::unbounded_channel();
    let now = Instant::now();

    let session = UserSession {
        connection_id,
        nickname: nick,
        username: username.to_owned(),
        realname: realname.to_owned(),
        hostname: hostname.to_owned(),
        modes: HashSet::new(),
        away_message: None,
        connected_at: now,
        signon_time,
        last_active: now,
        registered: true,
        sender: tx,
    };

    if let Err(e) = registry.register(session) {
        debug!("commit consumer: register skipped: {e}");
    }
}

fn apply_nick_changed(registry: &UserRegistry, old_nick: &str, new_nick: &str) {
    let Ok(old) = Nickname::new(old_nick) else {
        return;
    };
    let Ok(new) = Nickname::new(new_nick) else {
        return;
    };

    // Idempotent: if old nick doesn't exist, the change was already applied.
    if registry.get_by_nick(&old).is_none() {
        return;
    }

    if let Err(e) = registry.change_nick(&old, new) {
        debug!("commit consumer: nick change skipped: {e}");
    }
}

fn apply_user_quit(registry: &UserRegistry, channels: &ChannelRegistry, nickname: &str) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };

    // Find the user's connection ID to remove from registry.
    let Some(session_arc) = registry.get_by_nick(&nick) else {
        return; // Already removed (leader path).
    };

    let conn_id = session_arc.read().expect("session lock poisoned").connection_id;

    // Remove from all channels first.
    for (_, ch_arc) in channels.list_all() {
        let mut ch = ch_arc.write().expect("channel lock poisoned");
        ch.members.remove(&nick);
    }

    // Clean up empty channels.
    for (name, _) in channels.list_all() {
        channels.remove_if_empty(&name);
    }

    registry.remove_by_connection(conn_id);
}

fn apply_user_away(registry: &UserRegistry, nickname: &str, message: Option<&str>) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };

    if let Some(session_arc) = registry.get_by_nick(&nick) {
        let mut session = session_arc.write().expect("session lock poisoned");
        session.away_message = message.map(String::from);
    }
}

fn apply_user_mode_changed(
    registry: &UserRegistry,
    nickname: &str,
    modes_added: &[String],
    modes_removed: &[String],
) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };

    if let Some(session_arc) = registry.get_by_nick(&nick) {
        let mut session = session_arc.write().expect("session lock poisoned");
        for mode_str in modes_added {
            if let Some(mode) = parse_user_mode(mode_str) {
                session.modes.insert(mode);
            }
        }
        for mode_str in modes_removed {
            if let Some(mode) = parse_user_mode(mode_str) {
                session.modes.remove(&mode);
            }
        }
    }
}

fn apply_channel_joined(
    channels: &ChannelRegistry,
    nickname: &str,
    channel: &str,
    status: &str,
) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    let member_status = parse_member_status(status);
    let ch_arc = channels.get_or_create(chan_name);
    let mut ch = ch_arc.write().expect("channel lock poisoned");

    // Idempotent: skip if the user is already a member.
    if ch.members.contains_key(&nick) {
        return;
    }

    ch.members.insert(nick, member_status);
}

fn apply_channel_parted(channels: &ChannelRegistry, nickname: &str, channel: &str) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        {
            let mut ch = ch_arc.write().expect("channel lock poisoned");
            ch.members.remove(&nick);
        }
        channels.remove_if_empty(&chan_name);
    }
}

fn apply_topic_set(
    channels: &ChannelRegistry,
    channel: &str,
    topic: Option<&crate::raft::cluster_command::TopicInfo>,
) {
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        let mut ch = ch_arc.write().expect("channel lock poisoned");
        ch.topic = topic.map(|t| (t.text.clone(), t.who.clone(), t.timestamp));
    }
}

fn apply_channel_mode_changed(
    channels: &ChannelRegistry,
    channel: &str,
    modes_added: &[String],
    modes_removed: &[String],
    key: Option<&str>,
    user_limit: Option<u32>,
    member_status_changes: &[(String, String)],
) {
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        let mut ch = ch_arc.write().expect("channel lock poisoned");

        for mode_str in modes_added {
            if let Some(mode) = parse_channel_mode(mode_str) {
                ch.modes.insert(mode);
            }
        }
        for mode_str in modes_removed {
            if let Some(mode) = parse_channel_mode(mode_str) {
                ch.modes.remove(&mode);
            }
        }

        if let Some(k) = key {
            ch.key = Some(k.to_owned());
        }
        if let Some(limit) = user_limit {
            ch.user_limit = Some(limit);
        }

        for (nick_str, status_str) in member_status_changes {
            if let Ok(nick) = Nickname::new(nick_str) {
                if ch.members.contains_key(&nick) {
                    let status = parse_member_status(status_str);
                    ch.members.insert(nick, status);
                }
            }
        }
    }
}

fn apply_ban_added(
    channels: &ChannelRegistry,
    channel: &str,
    mask: &str,
    who_set: &str,
    timestamp: u64,
) {
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        let mut ch = ch_arc.write().expect("channel lock poisoned");

        // Idempotent: skip if the ban already exists.
        if ch.ban_list.iter().any(|b| b.mask == mask) {
            return;
        }

        ch.ban_list.push(BanEntry {
            mask: mask.to_owned(),
            who_set: who_set.to_owned(),
            timestamp,
        });
    }
}

fn apply_ban_removed(channels: &ChannelRegistry, channel: &str, mask: &str) {
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        let mut ch = ch_arc.write().expect("channel lock poisoned");
        ch.ban_list.retain(|b| b.mask != mask);
    }
}

fn apply_invite_added(channels: &ChannelRegistry, channel: &str, nickname: &str) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };
    let Ok(chan_name) = ChannelName::new(channel) else {
        return;
    };

    if let Some(ch_arc) = channels.get(&chan_name) {
        let mut ch = ch_arc.write().expect("channel lock poisoned");
        ch.invite_list.insert(nick);
    }
}

fn apply_oper_granted(registry: &UserRegistry, nickname: &str) {
    let Ok(nick) = Nickname::new(nickname) else {
        return;
    };

    if let Some(session_arc) = registry.get_by_nick(&nick) {
        let mut session = session_arc.write().expect("session lock poisoned");
        session.modes.insert(pirc_common::UserMode::Operator);
    }
}

/// Parse a string mode letter to a `MemberStatus`.
fn parse_member_status(s: &str) -> MemberStatus {
    match s {
        "operator" => MemberStatus::Operator,
        "voiced" => MemberStatus::Voiced,
        _ => MemberStatus::Normal,
    }
}

/// Parse a single-character mode string to a `ChannelMode` (flag-type only).
fn parse_channel_mode(s: &str) -> Option<ChannelMode> {
    match s {
        "i" => Some(ChannelMode::InviteOnly),
        "m" => Some(ChannelMode::Moderated),
        "n" => Some(ChannelMode::NoExternalMessages),
        "s" => Some(ChannelMode::Secret),
        "t" => Some(ChannelMode::TopicProtected),
        _ => None,
    }
}

/// Parse a single-character user mode string to a `UserMode`.
fn parse_user_mode(s: &str) -> Option<pirc_common::UserMode> {
    match s {
        // "i" (invisible) and "w" (wallops) map to Normal for now
        "i" | "w" => Some(pirc_common::UserMode::Normal),
        "o" => Some(pirc_common::UserMode::Operator),
        "v" => Some(pirc_common::UserMode::Voiced),
        _ => None,
    }
}

#[cfg(test)]
#[path = "commit_consumer_tests.rs"]
mod commit_consumer_tests;
