//! Handlers for group chat protocol commands.
//!
//! Processes `PIRC GROUP` subcommands: `CREATE`, `INVITE`, `JOIN`,
//! `LEAVE`, `MEMBERS`, and group-scoped signaling relay (`KEYEX`,
//! `P2P-OFFER`, `P2P-ANSWER`, `P2P-ICE`, `MSG`).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pirc_common::types::GroupId;
use pirc_common::Nickname;
use pirc_protocol::numeric::{ERR_NEEDMOREPARAMS, ERR_NOSUCHNICK};
use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::group_registry::GroupRegistry;
use crate::handler::{send_numeric, SERVER_NAME};
use crate::offline_store::OfflineMessageStore;
use crate::registry::UserRegistry;

/// Returns the current Unix timestamp in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Handle `PIRC GROUP CREATE <group_name>`.
///
/// Creates a new group with the sender as creator/admin and sends back
/// the assigned group ID.
pub fn handle_group_create(
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // params[0] = group_name
    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "GROUP CREATE"],
            "Not enough parameters",
        );
        return;
    }

    let group_name = &msg.params[0];
    let group_id = group_registry.create_group(
        group_name.clone(),
        sender_nick.to_string(),
        now_secs(),
    );

    info!(
        group_id = group_id.as_u64(),
        group_name = %group_name,
        creator = %sender_nick,
        "group created"
    );

    // Reply: :server PIRC GROUP CREATE <group_id> <group_name>
    let reply = Message::builder(Command::Pirc(PircSubcommand::GroupCreate))
        .prefix(Prefix::User {
            nick: sender_nick,
            user: username,
            host: hostname,
        })
        .param(&group_id.as_u64().to_string())
        .param(group_name)
        .build();
    let _ = sender.send(reply);
}

/// Handle `PIRC GROUP INVITE <group_id> <target_nick>`.
///
/// Validates that the sender is a group member, then relays the
/// invitation to the target user.
pub fn handle_group_invite(
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // params[0] = group_id, params[1] = target_nick
    if msg.params.len() < 2 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "GROUP INVITE"],
            "Not enough parameters",
        );
        return;
    }

    let Ok(group_id) = msg.params[0].parse::<GroupId>() else {
        send_server_notice(sender, &sender_nick, "Invalid group ID");
        return;
    };

    if !group_registry.is_member(group_id, sender_nick.as_ref()) {
        send_server_notice(sender, &sender_nick, "You are not a member of this group");
        return;
    }

    let target_str = &msg.params[1];
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_str],
            "No such nick/channel",
        );
        return;
    };

    // Relay the invite to the target
    let group_name = group_registry
        .group_name(group_id)
        .unwrap_or_default();
    let invite_msg = Message::builder(Command::Pirc(PircSubcommand::GroupInvite))
        .prefix(Prefix::User {
            nick: sender_nick.clone(),
            user: username,
            host: hostname,
        })
        .param(&group_id.as_u64().to_string())
        .param(target_nick.as_ref())
        .param(&group_name)
        .build();

    if let Some(target_session_arc) = user_registry.get_by_nick(&target_nick) {
        let target_session = target_session_arc.read().expect("session lock poisoned");
        let _ = target_session.sender.send(invite_msg);
        debug!(
            group_id = group_id.as_u64(),
            inviter = %sender_nick,
            target = %target_nick,
            "group invite relayed"
        );
    } else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_nick.as_ref()],
            "No such nick/channel",
        );
    }
}

