use std::sync::Arc;

use pirc_common::ChannelName;
use pirc_protocol::numeric::{
    ERR_BADCHANNELKEY, ERR_BANNEDCHANNEL, ERR_CHANNELISFULL, ERR_INVITEONLYCHAN,
    ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, ERR_TOOMANYCHANNELS, RPL_NOTOPIC,
    RPL_TOPIC, RPL_TOPICWHOTIME,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::{broadcast_to_channel, is_banned, send_names_reply};

/// Handle the JOIN command from a registered user.
///
/// Supports comma-separated channel names: `JOIN #chan1,#chan2 [key1,key2]`
/// Creates channels on first join, grants +o to first user, enforces mode
/// restrictions (+i, +k, +l, +b), broadcasts JOIN to channel members,
/// and sends topic + NAMES to the joining user.
pub fn handle_join(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    max_channels_per_user: u32,
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
            &[&nick_str, "JOIN"],
            "Not enough parameters",
        );
        return;
    }

    let channel_names: Vec<&str> = msg.params[0].split(',').collect();
    let keys: Vec<&str> = if msg.params.len() > 1 {
        msg.params[1].split(',').collect()
    } else {
        Vec::new()
    };

    // Track how many channels this user is already in for limit enforcement.
    let mut user_channel_count = channels.channels_for_nick(&nick) as u32;

    for (i, chan_str) in channel_names.iter().enumerate() {
        let chan_str = chan_str.trim();
        if chan_str.is_empty() {
            continue;
        }

        // Enforce per-user channel limit.
        if user_channel_count >= max_channels_per_user {
            send_numeric(
                sender,
                ERR_TOOMANYCHANNELS,
                &[&nick_str, chan_str],
                "You have joined too many channels",
            );
            continue;
        }

        // Validate channel name.
        let Ok(chan_name) = ChannelName::new(chan_str) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        let key = keys.get(i).copied();

        // Get or create the channel.
        let channel_arc = channels.get_or_create(chan_name.clone());
        let is_new;
        {
            let mut channel = channel_arc.write().expect("channel lock poisoned");

            // Check if already a member.
            if channel.members.contains_key(&nick) {
                // Silently ignore duplicate JOIN per IRC convention.
                continue;
            }

            is_new = channel.members.is_empty();

            // Enforce channel mode restrictions (skip for new channels).
            if !is_new {
                // +b: check ban list
                let user_mask = format!("{}!{}@{}", nick_str, username, hostname);
                if is_banned(&channel.ban_list, &user_mask) {
                    send_numeric(
                        sender,
                        ERR_BANNEDCHANNEL,
                        &[&nick_str, chan_str],
                        "Cannot join channel (+b)",
                    );
                    continue;
                }

                // +i: invite only
                if channel
                    .modes
                    .contains(&pirc_common::ChannelMode::InviteOnly)
                    && !channel.invite_list.contains(&nick)
                {
                    send_numeric(
                        sender,
                        ERR_INVITEONLYCHAN,
                        &[&nick_str, chan_str],
                        "Cannot join channel (+i)",
                    );
                    continue;
                }

                // +k: key required
                if let Some(ref chan_key) = channel.key {
                    match key {
                        Some(provided) if provided == chan_key => {}
                        _ => {
                            send_numeric(
                                sender,
                                ERR_BADCHANNELKEY,
                                &[&nick_str, chan_str],
                                "Cannot join channel (+k)",
                            );
                            continue;
                        }
                    }
                }

                // +l: user limit
                if let Some(limit) = channel.user_limit {
                    if channel.members.len() as u32 >= limit {
                        send_numeric(
                            sender,
                            ERR_CHANNELISFULL,
                            &[&nick_str, chan_str],
                            "Cannot join channel (+l)",
                        );
                        continue;
                    }
                }
            }

            // Add user to channel. First user gets +o.
            let status = if is_new {
                MemberStatus::Operator
            } else {
                MemberStatus::Normal
            };
            channel.members.insert(nick.clone(), status);

            // Remove from invite list if present (invite consumed).
            channel.invite_list.remove(&nick);
        }

        user_channel_count += 1;

        // Build the JOIN message with user prefix.
        let join_msg = Message::builder(Command::Join)
            .prefix(Prefix::User {
                nick: nick.clone(),
                user: username.clone(),
                host: hostname.clone(),
            })
            .param(chan_name.as_ref())
            .build();

        // Broadcast JOIN to all channel members (including the joining user).
        broadcast_to_channel(&channel_arc, &join_msg, None, registry);

        // Send topic to joining user.
        {
            let channel = channel_arc.read().expect("channel lock poisoned");
            match &channel.topic {
                Some((text, who, timestamp)) => {
                    send_numeric(sender, RPL_TOPIC, &[&nick_str, chan_name.as_ref()], text);
                    send_numeric(
                        sender,
                        RPL_TOPICWHOTIME,
                        &[&nick_str, chan_name.as_ref(), who, &timestamp.to_string()],
                        "",
                    );
                }
                None => {
                    send_numeric(
                        sender,
                        RPL_NOTOPIC,
                        &[&nick_str, chan_name.as_ref()],
                        "No topic is set",
                    );
                }
            }
        }

        // Send NAMES list.
        send_names_reply(sender, &nick_str, &chan_name, &channel_arc);
    }
}

/// Handle the PART command from a registered user.
///
/// Supports comma-separated channel names: `PART #chan1,#chan2 [:reason]`
/// Broadcasts PART to channel members, removes user, and cleans up empty channels.
pub fn handle_part(
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
            &[&nick_str, "PART"],
            "Not enough parameters",
        );
        return;
    }

    let channel_names: Vec<&str> = msg.params[0].split(',').collect();
    let reason = msg.params.get(1).map(String::as_str);

    for chan_str in channel_names {
        let chan_str = chan_str.trim();
        if chan_str.is_empty() {
            continue;
        }

        // Validate channel name.
        let Ok(chan_name) = ChannelName::new(chan_str) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        let Some(channel_arc) = channels.get(&chan_name) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        // Check membership and remove.
        {
            let channel = channel_arc.read().expect("channel lock poisoned");
            if !channel.members.contains_key(&nick) {
                send_numeric(
                    sender,
                    ERR_NOTONCHANNEL,
                    &[&nick_str, chan_str],
                    "You're not on that channel",
                );
                continue;
            }
        }

        // Build the PART message with user prefix.
        let mut part_builder = Message::builder(Command::Part)
            .prefix(Prefix::User {
                nick: nick.clone(),
                user: username.clone(),
                host: hostname.clone(),
            })
            .param(chan_name.as_ref());
        if let Some(reason) = reason {
            part_builder = part_builder.trailing(reason);
        }
        let part_msg = part_builder.build();

        // Broadcast PART to all channel members (including the parting user).
        broadcast_to_channel(&channel_arc, &part_msg, None, registry);

        // Remove user from channel.
        {
            let mut channel = channel_arc.write().expect("channel lock poisoned");
            channel.members.remove(&nick);
        }

        // Clean up empty channel.
        channels.remove_if_empty(&chan_name);
    }
}
