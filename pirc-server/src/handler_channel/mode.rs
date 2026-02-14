use std::sync::Arc;

use pirc_common::{ChannelMode, Nickname};
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, ERR_UNKNOWNMODE,
    ERR_USERNOTINCHANNEL, RPL_BANLIST, RPL_CHANNELMODEIS, RPL_ENDOFBANLIST,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::broadcast_to_channel;

/// Handle the MODE command targeting a channel.
///
/// - `MODE #channel` → RPL_CHANNELMODEIS with current modes
/// - `MODE #channel +b` → RPL_BANLIST entries + RPL_ENDOFBANLIST
/// - `MODE #channel <modestring> [params...]` → apply mode changes (operator only)
pub fn handle_channel_mode(
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

    // Mode query (no modestring provided).
    if msg.params.len() < 2 {
        let channel = channel_arc.read().expect("channel lock poisoned");
        let mode_string = format_channel_modes(&channel);
        send_numeric(
            sender,
            RPL_CHANNELMODEIS,
            &[&nick_str, chan_name.as_ref(), &mode_string],
            "",
        );
        return;
    }

    let modestring = &msg.params[1];

    // Ban list query: MODE #channel +b (or just "b") with no extra params
    if (modestring == "+b" || modestring == "b") && msg.params.len() < 3 {
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

    // Mode set: check membership and operator status.
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

    // Parse and apply mode changes.
    let mode_params: Vec<&str> = msg.params[2..].iter().map(String::as_str).collect();
    let mut param_idx = 0;
    let mut adding = true;
    let mut applied_modes = String::new();
    let mut applied_params: Vec<String> = Vec::new();
    let mut last_dir: Option<bool> = None;

    for ch in modestring.chars() {
        match ch {
            '+' => adding = true,
            '-' => adding = false,
            'i' | 'm' | 'n' | 's' | 't' => {
                let mode = match ch {
                    'i' => ChannelMode::InviteOnly,
                    'm' => ChannelMode::Moderated,
                    'n' => ChannelMode::NoExternalMessages,
                    's' => ChannelMode::Secret,
                    't' => ChannelMode::TopicProtected,
                    _ => unreachable!(),
                };
                let mut channel = channel_arc.write().expect("channel lock poisoned");
                if adding {
                    channel.modes.insert(mode);
                } else {
                    channel.modes.remove(&mode);
                }
                if last_dir != Some(adding) {
                    applied_modes.push(if adding { '+' } else { '-' });
                    last_dir = Some(adding);
                }
                applied_modes.push(ch);
            }
            'k' => {
                let mut channel = channel_arc.write().expect("channel lock poisoned");
                if adding {
                    if let Some(&key) = mode_params.get(param_idx) {
                        channel.key = Some(key.to_owned());
                        if last_dir != Some(adding) {
                            applied_modes.push('+');
                            last_dir = Some(adding);
                        }
                        applied_modes.push('k');
                        applied_params.push(key.to_owned());
                        param_idx += 1;
                    }
                } else {
                    channel.key = None;
                    if last_dir != Some(adding) {
                        applied_modes.push('-');
                        last_dir = Some(adding);
                    }
                    applied_modes.push('k');
                    // Consume parameter if provided (some clients send the key on -k too).
                    if mode_params.get(param_idx).is_some() {
                        param_idx += 1;
                    }
                }
            }
            'l' => {
                let mut channel = channel_arc.write().expect("channel lock poisoned");
                if adding {
                    if let Some(&limit_str) = mode_params.get(param_idx) {
                        if let Ok(limit) = limit_str.parse::<u32>() {
                            channel.user_limit = Some(limit);
                            if last_dir != Some(adding) {
                                applied_modes.push('+');
                                last_dir = Some(adding);
                            }
                            applied_modes.push('l');
                            applied_params.push(limit_str.to_owned());
                            param_idx += 1;
                        }
                    }
                } else {
                    channel.user_limit = None;
                    if last_dir != Some(adding) {
                        applied_modes.push('-');
                        last_dir = Some(adding);
                    }
                    applied_modes.push('l');
                }
            }
            'b' => {
                let mut channel = channel_arc.write().expect("channel lock poisoned");
                if adding {
                    if let Some(&mask) = mode_params.get(param_idx) {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        channel.ban_list.push(crate::channel::BanEntry {
                            mask: mask.to_owned(),
                            who_set: nick_str.clone(),
                            timestamp: now,
                        });
                        if last_dir != Some(adding) {
                            applied_modes.push('+');
                            last_dir = Some(adding);
                        }
                        applied_modes.push('b');
                        applied_params.push(mask.to_owned());
                        param_idx += 1;
                    }
                } else {
                    if let Some(&mask) = mode_params.get(param_idx) {
                        channel
                            .ban_list
                            .retain(|b| !b.mask.eq_ignore_ascii_case(mask));
                        if last_dir != Some(adding) {
                            applied_modes.push('-');
                            last_dir = Some(adding);
                        }
                        applied_modes.push('b');
                        applied_params.push(mask.to_owned());
                        param_idx += 1;
                    }
                }
            }
            'o' | 'v' => {
                if let Some(&target_str) = mode_params.get(param_idx) {
                    param_idx += 1;
                    let Ok(target_nick) = Nickname::new(target_str) else {
                        continue;
                    };
                    let mut channel = channel_arc.write().expect("channel lock poisoned");
                    if !channel.members.contains_key(&target_nick) {
                        send_numeric(
                            sender,
                            ERR_USERNOTINCHANNEL,
                            &[&nick_str, target_str, chan_str],
                            "They aren't on that channel",
                        );
                        continue;
                    }
                    let new_status = if adding {
                        if ch == 'o' {
                            MemberStatus::Operator
                        } else {
                            MemberStatus::Voiced
                        }
                    } else {
                        // When removing +o or +v, drop to Normal.
                        MemberStatus::Normal
                    };
                    channel.members.insert(target_nick, new_status);
                    if last_dir != Some(adding) {
                        applied_modes.push(if adding { '+' } else { '-' });
                        last_dir = Some(adding);
                    }
                    applied_modes.push(ch);
                    applied_params.push(target_str.to_owned());
                }
            }
            _ => {
                send_numeric(
                    sender,
                    ERR_UNKNOWNMODE,
                    &[&nick_str, &ch.to_string()],
                    "is unknown mode char to me",
                );
            }
        }
    }

    // Broadcast applied mode changes to channel members.
    if !applied_modes.is_empty() {
        let mut builder = Message::builder(Command::Mode)
            .prefix(Prefix::User {
                nick,
                user: username,
                host: hostname,
            })
            .param(chan_name.as_ref())
            .param(&applied_modes);
        for p in &applied_params {
            builder = builder.param(p);
        }
        let mode_msg = builder.build();
        broadcast_to_channel(&channel_arc, &mode_msg, None, registry);
    }
}

/// Format channel modes as a string like `+imt` or `+kl secret 50`.
pub(super) fn format_channel_modes(channel: &crate::channel::Channel) -> String {
    let mut mode_chars: Vec<char> = channel.modes.iter().map(|m| m.mode_char()).collect();
    mode_chars.sort();

    let mut params = Vec::new();

    if let Some(ref key) = channel.key {
        mode_chars.push('k');
        params.push(key.clone());
    }
    if let Some(limit) = channel.user_limit {
        mode_chars.push('l');
        params.push(limit.to_string());
    }

    let mut result = format!("+{}", mode_chars.iter().collect::<String>());
    for p in params {
        result.push(' ');
        result.push_str(&p);
    }
    result
}
