//! Key exchange protocol handling for the App.
//!
//! Contains methods on [`App`] that handle incoming PIRC KEYEXCHANGE,
//! KEYEXCHANGE-ACK, KEYEXCHANGE-COMPLETE, and ENCRYPTED messages,
//! as well as outgoing key exchange initiation for private messages.

use pirc_crypto::message::EncryptedMessage;
use pirc_crypto::protocol::{decode_from_wire, encode_for_wire, KeyExchangeMessage};
use pirc_network::connection::AsyncTransport;
use pirc_protocol::{Command, Message, Prefix, PircSubcommand};
use tracing::{info, warn};

use super::{current_timestamp, App};
use crate::client_command::EncryptionSubcommand;
use crate::encryption::EncryptionStatus;
use crate::tui::buffer_manager::BufferId;
use crate::tui::message_buffer::{BufferLine, LineType};

/// Maximum base64 characters per PIRC KEYEXCHANGE chunk.
///
/// 400 bytes is conservative; the worst-case server prefix and IRC overhead
/// consume up to ~60 bytes of the 512-byte line limit, leaving ~452 bytes.
const KEY_EXCHANGE_CHUNK_SIZE: usize = 400;

/// Parse a chunk header of the form `"<n>/<total>"`.
///
/// Returns `Some((n, total))` where both are 1-based and `n <= total`, or
/// `None` if the string is not a valid header.
fn parse_chunk_header(s: &str) -> Option<(usize, usize)> {
    let (n_str, total_str) = s.split_once('/')?;
    let n = n_str.parse::<usize>().ok()?;
    let total = total_str.parse::<usize>().ok()?;
    if n >= 1 && total >= 1 && n <= total {
        Some((n, total))
    } else {
        None
    }
}

impl App {
    /// Send a `PIRC KEYEXCHANGE <target> <data>` message, splitting into
    /// multiple chunks if `data` exceeds [`KEY_EXCHANGE_CHUNK_SIZE`].
    ///
    /// Returns `true` if all sends succeeded.
    async fn send_chunked_key_exchange(&mut self, target: &str, data: &str) -> bool {
        let raw = data.as_bytes();
        if raw.len() <= KEY_EXCHANGE_CHUNK_SIZE {
            let msg = Message::new(
                Command::Pirc(PircSubcommand::KeyExchange),
                vec![target.to_string(), data.to_string()],
            );
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(msg).await {
                    warn!("Failed to send key exchange to {target}: {e}");
                    return false;
                }
            }
            return true;
        }

