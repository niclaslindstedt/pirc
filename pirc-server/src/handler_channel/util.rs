use std::sync::Arc;

use pirc_common::{ChannelName, Nickname};
use pirc_protocol::numeric::{RPL_ENDOFNAMES, RPL_NAMREPLY};
use pirc_protocol::Message;
use tokio::sync::mpsc;

use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

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
pub fn send_names_reply(
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
pub fn is_banned(ban_list: &[crate::channel::BanEntry], user_mask: &str) -> bool {
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
