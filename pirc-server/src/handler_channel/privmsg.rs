use std::sync::Arc;

use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{ERR_CANNOTSENDTOCHAN, ERR_NOSUCHNICK, RPL_AWAY};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::broadcast_to_channel;

/// Handle the PRIVMSG command from a registered user.
///
/// Routes messages to either a channel (broadcast to all members except sender)
/// or a specific user (direct delivery). Enforces channel modes (+m, +n) and
/// sends appropriate error replies.
pub fn handle_privmsg(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    handle_message_command(msg, connection_id, registry, channels, sender, Command::Privmsg);
}

/// Handle the NOTICE command from a registered user.
///
/// Same routing as PRIVMSG but NOTICE should never generate automatic replies
/// (per IRC convention). In particular, no RPL_AWAY is sent for NOTICE.
pub fn handle_notice(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    handle_message_command(msg, connection_id, registry, channels, sender, Command::Notice);
}

/// Shared implementation for PRIVMSG and NOTICE.
fn handle_message_command(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    command: Command,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };
    let nick_str = nick.to_string();
    let cmd_str = command.as_str();

    if msg.params.len() < 2 {
        send_numeric(
            sender,
            pirc_protocol::numeric::ERR_NEEDMOREPARAMS,
            &[&nick_str, &cmd_str],
            "Not enough parameters",
        );
        return;
    }

    let target = &msg.params[0];
    let text = &msg.params[1];

    // Determine if target is a channel or a user.
    if target.starts_with('#') || target.starts_with('&') {
        handle_channel_message(
            target, text, &nick, &nick_str, &username, &hostname, registry, channels, sender,
            &command,
        );
    } else {
        handle_user_message(
            target, text, &nick, &nick_str, &username, &hostname, registry, sender, &command,
        );
    }
}

/// Route a message to a channel.
fn handle_channel_message(
    target: &str,
    text: &str,
    nick: &Nickname,
    nick_str: &str,
    username: &str,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    command: &Command,
) {
    // Validate channel name.
    let Ok(chan_name) = ChannelName::new(target) else {
        send_numeric(
            sender,
            ERR_CANNOTSENDTOCHAN,
            &[nick_str, target],
            "Cannot send to channel",
        );
        return;
    };

    // Look up channel.
    let Some(channel_arc) = channels.get(&chan_name) else {
        send_numeric(
            sender,
            ERR_CANNOTSENDTOCHAN,
            &[nick_str, target],
            "Cannot send to channel",
        );
        return;
    };

    // Check membership and mode restrictions.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");
        let is_member = channel.members.contains_key(nick);
        let member_status = channel.members.get(nick).copied();

        // +n (NoExternalMessages): non-members cannot send.
        if !is_member && channel.modes.contains(&ChannelMode::NoExternalMessages) {
            send_numeric(
                sender,
                ERR_CANNOTSENDTOCHAN,
                &[nick_str, target],
                "Cannot send to channel",
            );
            return;
        }

        // +m (Moderated): only voiced or operators can speak.
        if channel.modes.contains(&ChannelMode::Moderated) {
            let can_speak = match member_status {
                Some(MemberStatus::Operator) | Some(MemberStatus::Voiced) => true,
                _ => false,
            };
            if !can_speak {
                send_numeric(
                    sender,
                    ERR_CANNOTSENDTOCHAN,
                    &[nick_str, target],
                    "Cannot send to channel",
                );
                return;
            }
        }
    }

    // Build and broadcast the message to all channel members except the sender.
    let out_msg = Message::builder(command.clone())
        .prefix(Prefix::User {
            nick: nick.clone(),
            user: username.to_owned(),
            host: hostname.to_owned(),
        })
        .param(target)
        .trailing(text)
        .build();
    broadcast_to_channel(&channel_arc, &out_msg, Some(nick), registry);
}

/// Route a message to a specific user.
fn handle_user_message(
    target: &str,
    text: &str,
    nick: &Nickname,
    nick_str: &str,
    username: &str,
    hostname: &str,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    command: &Command,
) {
    let target_nick = match Nickname::new(target) {
        Ok(n) => n,
        Err(_) => {
            send_numeric(
                sender,
                ERR_NOSUCHNICK,
                &[nick_str, target],
                "No such nick/channel",
            );
            return;
        }
    };

    let Some(target_session_arc) = registry.get_by_nick(&target_nick) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[nick_str, target],
            "No such nick/channel",
        );
        return;
    };

    // Build the message.
    let out_msg = Message::builder(command.clone())
        .prefix(Prefix::User {
            nick: nick.clone(),
            user: username.to_owned(),
            host: hostname.to_owned(),
        })
        .param(target)
        .trailing(text)
        .build();

    // Send to target user.
    {
        let target_session = target_session_arc.read().expect("session lock poisoned");
        let _ = target_session.sender.send(out_msg);

        // For PRIVMSG only: send RPL_AWAY if target is away.
        if *command == Command::Privmsg {
            if let Some(ref away_msg) = target_session.away_message {
                send_numeric(
                    sender,
                    RPL_AWAY,
                    &[nick_str, target],
                    away_msg,
                );
            }
        }
    }
}
