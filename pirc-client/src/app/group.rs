//! Group chat command handling for the App.
//!
//! Contains methods on [`App`] that handle `/group` subcommands and
//! incoming `PIRC GROUP` server messages (CREATE confirmation, INVITE
//! notification, JOIN/LEAVE broadcasts, MEMBERS response).

use pirc_common::types::GroupId;
use pirc_network::connection::AsyncTransport;
use pirc_protocol::{Command, Message, Prefix, PircSubcommand};
use tracing::warn;

use super::{current_timestamp, App};
use crate::client_command::GroupSubcommand;
use crate::tui::buffer_manager::BufferId;
use crate::tui::message_buffer::{BufferLine, LineType};

impl App {
    /// Handle `/group <subcommand>`.
    pub(super) async fn handle_group_command(&mut self, sub: &GroupSubcommand) {
        match sub {
            GroupSubcommand::Create(name) => self.handle_group_create(name).await,
            GroupSubcommand::Invite(nick) => self.handle_group_invite(nick).await,
            GroupSubcommand::Join(group_id) => self.handle_group_join(*group_id).await,
            GroupSubcommand::Leave => self.handle_group_leave().await,
            GroupSubcommand::Members => self.handle_group_members(),
            GroupSubcommand::List => self.handle_group_list(),
            GroupSubcommand::Info => self.handle_group_info(),
        }
    }

