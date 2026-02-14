use std::sync::Arc;

use pirc_common::{ChannelMode, ChannelName};
use pirc_protocol::numeric::{RPL_LIST, RPL_LISTEND};
use pirc_protocol::Message;
use tokio::sync::mpsc;

use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::handler_channel::util::send_names_reply;
use crate::registry::UserRegistry;

/// Handle the LIST command.
///
/// `LIST` returns all visible channels (non-secret, or secret channels the
/// user is a member of). For each channel it sends `RPL_LIST` (322) with the
/// channel name, member count and topic, followed by `RPL_LISTEND` (323).
pub fn handle_list(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.clone()
    };
    let nick_str = nick.to_string();

    // Optionally filter by specific channel names.
    let filter_channels: Option<Vec<&str>> = msg.params.first().map(|p| p.split(',').collect());

    // Iterate all channels and send RPL_LIST for visible ones.
    let channel_list = channels.list_all();

    for (chan_name, channel_arc) in &channel_list {
        // If a filter was specified, check if this channel is in the filter list.
        if let Some(ref filters) = filter_channels {
            if !filters
                .iter()
                .any(|f| f.eq_ignore_ascii_case(chan_name.as_ref()))
            {
                continue;
            }
        }

        let channel = channel_arc.read().expect("channel lock poisoned");

        // Skip secret channels unless the user is a member.
        if channel.modes.contains(&ChannelMode::Secret) && !channel.members.contains_key(&nick) {
            continue;
        }

        let member_count = channel.member_count().to_string();
        let topic = channel
            .topic
            .as_ref()
            .map_or("", |(text, _, _)| text.as_str());

        send_numeric(
            sender,
            RPL_LIST,
            &[&nick_str, chan_name.as_ref(), &member_count],
            topic,
        );
    }

    send_numeric(sender, RPL_LISTEND, &[&nick_str], "End of /LIST");
}

/// Handle the NAMES command.
///
/// `NAMES #channel` sends `RPL_NAMREPLY` + `RPL_ENDOFNAMES` for the specified
/// channel. `NAMES` with no args sends names for all channels the user is on.
pub fn handle_names(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.clone()
    };
    let nick_str = nick.to_string();

    if msg.params.is_empty() {
        // NAMES with no args — send names for all channels the user is in.
        let channel_list = channels.list_all();
        for (chan_name, channel_arc) in &channel_list {
            let is_member = {
                let channel = channel_arc.read().expect("channel lock poisoned");
                channel.members.contains_key(&nick)
            };
            if is_member {
                send_names_reply(sender, &nick_str, chan_name, channel_arc);
            }
        }
    } else {
        // NAMES with specific channels (comma-separated).
        let channel_names: Vec<&str> = msg.params[0].split(',').collect();
        for chan_str in channel_names {
            let Ok(chan_name) = ChannelName::new(chan_str) else {
                continue;
            };

            if let Some(channel_arc) = channels.get(&chan_name) {
                send_names_reply(sender, &nick_str, &chan_name, &channel_arc);
            } else {
                // Channel doesn't exist — send empty RPL_ENDOFNAMES.
                send_numeric(
                    sender,
                    pirc_protocol::numeric::RPL_ENDOFNAMES,
                    &[&nick_str, chan_str],
                    "End of /NAMES list",
                );
            }
        }
    }
}