/// Handle `PIRC GROUP JOIN <group_id>`.
///
/// Adds the sender to the group membership and broadcasts the join to
/// all existing members. Sends the member list to the new member.
pub fn handle_group_join(
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // params[0] = group_id
    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "GROUP JOIN"],
            "Not enough parameters",
        );
        return;
    }

    let Ok(group_id) = msg.params[0].parse::<GroupId>() else {
        send_server_notice(sender, &sender_nick, "Invalid group ID");
        return;
    };

    if !group_registry.exists(group_id) {
        send_server_notice(sender, &sender_nick, "Group does not exist");
        return;
    }

    if !group_registry.add_member(group_id, sender_nick.to_string(), now_secs()) {
        send_server_notice(sender, &sender_nick, "Already a member of this group");
        return;
    }

    info!(
        group_id = group_id.as_u64(),
        member = %sender_nick,
        "member joined group"
    );

    let members = group_registry.members(group_id);

    // Broadcast GROUP JOIN to all members (including the new one)
    let join_msg = Message::builder(Command::Pirc(PircSubcommand::GroupJoin))
        .prefix(Prefix::User {
            nick: sender_nick.clone(),
            user: username.clone(),
            host: hostname.clone(),
        })
        .param(&group_id.as_u64().to_string())
        .build();

    broadcast_to_group_members(user_registry, &members, &join_msg);

    // Send GROUP MEMBERS to the new joiner
    let mut members_builder = Message::builder(Command::Pirc(PircSubcommand::GroupMembers))
        .prefix(Prefix::server(SERVER_NAME))
        .param(&group_id.as_u64().to_string());
    for member in &members {
        members_builder = members_builder.param(member);
    }
    let members_msg = members_builder.build();
    let _ = sender.send(members_msg);
}

/// Handle `PIRC GROUP LEAVE <group_id>`.
///
/// Removes the sender from the group and broadcasts the leave to
/// remaining members. Handles admin transfer and group destruction.
pub fn handle_group_leave(
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // params[0] = group_id
    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "GROUP LEAVE"],
            "Not enough parameters",
        );
        return;
    }

    let Ok(group_id) = msg.params[0].parse::<GroupId>() else {
        send_server_notice(sender, &sender_nick, "Invalid group ID");
        return;
    };

    handle_member_leave(
        group_id,
        &sender_nick,
        &username,
        &hostname,
        user_registry,
        group_registry,
    );
}

/// Handles a member leaving a group (explicit leave or disconnect).
///
/// Removes the member, broadcasts the leave, handles admin transfer.
/// Can be called both from explicit `GROUP LEAVE` and from disconnect handling.
pub fn handle_member_leave(
    group_id: GroupId,
    nick: &Nickname,
    username: &str,
    hostname: &str,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
) {
    let Some(result) = group_registry.remove_member(group_id, nick.as_ref()) else {
        return;
    };

    info!(
        group_id = group_id.as_u64(),
        member = %nick,
        destroyed = result.group_destroyed,
        "member left group"
    );

    if result.group_destroyed {
        // No one to notify
        return;
    }

    // Broadcast GROUP LEAVE to remaining members
    let leave_msg = Message::builder(Command::Pirc(PircSubcommand::GroupLeave))
        .prefix(Prefix::User {
            nick: nick.clone(),
            user: username.to_owned(),
            host: hostname.to_owned(),
        })
        .param(&group_id.as_u64().to_string())
        .build();

    broadcast_to_group_members(user_registry, &result.remaining, &leave_msg);
}

/// Handle `PIRC GROUP MSG <group_id> <target> <encrypted_payload>`.
///
/// Relays an encrypted group message from sender to a specific target
/// member via the server. If the target is offline, the message is
/// queued for delivery on reconnect.
pub fn handle_group_message_relay(
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    offline_store: &Arc<OfflineMessageStore>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // params[0] = group_id, params[1] = target, params[2] = encrypted_payload
    if msg.params.len() < 3 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "GROUP MSG"],
            "Not enough parameters",
        );
        return;
    }

    let Ok(group_id) = msg.params[0].parse::<GroupId>() else {
        send_server_notice(sender, &sender_nick, "Invalid group ID");
        return;
    };

    if !group_registry.is_member(group_id, sender_nick.as_ref()) {
        send_server_notice(sender, &sender_nick, "You are not a member of this group");
        return;
    }

    let target_str = &msg.params[1];
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_str],
            "No such nick/channel",
        );
        return;
    };

    // Relay to target
    let relay_msg = Message::builder(Command::Pirc(PircSubcommand::GroupMessage))
        .prefix(Prefix::User {
            nick: sender_nick.clone(),
            user: username,
            host: hostname,
        })
        .param(&group_id.as_u64().to_string())
        .param(target_nick.as_ref())
        .param(&msg.params[2])
        .build();

    if let Some(target_session) = user_registry.get_by_nick(&target_nick) {
        let session = target_session.read().expect("session lock poisoned");
        let _ = session.sender.send(relay_msg);
    } else {
        // Target is offline — queue for delivery on reconnect.
        offline_store.queue_message(&target_nick, relay_msg);
        let notice = Message::builder(Command::Notice)
            .prefix(Prefix::server(SERVER_NAME))
            .param(sender_nick.as_ref())
            .trailing(&format!(
                "{target_nick} is offline. Group message will be delivered when they reconnect."
            ))
            .build();
        let _ = sender.send(notice);
        debug!(
            group_id = group_id.as_u64(),
            sender = %sender_nick,
            target = %target_nick,
            "queued group message for offline user"
        );
    }
}

