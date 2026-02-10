use std::sync::Arc;

use pirc_common::{ChannelMode, Nickname};
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOSUCHNICK,
    ERR_NOTONCHANNEL, ERR_USERONCHANNEL, RPL_INVITING,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

/// Handle the INVITE command from a registered user.
///
/// `INVITE target #channel`
/// Invites a target user to a channel. If the channel is +i (invite-only),
/// the inviter must be a channel operator. Adds the target to the channel's
/// invite list so they can bypass +i restrictions on JOIN.
pub fn handle_invite(
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
            &[&nick_str, "INVITE"],
            "Not enough parameters",
        );
        return;
    }

    let target_str = &msg.params[0];
    let chan_str = &msg.params[1];

    // Validate target nick.
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[&nick_str, target_str],
            "No such nick/channel",
        );
        return;
    };

    // Check target exists in registry.
    if registry.get_by_nick(&target_nick).is_none() {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[&nick_str, target_str],
            "No such nick/channel",
        );
        return;
    }

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

    // Check inviter is on the channel, operator status if +i, and target not already on channel.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");

        let Some(inviter_status) = channel.members.get(&nick) else {
            send_numeric(
                sender,
                ERR_NOTONCHANNEL,
                &[&nick_str, chan_str],
                "You're not on that channel",
            );
            return;
        };

        // If channel is +i, inviter must be operator.
        if channel.modes.contains(&ChannelMode::InviteOnly)
            && *inviter_status != MemberStatus::Operator
        {
            send_numeric(
                sender,
                ERR_CHANOPRIVSNEEDED,
                &[&nick_str, chan_str],
                "You're not channel operator",
            );
            return;
        }

        // Target must not already be on channel.
        if channel.members.contains_key(&target_nick) {
            send_numeric(
                sender,
                ERR_USERONCHANNEL,
                &[&nick_str, target_str, chan_str],
                "is already on channel",
            );
            return;
        }
    }

    // Add target to invite list.
    {
        let mut channel = channel_arc.write().expect("channel lock poisoned");
        channel.invite_list.insert(target_nick.clone());
    }

    // Send RPL_INVITING to inviter.
    send_numeric(
        sender,
        RPL_INVITING,
        &[&nick_str, target_str, chan_str],
        "",
    );

    // Send INVITE message to target.
    let invite_msg = Message::builder(Command::Invite)
        .prefix(Prefix::User {
            nick,
            user: username,
            host: hostname,
        })
        .param(target_str)
        .trailing(chan_str)
        .build();

    if let Some(target_session_arc) = registry.get_by_nick(&target_nick) {
        let target_session = target_session_arc.read().expect("session lock poisoned");
        let _ = target_session.sender.send(invite_msg);
    }
}