        let chunks: Vec<&str> = raw
            .chunks(KEY_EXCHANGE_CHUNK_SIZE)
            .map(|c| std::str::from_utf8(c).expect("base64 is valid UTF-8"))
            .collect();
        let total = chunks.len();
        for (i, chunk) in chunks.iter().enumerate() {
            let n = i + 1;
            let msg = Message::new(
                Command::Pirc(PircSubcommand::KeyExchange),
                vec![target.to_string(), format!("{n}/{total}"), chunk.to_string()],
            );
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(msg).await {
                    warn!("Failed to send key exchange chunk {n}/{total} to {target}: {e}");
                    return false;
                }
            }
        }
        true
    }

    /// Generate and upload our pre-key bundle to the server.
    ///
    /// Sends `PIRC KEYEXCHANGE * <base64-bundle>` where `*` as target
    /// signals "store my bundle" to the server. Large bundles are split
    /// into multiple chunks automatically.
    pub(super) async fn upload_pre_key_bundle(&mut self) {
        let bundle = self.encryption.create_pre_key_bundle();
        let bundle_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
        let encoded = encode_for_wire(&bundle_msg.to_bytes());

        if !self.send_chunked_key_exchange("*", &encoded).await {
            self.push_status("Failed to upload encryption keys");
            return;
        }
        info!("Pre-key bundle uploaded");
    }

    /// Handle PIRC subcommand messages related to encryption.
    ///
    /// Returns `true` if the message was handled, `false` if it should be
    /// passed to the general message router.
    pub(super) async fn handle_pirc_message(&mut self, msg: &Message) -> bool {
        let sender = match &msg.prefix {
            Some(Prefix::User { nick, .. }) => nick.as_ref().to_string(),
            _ => return false,
        };

        match &msg.command {
            Command::Pirc(PircSubcommand::KeyExchange) => {
                // Detect chunked form: params = [target, "n/total", chunk_data]
                let assembled: Option<String> = if msg.params.len() >= 3 {
                    if let Some((n, total)) = parse_chunk_header(msg.params[1].as_str()) {
                        let chunk = msg.params[2].clone();
                        let entry = self
                            .chunk_bufs
                            .entry(sender.clone())
                            .or_insert_with(|| (vec![None; total], total));
                        if entry.1 != total {
                            *entry = (vec![None; total], total);
                        }
                        if n >= 1 && n <= total {
                            entry.0[n - 1] = Some(chunk);
                        }
                        if entry.0.iter().all(|c| c.is_some()) {
                            let joined: String = entry
                                .0
                                .iter()
                                .map(|c| c.as_deref().unwrap_or(""))
                                .collect();
                            self.chunk_bufs.remove(&sender);
                            Some(joined)
                        } else {
                            None
                        }
                    } else {
                        msg.params.get(1).cloned()
                    }
                } else {
                    msg.params.get(1).cloned()
                };

                if let Some(data) = assembled {
                    self.handle_key_exchange_message(&sender, &data).await;
                }
                true
            }
            Command::Pirc(PircSubcommand::KeyExchangeComplete) => {
                self.handle_key_exchange_complete(&sender);
                true
            }
            Command::Pirc(PircSubcommand::Encrypted) => {
                if let Some(data) = msg.params.get(1) {
                    self.handle_encrypted_message(&sender, data);
                }
                true
            }
            _ => false,
        }
    }

    /// Handle an incoming `PIRC KEYEXCHANGE <sender> <data>` message.
    ///
    /// Decodes the wire data, determines the key exchange message variant,
    /// and dispatches to the appropriate handler.
    pub(super) async fn handle_key_exchange_message(&mut self, sender: &str, data: &str) {
        let bytes = match decode_from_wire(data) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to decode key exchange data from {sender}: {e}");
                return;
            }
        };

        let ke_msg = match KeyExchangeMessage::from_bytes(&bytes) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to parse key exchange message from {sender}: {e}");
                return;
            }
        };

        match ke_msg {
            KeyExchangeMessage::RequestBundle => {
                self.handle_request_bundle(sender).await;
            }
            KeyExchangeMessage::Bundle(bundle) => {
                self.handle_bundle_response(sender, &bundle).await;
            }
            KeyExchangeMessage::InitMessage(init) => {
                self.handle_init_message(sender, &init).await;
            }
            KeyExchangeMessage::Complete => {
                self.handle_key_exchange_complete(sender);
            }
        }
    }

    /// Handle a `RequestBundle` — someone wants our pre-key bundle.
    ///
    /// Generates our bundle and sends it back to the requester, chunked if necessary.
    async fn handle_request_bundle(&mut self, requester: &str) {
        let bundle = self.encryption.create_pre_key_bundle();
        let bundle_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
        let encoded = encode_for_wire(&bundle_msg.to_bytes());

        if !self.send_chunked_key_exchange(requester, &encoded).await {
            warn!("Failed to send bundle to {requester}");
        }
    }

    /// Handle a `Bundle` response — we requested a peer's bundle and got it.
    ///
    /// Performs X3DH, creates a session, sends the init message, and
    /// encrypts+sends any queued messages.
    async fn handle_bundle_response(
        &mut self,
        peer: &str,
        bundle: &pirc_crypto::prekey::PreKeyBundle,
    ) {
        let (init_msg, encrypted_queued) = match self.encryption.handle_bundle_response(peer, bundle)
        {
            Ok(result) => result,
            Err(e) => {
                warn!("X3DH failed with {peer}: {e}");
                self.push_status(&format!("Key exchange with {peer} failed: {e}"));
                return;
            }
        };

        // Send the X3DH init message to the peer, chunked if necessary.
        let init_ke = KeyExchangeMessage::InitMessage(Box::new(init_msg));
        let encoded = encode_for_wire(&init_ke.to_bytes());

        if !self.send_chunked_key_exchange(peer, &encoded).await {
            self.push_status(&format!("Failed to send key exchange to {peer}"));
            return;
        }

        // Send any queued encrypted messages
        for encrypted in &encrypted_queued {
            self.send_encrypted_message(peer, encrypted).await;
        }
    }

    /// Handle an `InitMessage` — a peer completed X3DH and sent us the init data.
    ///
    /// Creates our session and sends `PIRC KEYEXCHANGE-COMPLETE` back.
    async fn handle_init_message(
        &mut self,
        peer: &str,
        init: &pirc_crypto::x3dh::X3DHInitMessage,
    ) {
        match self.encryption.handle_init_message(peer, init) {
            Ok(KeyExchangeMessage::Complete) => {
                // Send KEYEXCHANGE-COMPLETE to peer
                let msg = Message::new(
                    Command::Pirc(PircSubcommand::KeyExchangeComplete),
                    vec![peer.to_string()],
                );

                if let Some(ref mut conn) = self.connection {
                    if let Err(e) = conn.send(msg).await {
                        warn!("Failed to send KEYEXCHANGE-COMPLETE to {peer}: {e}");
                    }
                }

                self.push_status(&format!("Encrypted session established with {peer}"));
                self.push_encryption_event(
                    peer,
                    &format!("Encrypted session established with {peer}"),
                );
                self.encryption.persist();
            }
            Ok(_) => {
                warn!("Unexpected key exchange response from handle_init_message for {peer}");
            }
            Err(e) => {
                warn!("Failed to handle init message from {peer}: {e}");
                self.push_status(&format!("Key exchange with {peer} failed: {e}"));
            }
        }
    }

    /// Handle a `PIRC KEYEXCHANGE-COMPLETE` — the peer acknowledges session establishment.
    ///
    /// Promotes our pending session to active.
    pub(super) fn handle_key_exchange_complete(&mut self, peer: &str) {
        self.encryption.handle_complete(peer);
        self.push_status(&format!("Encrypted session established with {peer}"));
        self.push_encryption_event(peer, &format!("Encrypted session established with {peer}"));
        self.encryption.persist();
    }

    /// Initiate a key exchange with a peer and optionally queue a message.
    ///
    /// Called when sending a private message to a peer with no active session.
    /// Sends `PIRC KEYEXCHANGE <peer> <request-bundle>` to the server.
    pub(super) async fn initiate_key_exchange(
        &mut self,
        peer: &str,
        queued_message: Option<&str>,
    ) {
        // Don't re-initiate if already pending
        if self.encryption.has_pending_exchange(peer) {
            if let Some(text) = queued_message {
                self.encryption.queue_message(peer, text.as_bytes().to_vec());
            }
            return;
        }

        let ke_msg = self.encryption.initiate_key_exchange(peer);
        let encoded = encode_for_wire(&ke_msg.to_bytes());

        let msg = Message::new(
            Command::Pirc(PircSubcommand::KeyExchange),
            vec![peer.to_string(), encoded],
        );

        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(msg).await {
                warn!("Failed to send key exchange request to {peer}: {e}");
                self.push_status(&format!("Failed to initiate encryption with {peer}: {e}"));
                return;
            }
        }

        // Queue the message if provided
        if let Some(text) = queued_message {
            self.encryption.queue_message(peer, text.as_bytes().to_vec());
        }

        self.push_status(&format!("Establishing encrypted session with {peer}..."));
        self.push_encryption_event(
            peer,
            &format!("Establishing encrypted session with {peer}..."),
        );
    }

    /// Send a `PIRC ENCRYPTED <peer> <base64-data>` message.
    async fn send_encrypted_message(&mut self, peer: &str, encrypted: &EncryptedMessage) {
        let encoded = encode_for_wire(&encrypted.to_bytes());

        let msg = Message::new(
            Command::Pirc(PircSubcommand::Encrypted),
            vec![peer.to_string(), encoded],
        );

        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(msg).await {
                warn!("Failed to send encrypted message to {peer}: {e}");
            }
        }
    }

    /// Handle an incoming `PIRC ENCRYPTED <sender> <data>` message.
    ///
    /// Decrypts the message and displays it in the query buffer.
    pub(super) fn handle_encrypted_message(&mut self, sender: &str, data: &str) {
        let bytes = match decode_from_wire(data) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to decode encrypted message from {sender}: {e}");
                return;
            }
        };

        let encrypted = match EncryptedMessage::from_bytes(&bytes) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to parse encrypted message from {sender}: {e}");
                return;
            }
        };

        let plaintext = match self.encryption.decrypt(sender, &encrypted) {
            Ok(pt) => pt,
            Err(e) => {
                warn!("Failed to decrypt message from {sender}: {e}");
                let ts = current_timestamp(&self.config.ui.timestamp_format);
                self.view.push_message(
                    &BufferId::Query(sender.to_string()),
                    BufferLine {
                        timestamp: ts,
                        sender: None,
                        content: format!("Failed to decrypt message from {sender}"),
                        line_type: LineType::Error,
                    },
                );
                return;
            }
        };

        let content = String::from_utf8_lossy(&plaintext).to_string();
        let ts = current_timestamp(&self.config.ui.timestamp_format);

        self.view.push_message(
            &BufferId::Query(sender.to_string()),
            BufferLine {
                timestamp: ts,
                sender: Some(sender.to_string()),
                content,
                line_type: LineType::Message,
            },
        );
    }

    /// Push an encryption lifecycle event as a system message to the query buffer.
    fn push_encryption_event(&mut self, peer: &str, message: &str) {
        let ts = current_timestamp(&self.config.ui.timestamp_format);
        self.view.push_message(
            &BufferId::Query(peer.to_string()),
            BufferLine {
                timestamp: ts,
                sender: None,
                content: message.to_string(),
                line_type: LineType::System,
            },
        );
    }

    /// Handle a `/msg <nick> <text>` or `/query <nick> <text>` to a user.
    ///
    /// Routes through encryption: if a session exists, encrypts; if not,
    /// initiates key exchange and queues the message.
    pub(super) async fn handle_private_msg_command(&mut self, target: &str, message: &str) {
        if self.connection.is_none() {
            self.push_status("Not connected");
            return;
        }

        // Try to send encrypted; this handles session, pending, and initiation
        let handled = self.send_private_message(target, message).await;

        // Echo the message locally in the query buffer
        let nick = self.connection_mgr.nick().to_string();
        let ts = current_timestamp(&self.config.ui.timestamp_format);
        self.view.push_message(
            &BufferId::Query(target.to_string()),
            BufferLine {
                timestamp: ts,
                sender: Some(nick),
                content: message.to_string(),
                line_type: LineType::Message,
            },
        );

        if !handled {
            // Encryption not available — fall back to plaintext PRIVMSG
            let msg = Message::new(
                Command::Privmsg,
                vec![target.to_string(), message.to_string()],
            );
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(msg).await {
                    self.push_status(&format!("Send error: {e}"));
                }
            }
        }
    }

    /// Send a private message to a peer, encrypting if a session exists.
    ///
    /// If no session exists, initiates a key exchange and queues the message.
    /// Returns `true` if the message was handled (encrypted or queued),
    /// `false` if encryption is not available and the caller should send plaintext.
    pub(super) async fn send_private_message(&mut self, peer: &str, text: &str) -> bool {
        // If we have an active session, encrypt and send
        if self.encryption.has_session(peer) {
            match self.encryption.encrypt(peer, text.as_bytes()) {
                Ok(encrypted) => {
                    self.send_encrypted_message(peer, &encrypted).await;
                    return true;
                }
                Err(e) => {
                    warn!("Encryption failed for {peer}: {e}");
                    self.push_status(&format!("Encryption failed for {peer}: {e}"));
                    return false;
                }
            }
        }

        // If exchange is pending, queue the message
        if self.encryption.has_pending_exchange(peer) {
            self.encryption.queue_message(peer, text.as_bytes().to_vec());
            self.push_status(&format!(
                "Message to {peer} queued (key exchange in progress)"
            ));
            return true;
        }

        // No session and no pending exchange — initiate key exchange
        self.initiate_key_exchange(peer, Some(text)).await;
        true
    }

    // ── /encryption and /fingerprint command handlers ────────────────

    /// Handle `/encryption <subcommand>`.
    pub(super) fn handle_encryption_command(&mut self, sub: &EncryptionSubcommand) {
        match sub {
            EncryptionSubcommand::Status => self.handle_encryption_status(),
            EncryptionSubcommand::Reset(nick) => self.handle_encryption_reset(nick),
            EncryptionSubcommand::Info(nick) => self.handle_encryption_info(nick),
        }
    }

    /// `/encryption status` — list all peers with encryption status.
    fn handle_encryption_status(&mut self) {
        let peers = self.encryption.list_peers();
        if peers.is_empty() {
            self.push_status("No active or pending encrypted sessions.");
            return;
        }

        self.push_status("Encrypted sessions:");
        for (peer, status) in &peers {
            let status_str = match status {
                EncryptionStatus::Active => "active",
                EncryptionStatus::Establishing => "establishing",
                EncryptionStatus::None => "none",
            };
            self.push_status(&format!("  {peer}: {status_str}"));
        }
    }

    /// `/encryption reset <nick>` — remove session with a peer.
    fn handle_encryption_reset(&mut self, nick: &str) {
        self.encryption.remove_session(nick);
        self.push_status(&format!(
            "Encrypted session with {nick} has been reset. Next message will trigger fresh key exchange."
        ));
        self.push_encryption_event(
            nick,
            &format!("Encrypted session with {nick} has been reset"),
        );
    }

    /// `/encryption info <nick>` — show detailed encryption info for a session.
    fn handle_encryption_info(&mut self, nick: &str) {
        let status = self.encryption.encryption_status(nick);
        let status_str = match status {
            EncryptionStatus::Active => "active",
            EncryptionStatus::Establishing => "establishing",
            EncryptionStatus::None => "none",
        };

        self.push_status(&format!("Encryption info for {nick}:"));
        self.push_status(&format!("  Session state: {status_str}"));

        if let Some(fp) = self.encryption.get_peer_fingerprint(nick) {
            self.push_status(&format!("  Peer fingerprint: {fp}"));
        } else {
            self.push_status("  Peer fingerprint: unknown (no prior key exchange)");
        }
    }

    /// Handle `/fingerprint [nick]`.
    pub(super) fn handle_fingerprint_command(&mut self, nick: Option<&str>) {
        match nick {
            None => {
                let fp = self.encryption.get_identity_fingerprint();
                self.push_status(&format!("Your fingerprint: {fp}"));
            }
            Some(nick) => {
                if let Some(fp) = self.encryption.get_peer_fingerprint(nick) {
                    self.push_status(&format!("Fingerprint for {nick}: {fp}"));
                } else {
                    self.push_status(&format!(
                        "No fingerprint available for {nick} (no prior key exchange)"
                    ));
                }
            }
        }
    }
}
