use std::sync::Arc;

use pirc_common::Nickname;
use pirc_protocol::numeric::{ERR_NEEDMOREPARAMS, ERR_NOSUCHNICK};
use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
use tokio::sync::mpsc;

use crate::handler::send_numeric;
use crate::registry::UserRegistry;

/// Relay a PIRC subcommand message from sender to target user.
///
/// Used for E2E encryption protocol messages that the server must forward
/// without inspecting content: `ENCRYPTED`, `KEYEXCHANGE-ACK`,
/// `KEYEXCHANGE-COMPLETE`, and `FINGERPRINT`.
///
/// Wire format: `PIRC <SUBCOMMAND> <target> [params...]`
///
/// The server rewrites the prefix to the sender's `nick!user@host` and
/// forwards the message (including all params after the target) to the
/// target user. If the target is not found, sends `ERR_NOSUCHNICK`.
pub fn handle_relay(
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

    let Some(target_session_arc) = registry.get_by_nick(&target_nick) else {
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
            nick: sender_nick,
            user: username,
            host: hostname,
        });
    for param in &msg.params {
        builder = builder.param(param);
    }
    let relay_msg = builder.build();

    let target_session = target_session_arc.read().expect("session lock poisoned");
    let _ = target_session.sender.send(relay_msg);
}
