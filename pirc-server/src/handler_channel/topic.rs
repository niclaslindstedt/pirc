use std::sync::Arc;

use pirc_common::ChannelMode;
use pirc_protocol::numeric::{
    ERR_CHANOPRIVSNEEDED, ERR_NEEDMOREPARAMS, ERR_NOSUCHCHANNEL, ERR_NOTONCHANNEL, RPL_NOTOPIC,
    RPL_TOPIC, RPL_TOPICWHOTIME,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::handler::send_numeric;
use crate::registry::UserRegistry;

use super::util::broadcast_to_channel;

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
