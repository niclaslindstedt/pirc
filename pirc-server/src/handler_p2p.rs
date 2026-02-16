use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::numeric::{ERR_NEEDMOREPARAMS, ERR_NOSUCHNICK};
use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
use tokio::sync::mpsc;

use crate::handler::send_numeric;
use crate::registry::UserRegistry;

/// Relay a P2P signaling message from sender to target user.
///
/// Used for P2P connection signaling messages: `P2P OFFER`, `P2P ANSWER`,
/// `P2P ICE`, `P2P ESTABLISHED`, and `P2P FAILED`.
///
/// Wire format: `PIRC P2P <SUBCOMMAND> <target> [params...]`
///
/// The server rewrites the prefix to the sender's `nick!user@host` and
/// forwards the message (including all params) to the target user.
///
/// Unlike encryption relay messages, P2P signaling is time-sensitive and
/// is **not** queued for offline delivery. If the target is not online,
/// the sender receives `ERR_NOSUCHNICK`.
pub fn handle_p2p_relay(
    subcommand: &PircSubcommand,
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (sender_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    let cmd_keyword = subcommand.as_str();

    // At minimum we need a target parameter.
    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), cmd_keyword],
            "Not enough parameters",
        );
        return;
    }

    let target_str = &msg.params[0];
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_str],
            "No such nick/channel",
        );
        return;
    };

    // Build the relayed message with the sender's prefix and all original params.
    let mut builder = Message::builder(Command::Pirc(subcommand.clone()))
        .prefix(Prefix::User {
            nick: sender_nick.clone(),
            user: username,
            host: hostname,
        });
    for param in &msg.params {
        builder = builder.param(param);
    }
    let relay_msg = builder.build();

    if let Some(target_session_arc) = registry.get_by_nick(&target_nick) {
        let target_session = target_session_arc.read().expect("session lock poisoned");
        let _ = target_session.sender.send(relay_msg);
    } else {
        // P2P signaling is time-sensitive — no offline queuing.
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_nick.as_ref()],
            "No such nick/channel",
        );
    }
}
