use std::sync::Arc;

use pirc_common::{Nickname, UserMode};
use pirc_protocol::numeric::{
    ERR_NEEDMOREPARAMS, ERR_NOOPERHOST, ERR_NOPRIVILEGES, ERR_NOSUCHNICK, ERR_PASSWDMISMATCH,
    RPL_YOUREOPER,
};
use pirc_protocol::{Command, Message, Prefix};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tracing::info;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::handler::{broadcast_quit_and_remove, send_numeric, SERVER_NAME};
use crate::registry::UserRegistry;
use crate::user::UserSession;

/// Check if a user has IRC operator privileges.
pub(crate) fn is_oper(session: &UserSession) -> bool {
    session.modes.contains(&UserMode::Operator)
}

/// Handle the OPER command to authenticate as an IRC operator.
///
/// `OPER <name> <password>` validates credentials from the server config,
/// optionally checks host mask, sets `UserMode::Operator`, and sends
/// `RPL_YOUREOPER` and a MODE notification on success.
pub(crate) fn handle_oper(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    config: &ServerConfig,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.to_string(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    if msg.params.len() < 2 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick, "OPER"],
            "Not enough parameters",
        );
        return;
    }

    let oper_name = &msg.params[0];
    let oper_password = &msg.params[1];

    // Look up operator credentials by name.
    let oper_config = config.operators.iter().find(|o| o.name == *oper_name);

    let Some(oper_config) = oper_config else {
        send_numeric(sender, ERR_PASSWDMISMATCH, &[&nick], "Password incorrect");
        return;
    };

    // Verify password using constant-time comparison to prevent timing attacks.
    if oper_config.password.as_bytes().ct_eq(oper_password.as_bytes()).unwrap_u8() != 1 {
        send_numeric(sender, ERR_PASSWDMISMATCH, &[&nick], "Password incorrect");
        return;
    }

    // Check host mask if configured.
    if let Some(ref mask) = oper_config.host_mask {
        if !host_matches_mask(&hostname, mask) {
            send_numeric(sender, ERR_NOOPERHOST, &[&nick], "No O-lines for your host");
            return;
        }
    }

    // Grant operator status.
    {
        let mut session = session_arc.write().expect("session lock poisoned");
        session.modes.insert(UserMode::Operator);
    }

    // Send RPL_YOUREOPER (381).
    send_numeric(
        sender,
        RPL_YOUREOPER,
        &[&nick],
        "You are now an IRC operator",
    );

    // Send MODE change notification: :nick!user@host MODE nick :+o
    let mode_msg = Message::builder(Command::Mode)
        .prefix(Prefix::User {
            nick: nick
                .parse()
                .unwrap_or_else(|_| pirc_common::Nickname::new("*").unwrap()),
            user: username,
            host: hostname,
        })
        .param(&nick)
        .trailing("+o")
        .build();
    let _ = sender.send(mode_msg);
}

/// Handle the KILL command from an IRC operator.
///
/// `KILL <nick> <reason>` forcibly disconnects a user from the server.
/// Only IRC operators may use this command.
pub(crate) fn handle_kill(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
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
        (
            session.nickname.to_string(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick, "KILL"],
            "Not enough parameters",
        );
        return;
    }

    let target_nick_str = &msg.params[0];
    let reason = msg.params.get(1).map_or("Killed", |s| s.as_str());

    let Ok(target_nick) = Nickname::new(target_nick_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[&nick, target_nick_str],
            "No such nick/channel",
        );
        return;
    };

    let Some(target_arc) = registry.get_by_nick(&target_nick) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[&nick, target_nick_str],
            "No such nick/channel",
        );
        return;
    };

    let (target_conn_id, target_username, target_hostname, target_sender) = {
        let target = target_arc.read().expect("session lock poisoned");
        (
            target.connection_id,
            target.username.clone(),
            target.hostname.clone(),
            target.sender.clone(),
        )
    };

    // Build KILL message and send to target.
    let kill_msg = Message::builder(Command::Kill)
        .prefix(Prefix::User {
            nick: nick.parse().unwrap_or_else(|_| Nickname::new("*").unwrap()),
            user: username,
            host: hostname,
        })
        .param(target_nick_str)
        .trailing(reason)
        .build();
    let _ = target_sender.send(kill_msg);

    // Send ERROR to target.
    let error_msg = Message::builder(Command::Error)
        .trailing(&format!(
            "Closing Link: {target_hostname} (Killed ({nick} ({reason})))"
        ))
        .build();
    let _ = target_sender.send(error_msg);

    // Build QUIT message for channel broadcast.
    let quit_msg = Message::builder(Command::Quit)
        .prefix(Prefix::User {
            nick: target_nick.clone(),
            user: target_username,
            host: target_hostname,
        })
        .trailing(&format!("Killed ({nick} ({reason}))"))
        .build();

    broadcast_quit_and_remove(&target_nick, &quit_msg, channels, registry);
    registry.remove_by_connection(target_conn_id);
}