    /// `/group create <name>` — send CREATE to server.
    async fn handle_group_create(&mut self, name: &str) {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::GroupCreate),
            vec![name.to_string()],
        );
        self.send_or_status(msg).await;
    }

    /// `/group invite <nick>` — invite a user to the current group.
    async fn handle_group_invite(&mut self, nick: &str) {
        let Some(group_id) = self.current_group_context() else {
            self.push_status("Not in a group buffer. Switch to a group buffer first.");
            return;
        };
        let msg = Message::new(
            Command::Pirc(PircSubcommand::GroupInvite),
            vec![group_id.as_u64().to_string(), nick.to_string()],
        );
        self.send_or_status(msg).await;
    }

    /// `/group join <group_id>` — join a group.
    async fn handle_group_join(&mut self, group_id: GroupId) {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::GroupJoin),
            vec![group_id.as_u64().to_string()],
        );
        self.send_or_status(msg).await;
    }

    /// `/group leave` — leave the current group.
    async fn handle_group_leave(&mut self) {
        let Some(group_id) = self.current_group_context() else {
            self.push_status("Not in a group buffer. Switch to a group buffer first.");
            return;
        };
        let msg = Message::new(
            Command::Pirc(PircSubcommand::GroupLeave),
            vec![group_id.as_u64().to_string()],
        );
        self.send_or_status(msg).await;
    }

    /// `/group members` — display member list of the current group.
    fn handle_group_members(&mut self) {
        let Some(group_id) = self.current_group_context() else {
            self.push_status("Not in a group buffer. Switch to a group buffer first.");
            return;
        };
        // Display locally-known members from the group chat manager
        let members = self.group_chat.group_members(group_id);
        if members.is_empty() {
            self.push_status(&format!(
                "Group {}: no members tracked locally",
                group_id.as_u64()
            ));
        } else {
            self.push_status(&format!(
                "Group {} members: {}",
                group_id.as_u64(),
                members.join(", ")
            ));
        }
    }

    /// `/group list` — list all groups the user belongs to.
    fn handle_group_list(&mut self) {
        let ids = self.group_chat.group_ids();
        if ids.is_empty() {
            self.push_status("You are not in any groups.");
        } else {
            self.push_status("Groups:");
            for id in &ids {
                let member_count = self.group_chat.group_members(*id).len();
                self.push_status(&format!(
                    "  Group {} ({} members)",
                    id.as_u64(),
                    member_count
                ));
            }
        }
    }

    /// `/group info` — show info about the current group.
    fn handle_group_info(&mut self) {
        let Some(group_id) = self.current_group_context() else {
            self.push_status("Not in a group buffer. Switch to a group buffer first.");
            return;
        };
        let members = self.group_chat.group_members(group_id);
        let connected = self.group_chat.connected_members(group_id);
        let degraded = self.group_chat.degraded_members(group_id);
        let all_encrypted = self.group_chat.all_encryption_ready(group_id);

        self.push_status(&format!("Group {}", group_id.as_u64()));
        self.push_status(&format!("  Members: {}", members.len()));
        self.push_status(&format!("  P2P connected: {}", connected.len()));
        self.push_status(&format!("  Relay fallback: {}", degraded.len()));
        self.push_status(&format!(
            "  Encryption: {}",
            if all_encrypted {
                "all ready"
            } else {
                "not all members ready"
            }
        ));
    }

    // ── Server GROUP message handling ─────────────────────────────

    /// Handle inbound PIRC GROUP subcommand messages from the server.
    ///
    /// Returns `true` if the message was handled, `false` otherwise.
    pub(super) fn handle_group_message(&mut self, msg: &Message) -> bool {
        match &msg.command {
            Command::Pirc(PircSubcommand::GroupCreate) => {
                self.on_group_create_response(msg);
                true
            }
            Command::Pirc(PircSubcommand::GroupInvite) => {
                self.on_group_invite(msg);
                true
            }
            Command::Pirc(PircSubcommand::GroupJoin) => {
                self.on_group_join_broadcast(msg);
                true
            }
            Command::Pirc(PircSubcommand::GroupLeave) => {
                self.on_group_leave_broadcast(msg);
                true
            }
            Command::Pirc(PircSubcommand::GroupMembers) => {
                self.on_group_members(msg);
                true
            }
            Command::Pirc(PircSubcommand::GroupMessage) => {
                self.on_group_msg(msg);
                true
            }
            _ => false,
        }
    }

    /// GROUP CREATE confirmation — server assigns `group_id`.
    fn on_group_create_response(&mut self, msg: &Message) {
        // params[0] = group_id, params[1] = group_name
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };
        let group_name = msg.params.get(1).cloned().unwrap_or_default();

        self.group_chat.add_group(group_id);

        let buf_id = group_buffer_id(group_id);
        self.view.buffers_mut().ensure_open(buf_id.clone());
        self.push_group_system(
            group_id,
            &format!("Group '{}' created (ID: {})", group_name, group_id.as_u64()),
        );
        self.push_status(&format!(
            "Group '{}' created (ID: {})",
            group_name,
            group_id.as_u64()
        ));
    }

    /// GROUP INVITE notification — someone invited us.
    fn on_group_invite(&mut self, msg: &Message) {
        let inviter = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => "unknown".to_string(),
        };
        // params[0] = group_id, params[1] = target_nick, params[2] = group_name
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };
        let group_name = msg.params.get(2).cloned().unwrap_or_default();

        self.push_status(&format!(
            "{inviter} invited you to group '{}' (ID: {}). Use /group join {} to accept.",
            group_name,
            group_id.as_u64(),
            group_id.as_u64()
        ));
    }

    /// GROUP JOIN broadcast — a member joined a group.
    fn on_group_join_broadcast(&mut self, msg: &Message) {
        let joiner = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => return,
        };
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };

        let our_nick = self.connection_mgr.nick().to_string();
        if joiner == our_nick {
            // We joined — register the group and open buffer
            self.group_chat.add_group(group_id);
            let buf_id = group_buffer_id(group_id);
            self.view.buffers_mut().ensure_open(buf_id);
            self.push_group_system(group_id, "You have joined the group");
        } else {
            // Someone else joined
            self.group_chat.handle_member_join(group_id, &joiner);
            self.push_group_system(group_id, &format!("{joiner} has joined the group"));
        }
    }

    /// GROUP LEAVE broadcast — a member left a group.
    fn on_group_leave_broadcast(&mut self, msg: &Message) {
        let leaver = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => return,
        };
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };

        let our_nick = self.connection_mgr.nick().to_string();
        if leaver == our_nick {
            // We left the group
            self.push_group_system(group_id, "You have left the group");
            self.group_chat.remove_group(group_id);
        } else {
            self.group_chat.handle_member_leave(group_id, &leaver);
            self.push_group_system(group_id, &format!("{leaver} has left the group"));
        }
    }

    /// GROUP MEMBERS response — server sends member list.
    fn on_group_members(&mut self, msg: &Message) {
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };

        let members: Vec<&str> = msg.params.iter().skip(1).map(String::as_str).collect();
        let our_nick = self.connection_mgr.nick().to_string();

        // Register each member (except ourselves)
        for member in &members {
            if *member != our_nick {
                self.group_chat.handle_member_join(group_id, member);
            }
        }

        self.push_group_system(
            group_id,
            &format!("Members: {}", members.join(", ")),
        );
    }

    /// GROUP MSG — encrypted group message received via relay.
    fn on_group_msg(&mut self, msg: &Message) {
        let sender = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => return,
        };
        // params[0] = group_id, params[1] = target (us), params[2] = encrypted_payload
        let Some(id_str) = msg.params.first() else { return };
        let Ok(group_id) = id_str.parse::<GroupId>() else { return };
        let Some(payload_str) = msg.params.get(2) else { return };

        // Decode and decrypt
        let bytes = match pirc_crypto::protocol::decode_from_wire(payload_str) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to decode group message from {sender}: {e}");
                return;
            }
        };

        match self.group_chat.receive_message(group_id, &sender, &bytes) {
            Ok(received) => {
                let content = String::from_utf8_lossy(&received.plaintext).to_string();
                let ts = current_timestamp(&self.config.ui.timestamp_format);
                let buf_id = group_buffer_id(group_id);
                self.view.push_message(
                    &buf_id,
                    BufferLine {
                        timestamp: ts,
                        sender: Some(sender),
                        content,
                        line_type: LineType::Message,
                    },
                );
            }
            Err(e) => {
                warn!("Failed to decrypt group message from {sender}: {e}");
                self.push_group_system(
                    group_id,
                    &format!("Failed to decrypt message from {sender}"),
                );
            }
        }
    }

    // ── Group chat message sending ─────────────────────────────────

    /// Send a plain-text message to a group via encrypted fan-out.
    ///
    /// Encrypts through [`GroupChatManager::send_message`] and sends any
    /// relay messages to the server as `PIRC GROUP MSG`. Echoes the
    /// plaintext locally in the group buffer.
    pub(super) async fn handle_group_chat_message(
        &mut self,
        group_id: GroupId,
        text: &str,
        target: &BufferId,
    ) {
        let (_, relay_messages) = match self
            .group_chat
            .send_message(group_id, text.as_bytes())
            .await
        {
            Ok(result) => result,
            Err(e) => {
                self.push_status(&format!("Group send error: {e}"));
                return;
            }
        };

        // Send relay messages through the server for members without P2P.
        if !relay_messages.is_empty() {
            if let Some(ref mut conn) = self.connection {
                for relay in &relay_messages {
                    let payload = pirc_crypto::protocol::encode_for_wire(
                        &relay.encrypted_payload,
                    );
                    let msg = Message::new(
                        Command::Pirc(PircSubcommand::GroupMessage),
                        vec![
                            group_id.as_u64().to_string(),
                            relay.target.clone(),
                            payload,
                        ],
                    );
                    if let Err(e) = conn.send(msg).await {
                        warn!(
                            group_id = group_id.as_u64(),
                            target = %relay.target,
                            error = %e,
                            "failed to send relay message"
                        );
                    }
                }
            } else {
                self.push_status("Not connected");
                return;
            }
        }

        // Echo the plaintext locally.
        let nick = self.connection_mgr.nick().to_string();
        self.view.push_message(
            target,
            BufferLine {
                timestamp: current_timestamp(&self.config.ui.timestamp_format),
                sender: Some(nick),
                content: text.to_string(),
                line_type: LineType::Message,
            },
        );
    }

    // ── Helpers ───────────────────────────────────────────────────

    /// Get the [`GroupId`] if the active buffer is a group buffer.
    fn current_group_context(&self) -> Option<GroupId> {
        let active = self.view.buffers().active_id();
        group_id_from_buffer(active)
    }

    /// Push a system message to a group buffer.
    fn push_group_system(&mut self, group_id: GroupId, text: &str) {
        let ts = current_timestamp(&self.config.ui.timestamp_format);
        let buf_id = group_buffer_id(group_id);
        self.view.buffers_mut().ensure_open(buf_id.clone());
        self.view.push_message(
            &buf_id,
            BufferLine {
                timestamp: ts,
                sender: None,
                content: text.to_string(),
                line_type: LineType::System,
            },
        );
    }

    /// Send a protocol message or push a "Not connected" status.
    async fn send_or_status(&mut self, msg: Message) {
        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(msg).await {
                self.push_status(&format!("Send error: {e}"));
            }
        } else {
            self.push_status("Not connected");
        }
    }
}

/// Group buffers use `Channel("group:<id>")` naming convention.
fn group_buffer_id(group_id: GroupId) -> BufferId {
    BufferId::Channel(format!("group:{}", group_id.as_u64()))
}

/// Extract a [`GroupId`] from a buffer ID if it's a group buffer.
pub(super) fn group_id_from_buffer(buf_id: &BufferId) -> Option<GroupId> {
    match buf_id {
        BufferId::Channel(name) if name.starts_with("group:") => {
            name[6..].parse::<GroupId>().ok()
        }
        _ => None,
    }
}
