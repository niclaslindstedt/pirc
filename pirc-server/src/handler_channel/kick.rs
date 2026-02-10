use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL,
    ERR_USERNOTINCHANNEL,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::broadcast_to_channel;

/// Handle the KICK command from a registered user.
///
/// `KICK #channel target [:reason]`
/// Removes target from the channel. Only channel operators may kick.
/// Default reason is the kicker's nick if none provided.
pub fn handle_kick(
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

    if msg.params.len() < 2 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick_str, "KICK"],
            "Not enough parameters",
        );
        return;
    }

    let chan_str = &msg.params[0];
    let target_str = &msg.params[1];
    let reason = msg.params.get(2).map(String::as_str).unwrap_or(&nick_str);

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

    // Parse target nickname.
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_USERNOTINCHANNEL,
            &[&nick_str, target_str, chan_str],
            "They aren't on that channel",
        );
        return;
    };

    // Validate kicker is on the channel, has operator status, and target is on the channel.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");

        // Check kicker is on the channel.
        let Some(kicker_status) = channel.members.get(&nick) else {
            send_numeric(
                sender,
                ERR_NOTONCHANNEL,
                &[&nick_str, chan_str],
                "You're not on that channel",
            );
            return;
        };

        // Check kicker has operator status.
        if *kicker_status != MemberStatus::Operator {
            send_numeric(
                sender,
                ERR_CHANOPRIVSNEEDED,
                &[&nick_str, chan_str],
                "You're not channel operator",
            );
            return;
        }

        // Check target is on the channel.
        if !channel.members.contains_key(&target_nick) {
            send_numeric(
                sender,
                ERR_USERNOTINCHANNEL,
                &[&nick_str, target_str, chan_str],
                "They aren't on that channel",
            );
            return;
        }
    }

    // Build the KICK message with user prefix.
    let kick_msg = Message::builder(Command::Kick)
        .prefix(Prefix::User {
            nick: nick.clone(),
            user: username,
            host: hostname,
        })
        .param(chan_name.as_ref())
        .param(target_str)
        .trailing(reason)
        .build();

    // Broadcast KICK to all channel members (including both kicker and target).
    broadcast_to_channel(&channel_arc, &kick_msg, None, registry);

    // Remove target from channel.
    {
        let mut channel = channel_arc.write().expect("channel lock poisoned");
        channel.members.remove(&target_nick);
    }

    // Clean up empty channel.
    channels.remove_if_empty(&chan_name);
}