/// Handle the DIE command from an IRC operator.
///
/// Sends a server notice to all connected users and signals server shutdown.
/// Returns `true` if the shutdown should proceed.
pub(crate) fn handle_die(
    _msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) -> bool {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return false;
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
            return false;
        }
        session.nickname.to_string()
    };

    info!(operator = %nick, "DIE command received, initiating server shutdown");

    // Broadcast server notice to all connected users.
    let notice = Message::builder(Command::Notice)
        .prefix(Prefix::server(SERVER_NAME))
        .param("*")
        .trailing(&format!("Server shutting down by operator {nick}"))
        .build();

    for session_arc in registry.iter_sessions() {
        let session = session_arc.read().expect("session lock poisoned");
        let _ = session.sender.send(notice.clone());
    }

    true
}

/// Handle the RESTART command from an IRC operator.
///
/// Sends a server notice to all connected users and signals server restart.
/// For v1, this behaves identically to DIE (the process manager is expected
/// to restart the server).
/// Returns `true` if the shutdown should proceed.
pub(crate) fn handle_restart(
    _msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) -> bool {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return false;
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
            return false;
        }
        session.nickname.to_string()
    };

    info!(operator = %nick, "RESTART command received, initiating server restart");

    // Broadcast server notice to all connected users.
    let notice = Message::builder(Command::Notice)
        .prefix(Prefix::server(SERVER_NAME))
        .param("*")
        .trailing(&format!("Server restarting by operator {nick}"))
        .build();

    for session_arc in registry.iter_sessions() {
        let session = session_arc.read().expect("session lock poisoned");
        let _ = session.sender.send(notice.clone());
    }

    true
}

/// Handle the WALLOPS command from an IRC operator.
///
/// `WALLOPS :message` sends a message to all users with operator mode set.
pub(crate) fn handle_wallops(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
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
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[nick.as_ref(), "WALLOPS"],
            "Not enough parameters",
        );
        return;
    }

    let text = &msg.params[0];

    let wallops_msg = Message::builder(Command::Wallops)
        .prefix(Prefix::User {
            nick,
            user: username,
            host: hostname,
        })
        .trailing(text)
        .build();

    // Send to all operators.
    for session_arc in registry.iter_sessions() {
        let session = session_arc.read().expect("session lock poisoned");
        if session.modes.contains(&UserMode::Operator) {
            let _ = session.sender.send(wallops_msg.clone());
        }
    }
}

/// Simple glob-style host mask matching.
///
/// Supports `*` as a wildcard that matches any sequence of characters.
pub(crate) fn host_matches_mask(host: &str, mask: &str) -> bool {
    let parts: Vec<&str> = mask.split('*').collect();

    if parts.len() == 1 {
        // No wildcard: exact match.
        return host == mask;
    }

    let mut pos = 0;

    // First part must match at the start.
    if !parts[0].is_empty() {
        if !host.starts_with(parts[0]) {
            return false;
        }
        pos = parts[0].len();
    }

    // Last part must match at the end.
    let last = parts[parts.len() - 1];
    if !last.is_empty() && !host.ends_with(last) {
        return false;
    }

    // Middle parts must appear in order.
    for &part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if let Some(idx) = host[pos..].find(part) {
            pos += idx + part.len();
        } else {
            return false;
        }
    }

    true
}