/// Handle group-scoped signaling relay commands.
///
/// Routes `PIRC GROUP KEYEX`, `P2P-OFFER`, `P2P-ANSWER`, and `P2P-ICE`
/// messages to the target user. These are time-sensitive and not queued
/// for offline delivery.
pub fn handle_group_signaling_relay(
    subcommand: &PircSubcommand,
    msg: &Message,
    connection_id: u64,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = user_registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    let cmd_keyword = subcommand.as_str();

    // params[0] = group_id, params[1] = target, params[2..] = data
    if msg.params.len() < 2 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), cmd_keyword],
            "Not enough parameters",
        );
        return;
    }

    let Ok(group_id) = msg.params[0].parse::<GroupId>() else {
        send_server_notice(sender, &sender_nick, "Invalid group ID");
        return;
    };

    if !group_registry.is_member(group_id, sender_nick.as_ref()) {
        send_server_notice(sender, &sender_nick, "You are not a member of this group");
        return;
    }

    let target_str = &msg.params[1];
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_str],
            "No such nick/channel",
        );
        return;
    };

    // Build relayed message with sender prefix and all original params
    let mut builder = Message::builder(Command::Pirc(subcommand.clone()))
        .prefix(Prefix::User {
            nick: sender_nick.clone(),
            user: username,
            host: hostname,
        });
    for param in &msg.params {
        builder = builder.param(param);
    }
    let relay_msg = builder.build();

    if let Some(target_session) = user_registry.get_by_nick(&target_nick) {
        let session = target_session.read().expect("session lock poisoned");
        let _ = session.sender.send(relay_msg);
    } else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_nick.as_ref()],
            "No such nick/channel",
        );
    }
}

/// Remove a disconnected user from all groups they are a member of.
///
/// Called when a user disconnects (QUIT or connection drop). Broadcasts
/// `GROUP LEAVE` to remaining members of each group.
pub fn remove_user_from_all_groups(
    nick: &Nickname,
    username: &str,
    hostname: &str,
    user_registry: &Arc<UserRegistry>,
    group_registry: &Arc<GroupRegistry>,
) {
    // DashMap iteration is lock-free and provides a snapshot view.
    let group_ids: Vec<GroupId> = group_registry
        .groups_for_member(nick.as_ref());

    for group_id in group_ids {
        handle_member_leave(
            group_id,
            nick,
            username,
            hostname,
            user_registry,
            group_registry,
        );
    }
}

/// Broadcast a message to all members of a group.
fn broadcast_to_group_members(
    user_registry: &Arc<UserRegistry>,
    members: &[String],
    msg: &Message,
) {
    for member_nick_str in members {
        if let Ok(nick) = Nickname::new(member_nick_str) {
            if let Some(session_arc) = user_registry.get_by_nick(&nick) {
                let session = session_arc.read().expect("session lock poisoned");
                let _ = session.sender.send(msg.clone());
            }
        }
    }
}

/// Send a server NOTICE to a user.
fn send_server_notice(
    sender: &mpsc::UnboundedSender<Message>,
    nick: &Nickname,
    text: &str,
) {
    let notice = Message::builder(Command::Notice)
        .prefix(Prefix::server(SERVER_NAME))
        .param(nick.as_ref())
        .trailing(text)
        .build();
    let _ = sender.send(notice);
}

#[cfg(test)]
#[path = "handler_group_tests.rs"]
mod tests;
