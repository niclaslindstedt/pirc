//! P2P signaling message handling for the App.
//!
//! Contains methods on [`App`] that handle incoming PIRC P2P signaling
//! messages (OFFER, ANSWER, ICE, ESTABLISHED, FAILED) and translate
//! outbound [`P2pSessionEvent`]s to protocol messages sent to the server.

use pirc_network::connection::AsyncTransport;
use pirc_protocol::{Message, Prefix, PircSubcommand};
use tracing::warn;

use super::{current_timestamp, App};
use crate::p2p::SignalingMessage;
use crate::tui::message_buffer::{BufferLine, LineType};

impl App {
    /// Handle PIRC P2P subcommand messages.
    ///
    /// Returns `true` if the message was handled, `false` if it should be
    /// passed to another handler.
    pub(super) async fn handle_p2p_message(&mut self, msg: &Message) -> bool {
        let sender = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => return false,
        };

        match &msg.command {
            pirc_protocol::Command::Pirc(PircSubcommand::P2pOffer) => {
                let candidate_lines: Vec<String> =
                    msg.params.iter().skip(1).cloned().collect();
                let outbound = self.p2p.handle_offer(&sender, &candidate_lines).await;
                self.push_p2p_status(&format!("P2P offer received from {sender}"));
                self.send_signaling_messages(outbound).await;
                true
            }
            pirc_protocol::Command::Pirc(PircSubcommand::P2pAnswer) => {
                let candidate_lines: Vec<String> =
                    msg.params.iter().skip(1).cloned().collect();
                let outbound = self.p2p.handle_answer(&sender, &candidate_lines).await;
                self.push_p2p_status(&format!("P2P answer received from {sender}"));
                self.send_signaling_messages(outbound).await;
                true
            }
            pirc_protocol::Command::Pirc(PircSubcommand::P2pIce) => {
                if let Some(candidate) = msg.params.get(1) {
                    self.p2p.handle_ice_candidate(&sender, candidate);
                }
                true
            }
            pirc_protocol::Command::Pirc(PircSubcommand::P2pEstablished) => {
                self.p2p.handle_established(&sender);
                self.push_p2p_status(&format!("P2P connection established with {sender}"));
                true
            }
            pirc_protocol::Command::Pirc(PircSubcommand::P2pFailed) => {
                let reason = msg.params.get(1).cloned().unwrap_or_default();
                self.p2p.handle_failed(&sender, &reason);
                self.push_p2p_status(&format!("P2P connection with {sender} failed: {reason}"));
                true
            }
            _ => false,
        }
    }

    /// Send a list of signaling messages to the server.
    async fn send_signaling_messages(&mut self, messages: Vec<SignalingMessage>) {
        for sig in messages {
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(sig.message).await {
                    warn!("Failed to send P2P signaling message: {e}");
                }
            }
        }
    }

    /// Push a P2P-related status message.
    fn push_p2p_status(&mut self, text: &str) {
        self.view.push_status_message(BufferLine {
            timestamp: current_timestamp(&self.config.ui.timestamp_format),
            sender: None,
            content: text.to_string(),
            line_type: LineType::System,
        });
    }
}
