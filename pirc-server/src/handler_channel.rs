use std::sync::Arc;

use pirc_common::{ChannelMode, ChannelName, Nickname};
use pirc_protocol::numeric::{
    ERR_BADCHANNELKEY, ERR_BANNEDCHANNEL, ERR_CHANOPRIVSNEEDED, ERR_CHANNELISFULL,
    ERR_INVITEONLYCHAN, ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL,
    ERR_USERNOTINCHANNEL, RPL_ENDOFNAMES, RPL_NAMREPLY, RPL_NOTOPIC, RPL_TOPIC, RPL_TOPICWHOTIME,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

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

    for (i, chan_str) in channel_names.iter().enumerate() {
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
                if channel.modes.contains(&ChannelMode::InviteOnly)
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
                    send_numeric(
                        sender,
                        RPL_TOPIC,
                        &[&nick_str, chan_name.as_ref()],
                        text,
                    );
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

/// Handle the TOPIC command from a registered user.
///
/// `TOPIC #channel` queries the current topic.
/// `TOPIC #channel :new topic` sets the topic.
/// `TOPIC #channel :` clears the topic.
///
/// When +t (TopicProtected) is set, only channel operators may change the topic.
pub fn handle_topic(
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
            &[&nick_str, "TOPIC"],
            "Not enough parameters",
        );
        return;
    }

    let chan_str = &msg.params[0];

    // Validate channel name.
    let Ok(chan_name) = ChannelName::new(chan_str) else {
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

    // Check membership.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");
        if !channel.members.contains_key(&nick) {
            send_numeric(
                sender,
                ERR_NOTONCHANNEL,
                &[&nick_str, chan_str],
                "You're not on that channel",
            );
            return;
        }
    }

    if msg.params.len() < 2 {
        // Topic query: return current topic or RPL_NOTOPIC.
        let channel = channel_arc.read().expect("channel lock poisoned");
        match &channel.topic {
            Some((text, who, timestamp)) => {
                send_numeric(
                    sender,
                    RPL_TOPIC,
                    &[&nick_str, chan_name.as_ref()],
                    text,
                );
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
        return;
    }

    // Topic set: check +t mode and operator status.
    {
        let channel = channel_arc.read().expect("channel lock poisoned");
        if channel.modes.contains(&ChannelMode::TopicProtected) {
            let status = channel.members.get(&nick);
            if status != Some(&MemberStatus::Operator) {
                send_numeric(
                    sender,
                    ERR_CHANOPRIVSNEEDED,
                    &[&nick_str, chan_str],
                    "You're not channel operator",
                );
                return;
            }
        }
    }

    // Set or clear the topic.
    let new_topic = &msg.params[1];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    {
        let mut channel = channel_arc.write().expect("channel lock poisoned");
        if new_topic.is_empty() {
            channel.topic = None;
        } else {
            channel.topic = Some((new_topic.clone(), nick_str.clone(), now));
        }
    }

    // Broadcast TOPIC message to all channel members.
    let topic_msg = Message::builder(Command::Topic)
        .prefix(Prefix::User {
            nick: nick.clone(),
            user: username,
            host: hostname,
        })
        .param(chan_name.as_ref())
        .trailing(new_topic)
        .build();
    broadcast_to_channel(&channel_arc, &topic_msg, None, registry);
}

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
    let Ok(chan_name) = ChannelName::new(chan_str) else {
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

/// Broadcast a message to all members of a channel.
///
/// If `exclude` is `Some(nick)`, that nick will not receive the message.
pub fn broadcast_to_channel(
    channel_arc: &Arc<std::sync::RwLock<crate::channel::Channel>>,
    msg: &Message,
    exclude: Option<&Nickname>,
    registry: &Arc<UserRegistry>,
) {
    let member_nicks: Vec<Nickname> = {
        let channel = channel_arc.read().expect("channel lock poisoned");
        channel.members.keys().cloned().collect()
    };

    for member_nick in &member_nicks {
        if exclude.is_some_and(|e| e == member_nick) {
            continue;
        }
        if let Some(session_arc) = registry.get_by_nick(member_nick) {
            let session = session_arc.read().expect("session lock poisoned");
            let _ = session.sender.send(msg.clone());
        }
    }
}

/// Send RPL_NAMREPLY + RPL_ENDOFNAMES to a user for a channel.
fn send_names_reply(
    sender: &mpsc::UnboundedSender<Message>,
    nick: &str,
    chan_name: &ChannelName,
    channel_arc: &Arc<std::sync::RwLock<crate::channel::Channel>>,
) {
    let names_str = {
        let channel = channel_arc.read().expect("channel lock poisoned");
        let mut names: Vec<String> = channel
            .members
            .iter()
            .map(|(member_nick, status)| {
                match status.prefix_char() {
                    Some(prefix) => format!("{}{}", prefix, member_nick.as_ref()),
                    None => member_nick.to_string(),
                }
            })
            .collect();
        names.sort();
        names.join(" ")
    };

    // RPL_NAMREPLY: = means public channel
    send_numeric(
        sender,
        RPL_NAMREPLY,
        &[nick, "=", chan_name.as_ref()],
        &names_str,
    );

    send_numeric(
        sender,
        RPL_ENDOFNAMES,
        &[nick, chan_name.as_ref()],
        "End of /NAMES list",
    );
}

/// Check if a user mask matches any ban entry.
fn is_banned(ban_list: &[crate::channel::BanEntry], user_mask: &str) -> bool {
    ban_list.iter().any(|ban| matches_ban_mask(&ban.mask, user_mask))
}

/// Simple glob-style ban mask matching.
///
/// Supports `*` as a wildcard matching any sequence of characters.
pub fn matches_ban_mask(mask: &str, target: &str) -> bool {
    let mask_lower = mask.to_ascii_lowercase();
    let target_lower = target.to_ascii_lowercase();
    glob_match(&mask_lower, &target_lower)
}

/// Simple glob matching: `*` matches any sequence, `?` matches any single char.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let mut px = 0;
    let mut tx = 0;
    let mut star_px = usize::MAX;
    let mut star_tx = 0;
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();

    while tx < tb.len() {
        if px < pb.len() && (pb[px] == b'?' || pb[px] == tb[tx]) {
            px += 1;
            tx += 1;
        } else if px < pb.len() && pb[px] == b'*' {
            star_px = px;
            star_tx = tx;
            px += 1;
        } else if star_px != usize::MAX {
            px = star_px + 1;
            star_tx += 1;
            tx = star_tx;
        } else {
            return false;
        }
    }
    while px < pb.len() && pb[px] == b'*' {
        px += 1;
    }
    px == pb.len()
}

/// Remove a user from all channels they are in, cleaning up empty channels.
pub fn remove_user_from_all_channels(nick: &Nickname, channels: &Arc<ChannelRegistry>) {
    let channel_list = channels.list();
    for (chan_name, _, _) in channel_list {
        if let Some(channel_arc) = channels.get(&chan_name) {
            let mut channel = channel_arc.write().expect("channel lock poisoned");
            channel.members.remove(nick);
        }
        channels.remove_if_empty(&chan_name);
    }
}
