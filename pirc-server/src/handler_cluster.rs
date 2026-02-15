use std::sync::Arc;
use std::time::Duration;

use pirc_protocol::numeric::ERR_NOPRIVILEGES;
use pirc_protocol::Message;
use tokio::sync::mpsc;

use crate::cluster::InviteKeyStore;
use crate::handler::{send_numeric, SERVER_NAME};
use crate::handler_oper::is_oper;
use crate::raft::{NodeId, RaftHandle, SharedPeerMap};
use crate::registry::UserRegistry;

/// Shared cluster state made available to command handlers.
pub struct ClusterContext {
    pub invite_keys: Arc<tokio::sync::Mutex<InviteKeyStore>>,
    pub raft_handle: Arc<RaftHandle<String>>,
    pub shared_peer_map: SharedPeerMap,
    pub self_id: NodeId,
}

/// Send a server NOTICE to the client.
fn send_notice(sender: &mpsc::UnboundedSender<Message>, nick: &str, text: &str) {
    let msg = Message::builder(pirc_protocol::Command::Notice)
        .prefix(pirc_protocol::Prefix::server(SERVER_NAME))
        .param(nick)
        .trailing(text)
        .build();
    let _ = sender.send(msg);
}

/// Handle `PIRC INVITE-KEY GENERATE [ttl_secs]`.
///
/// Requires operator privileges. Generates a new single-use invite key.
pub fn handle_invite_key_generate(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        if !is_oper(&session) {
            send_numeric(
                sender,
                ERR_NOPRIVILEGES,
                &[session.nickname.as_ref()],
                "Permission Denied- You're not an IRC operator",
            );
            return;
        }
        session.nickname.to_string()
    };

    // Parse optional TTL from first param (seconds).
    let ttl = msg
        .params
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs);

    let key = {
        let mut store = ctx.invite_keys.try_lock().expect("invite_keys lock");
        store.create(ctx.self_id, ttl, true)
    };

    send_notice(sender, &nick, &format!("Invite key generated: {key}"));
    if let Some(ttl) = ttl {
        send_notice(
            sender,
            &nick,
            &format!("Expires in {} seconds", ttl.as_secs()),
        );
    } else {
        send_notice(sender, &nick, "Expires in 86400 seconds (24h)");
    }
}

/// Handle `PIRC INVITE-KEY LIST`.
///
/// Requires operator privileges. Lists all active (non-expired, non-revoked) keys.
pub fn handle_invite_key_list(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        if !is_oper(&session) {
            send_numeric(
                sender,
                ERR_NOPRIVILEGES,
                &[session.nickname.as_ref()],
                "Permission Denied- You're not an IRC operator",
            );
            return;
        }
        session.nickname.to_string()
    };

    let store = ctx.invite_keys.try_lock().expect("invite_keys lock");
    let records = store.list();
    let now = std::time::SystemTime::now();

    let active: Vec<_> = records
        .iter()
        .filter(|r| !(r.revoked || r.is_expired(now) || r.single_use && r.used))
        .collect();

    if active.is_empty() {
        send_notice(sender, &nick, "No active invite keys.");
        return;
    }

    send_notice(
        sender,
        &nick,
        &format!("Active invite keys ({}):", active.len()),
    );
    for record in active {
        let remaining = record
            .expires_at
            .duration_since(now)
            .unwrap_or_default()
            .as_secs();
        let status = if record.used { "used" } else { "unused" };
        let use_type = if record.single_use {
            "single-use"
        } else {
            "multi-use"
        };
        send_notice(
            sender,
            &nick,
            &format!(
                "  {} ({}, {}, expires in {}s)",
                record.key, use_type, status, remaining
            ),
        );
    }
}

/// Handle `PIRC INVITE-KEY REVOKE <token>`.
///
/// Requires operator privileges.
pub fn handle_invite_key_revoke(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        if !is_oper(&session) {
            send_numeric(
                sender,
                ERR_NOPRIVILEGES,
                &[session.nickname.as_ref()],
                "Permission Denied- You're not an IRC operator",
            );
            return;
        }
        session.nickname.to_string()
    };

    let Some(token) = msg.params.first() else {
        send_notice(sender, &nick, "Usage: /invite-key revoke <key>");
        return;
    };

    let mut store = ctx.invite_keys.try_lock().expect("invite_keys lock");
    if store.revoke(token) {
        send_notice(sender, &nick, "Invite key revoked.");
    } else {
        send_notice(sender, &nick, "Invite key not found.");
    }
}

/// Handle `PIRC CLUSTER STATUS`.
///
/// Shows Raft node state: node ID, role, term, leader, cluster members.
pub fn handle_cluster_status(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.to_string()
    };

    let state = ctx.raft_handle.state();
    let term = ctx.raft_handle.current_term();
    let leader = ctx.raft_handle.current_leader();

    send_notice(sender, &nick, "=== Cluster Status ===");
    send_notice(
        sender,
        &nick,
        &format!("Node ID: {}", ctx.self_id.as_u64()),
    );
    send_notice(sender, &nick, &format!("Role: {state}"));
    send_notice(sender, &nick, &format!("Term: {}", term.as_u64()));
    match leader {
        Some(id) => send_notice(sender, &nick, &format!("Leader: node {}", id.as_u64())),
        None => send_notice(sender, &nick, "Leader: unknown"),
    }
    send_notice(
        sender,
        &nick,
        &format!("Is leader: {}", ctx.raft_handle.is_leader()),
    );
}

/// Handle `PIRC CLUSTER MEMBERS`.
///
/// Lists all cluster members with their node ID and address.
pub fn handle_cluster_members(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.to_string()
    };

    send_notice(sender, &nick, "=== Cluster Members ===");
    send_notice(
        sender,
        &nick,
        &format!("  Node {} (self)", ctx.self_id.as_u64()),
    );

    let map = ctx.shared_peer_map.try_read().expect("peer_map lock");
    let mut entries: Vec<_> = map.entries().collect();
    entries.sort_by_key(|(id, _)| id.as_u64());

    for (id, addr) in entries {
        send_notice(
            sender,
            &nick,
            &format!("  Node {} at {addr}", id.as_u64()),
        );
    }
}

/// Handle `PIRC NETWORK INFO`.
///
/// Shows network-level info: total servers, connected users on this server.
pub fn handle_network_info(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    ctx: &ClusterContext,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.to_string()
    };

    let peer_count = ctx.shared_peer_map.try_read().expect("peer_map lock").entries().count();
    let total_servers = peer_count + 1; // +1 for self
    let local_users = registry.connection_count();

    send_notice(sender, &nick, "=== Network Info ===");
    send_notice(
        sender,
        &nick,
        &format!("Total servers: {total_servers}"),
    );
    send_notice(
        sender,
        &nick,
        &format!("Local users: {local_users}"),
    );
}

#[cfg(test)]
#[path = "handler_cluster_tests.rs"]
mod tests;
