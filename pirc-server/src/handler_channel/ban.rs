use std::sync::Arc;

use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, RPL_BANLIST,
    RPL_ENDOFBANLIST,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::broadcast_to_channel;

/// Handle the BAN command from a registered user (pirc extension).
///
/// `BAN #channel` — lists current bans (RPL_BANLIST + RPL_ENDOFBANLIST)
/// `BAN #channel mask` — adds a ban mask
/// `BAN #channel -mask` — removes a ban mask
///
/// Wraps MODE +b/-b functionality. Setter must be on the channel and have
/// operator status.
pub fn handle_ban(
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
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };
    let nick_str = nick.to_string();

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick_str, "BAN"],
            "Not enough parameters",
        );
        return;
    }

    let chan_str = &msg.params[0];

    // Validate channel name.
    let Ok(chan_name) = pirc_common::ChannelName::new(chan_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHCHANNEL,
            &[&nick_str, chan_str],
            "No such channel",
        );
        return;
    };

    // Look up channel.
    let Some(channel_arc) = channels.get(&chan_name) else {
        send_numeric(
            sender,
            ERR_NOSUCHCHANNEL,
            &[&nick_str, chan_str],
            "No such channel",
        );
        return;
    };

    // Ban list query (no mask argument).
    if msg.params.len() < 2 {
        let channel = channel_arc.read().expect("channel lock poisoned");
        for ban in &channel.ban_list {
            send_numeric(
                sender,
                RPL_BANLIST,
                &[
                    &nick_str,
                    chan_name.as_ref(),
                    &ban.mask,
                    &ban.who_set,
                    &ban.timestamp.to_string(),
                ],
                "",
            );
        }
        send_numeric(
            sender,
            RPL_ENDOFBANLIST,
            &[&nick_str, chan_name.as_ref()],
            "End of channel ban list",
        );
        return;
    }

    // Ban set/unset: check membership and operator status.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");

        let Some(status) = channel.members.get(&nick) else {
            send_numeric(
                sender,
                ERR_NOTONCHANNEL,
                &[&nick_str, chan_str],
                "You're not on that channel",
            );
            return;
        };

        if *status != MemberStatus::Operator {
            send_numeric(
                sender,
                ERR_CHANOPRIVSNEEDED,
                &[&nick_str, chan_str],
                "You're not channel operator",
            );
            return;
        }
    }

    let mask_str = &msg.params[1];

    // Check if removing (prefix with '-') or adding.
    let (removing, mask) = if let Some(stripped) = mask_str.strip_prefix('-') {
        (true, stripped.to_owned())
    } else {
        (false, mask_str.to_owned())
    };

    if removing {
        // Remove ban.
        let mut channel = channel_arc.write().expect("channel lock poisoned");
        channel
            .ban_list
            .retain(|b| !b.mask.eq_ignore_ascii_case(&mask));
    } else {
        // Add ban.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut channel = channel_arc.write().expect("channel lock poisoned");
        channel.ban_list.push(crate::channel::BanEntry {
            mask: mask.clone(),
            who_set: nick_str.clone(),
            timestamp: now,
        });
    }

    // Broadcast mode change to channel members.
    let mode_char = if removing { "-b" } else { "+b" };
    let mode_msg = Message::builder(Command::Mode)
        .prefix(Prefix::User {
            nick,
            user: username,
            host: hostname,
        })
        .param(chan_name.as_ref())
        .param(mode_char)
        .param(&mask)
        .build();
    broadcast_to_channel(&channel_arc, &mode_msg, None, registry);
}
