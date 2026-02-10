use std::collections::HashSet;

use pirc_common::{Nickname, UserMode};
use pirc_protocol::Message;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Per-connection user state.
///
/// Holds all the information the server tracks for a single connected client:
/// identity, modes, away status, timestamps, and a channel for sending
/// messages back to the client's TCP connection.
pub struct UserSession {
    /// Unique connection ID (from [`pirc_network::ConnectionInfo`]).
    pub connection_id: u64,
    /// Current nickname (validated).
    pub nickname: Nickname,
    /// Username from the USER command.
    pub username: String,
    /// Realname from the USER command.
    pub realname: String,
    /// Hostname (peer address or resolved).
    pub hostname: String,
    /// Current user modes.
    pub modes: HashSet<UserMode>,
    /// Away message (`None` means not away).
    pub away_message: Option<String>,
    /// When the user connected.
    pub connected_at: Instant,
    /// Last time the user sent a message (for idle tracking).
    pub last_active: Instant,
    /// Whether registration is complete (NICK + USER both received).
    pub registered: bool,
    /// Sender handle to write messages to this user's connection.
    pub sender: mpsc::UnboundedSender<Message>,
}
