//! Client-side P2P connection manager.
//!
//! Manages P2P sessions per peer, translates between [`P2pSessionEvent`]s and
//! PIRC protocol signaling messages, and handles incoming signaling dispatched
//! from the server.

use std::collections::HashMap;
use std::net::SocketAddr;

use pirc_p2p::ice::{GathererConfig, IceCandidate};
use pirc_p2p::session::{P2pSession, P2pSessionEvent, SessionState};
use pirc_protocol::{Command, Message, PircSubcommand};
use tracing::{debug, warn};

use crate::config::P2pConfig;

/// Outbound signaling message to send to the server.
#[derive(Debug)]
pub struct SignalingMessage {
    pub message: Message,
}

/// Manager for all active P2P sessions.
pub struct P2pManager {
    /// Active sessions keyed by peer nick.
    sessions: HashMap<String, P2pSession>,
    /// Gatherer config derived from client P2P configuration.
    gatherer_config: GathererConfig,
}

impl P2pManager {
    /// Creates a new manager from the client P2P configuration.
    #[must_use]
    pub fn new(config: &P2pConfig) -> Self {
        let gatherer_config = GathererConfig {
            stun_server: config
                .stun_server
                .as_deref()
                .and_then(|s| s.parse::<SocketAddr>().ok()),
            turn_server: config
                .turn_server
                .as_deref()
                .and_then(|s| s.parse::<SocketAddr>().ok()),
            turn_username: config.turn_username.clone(),
            turn_password: config.turn_password.clone(),
        };
        Self {
            sessions: HashMap::new(),
            gatherer_config,
        }
    }

    /// Returns the current session state for a peer, if any.
    #[must_use]
    pub fn session_state(&self, nick: &str) -> Option<SessionState> {
        self.sessions.get(nick).map(P2pSession::state)
    }

    /// Returns whether a peer has an active (connected) P2P session.
    #[must_use]
    pub fn is_connected(&self, nick: &str) -> bool {
        self.session_state(nick) == Some(SessionState::Connected)
    }

    /// Initiates a P2P connection to a target peer.
    ///
    /// Creates a new session and runs the initiator flow. Returns outbound
    /// signaling messages that must be sent to the server.
    pub async fn initiate(&mut self, target: &str) -> Vec<SignalingMessage> {
        if self.sessions.contains_key(target) {
            debug!(target, "P2P session already exists, skipping initiate");
            return Vec::new();
        }

        let mut session = P2pSession::new(target.to_string(), self.gatherer_config.clone());
        if let Err(e) = session.initiate().await {
            warn!(target, %e, "failed to initiate P2P session");
            return Vec::new();
        }

        let messages = drain_session_messages(&mut session);
        self.sessions.insert(target.to_string(), session);
        messages
    }

    /// Handles an incoming P2P OFFER from a remote peer.
    ///
    /// Creates a responder session, gathers candidates, and returns signaling
    /// messages (ANSWER + ICE candidates) to send back.
    pub async fn handle_offer(
        &mut self,
        sender: &str,
        candidate_lines: &[String],
    ) -> Vec<SignalingMessage> {
        // Parse remote candidates from the offer
        let remote_candidates: Vec<IceCandidate> = candidate_lines
            .iter()
            .filter_map(|line| match IceCandidate::from_sdp_string(line) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!(sender, %e, "failed to parse offer candidate");
                    None
                }
            })
            .collect();

        if remote_candidates.is_empty() {
            warn!(sender, "received offer with no valid candidates");
            return Vec::new();
        }

        // Remove any existing session (new offer supersedes)
        self.sessions.remove(sender);

        let mut session = P2pSession::new(sender.to_string(), self.gatherer_config.clone());
        if let Err(e) = session.respond(remote_candidates).await {
            warn!(sender, %e, "failed to respond to P2P offer");
            return Vec::new();
        }

        // Run connectivity checks
        let _ = session.run_checks().await;

        let messages = drain_session_messages(&mut session);
        self.sessions.insert(sender.to_string(), session);
        messages
    }

    /// Handles an incoming P2P ANSWER from a remote peer.
    ///
    /// Sets remote candidates on the initiator session and runs connectivity
    /// checks. Returns signaling messages (ESTABLISHED or FAILED).
    pub async fn handle_answer(
        &mut self,
        sender: &str,
        candidate_lines: &[String],
    ) -> Vec<SignalingMessage> {
        let Some(session) = self.sessions.get_mut(sender) else {
            warn!(sender, "received P2P answer but no session exists");
            return Vec::new();
        };

        if session.state() != SessionState::OfferSent {
            warn!(sender, state = ?session.state(), "received answer in unexpected state");
            return Vec::new();
        }

        let remote_candidates: Vec<IceCandidate> = candidate_lines
            .iter()
            .filter_map(|line| match IceCandidate::from_sdp_string(line) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!(sender, %e, "failed to parse answer candidate");
                    None
                }
            })
            .collect();

        session.set_remote_candidates(remote_candidates);

        // Run connectivity checks
        let _ = session.run_checks().await;

        drain_session_messages(session)
    }

    /// Handles an incoming trickle ICE candidate from a remote peer.
    pub fn handle_ice_candidate(&mut self, sender: &str, candidate_line: &str) {
        let Some(session) = self.sessions.get_mut(sender) else {
            debug!(sender, "received ICE candidate but no session exists");
            return;
        };

        match IceCandidate::from_sdp_string(candidate_line) {
            Ok(candidate) => session.add_remote_candidate(candidate),
            Err(e) => warn!(sender, %e, "failed to parse trickle ICE candidate"),
        }
    }

    /// Handles a P2P ESTABLISHED notification from a remote peer.
    pub fn handle_established(&mut self, sender: &str) {
        if let Some(session) = self.sessions.get(sender) {
            debug!(
                sender,
                state = ?session.state(),
                "received P2P ESTABLISHED from peer"
            );
        }
    }

    /// Handles a P2P FAILED notification from a remote peer.
    pub fn handle_failed(&mut self, sender: &str, reason: &str) {
        debug!(sender, reason, "received P2P FAILED from peer");
        self.sessions.remove(sender);
    }

    /// Removes and cleans up a session for the given peer.
    pub fn remove_session(&mut self, nick: &str) {
        self.sessions.remove(nick);
    }

    /// Removes all sessions (e.g., on disconnect).
    pub fn clear(&mut self) {
        self.sessions.clear();
    }
}

/// Drains events from a session and converts them to protocol messages.
fn drain_session_messages(session: &mut P2pSession) -> Vec<SignalingMessage> {
    session
        .drain_events()
        .into_iter()
        .map(event_to_message)
        .collect()
}

/// Converts a [`P2pSessionEvent`] to a protocol [`SignalingMessage`].
fn event_to_message(event: P2pSessionEvent) -> SignalingMessage {
    match event {
        P2pSessionEvent::SendOffer { target, offer_data } => {
            let mut params = vec![target];
            params.extend(offer_data);
            SignalingMessage {
                message: Message::new(Command::Pirc(PircSubcommand::P2pOffer), params),
            }
        }
        P2pSessionEvent::SendAnswer {
            target,
            answer_data,
        } => {
            let mut params = vec![target];
            params.extend(answer_data);
            SignalingMessage {
                message: Message::new(Command::Pirc(PircSubcommand::P2pAnswer), params),
            }
        }
        P2pSessionEvent::SendIceCandidate { target, candidate } => SignalingMessage {
            message: Message::new(
                Command::Pirc(PircSubcommand::P2pIce),
                vec![target, candidate],
            ),
        },
        P2pSessionEvent::ConnectionEstablished { target, .. } => SignalingMessage {
            message: Message::new(
                Command::Pirc(PircSubcommand::P2pEstablished),
                vec![target],
            ),
        },
        P2pSessionEvent::ConnectionFailed { target, reason } => SignalingMessage {
            message: Message::new(
                Command::Pirc(PircSubcommand::P2pFailed),
                vec![target, reason],
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> P2pConfig {
        P2pConfig {
            stun_server: None,
            turn_server: None,
            turn_username: None,
            turn_password: None,
        }
    }

    #[test]
    fn new_manager_has_no_sessions() {
        let mgr = P2pManager::new(&default_config());
        assert!(mgr.session_state("alice").is_none());
        assert!(!mgr.is_connected("alice"));
    }

    #[test]
    fn gatherer_config_from_p2p_config() {
        let config = P2pConfig {
            stun_server: Some("192.168.1.1:3478".to_string()),
            turn_server: Some("192.168.1.2:3478".to_string()),
            turn_username: Some("user".to_string()),
            turn_password: Some("pass".to_string()),
        };
        let mgr = P2pManager::new(&config);
        assert_eq!(
            mgr.gatherer_config.stun_server,
            Some("192.168.1.1:3478".parse().unwrap())
        );
        assert_eq!(
            mgr.gatherer_config.turn_server,
            Some("192.168.1.2:3478".parse().unwrap())
        );
        assert_eq!(
            mgr.gatherer_config.turn_username.as_deref(),
            Some("user")
        );
        assert_eq!(
            mgr.gatherer_config.turn_password.as_deref(),
            Some("pass")
        );
    }

    #[test]
    fn gatherer_config_handles_invalid_addresses() {
        let config = P2pConfig {
            stun_server: Some("not-an-address".to_string()),
            turn_server: None,
            turn_username: None,
            turn_password: None,
        };
        let mgr = P2pManager::new(&config);
        assert!(mgr.gatherer_config.stun_server.is_none());
    }

    #[test]
    fn gatherer_config_from_empty_config() {
        let mgr = P2pManager::new(&default_config());
        assert!(mgr.gatherer_config.stun_server.is_none());
        assert!(mgr.gatherer_config.turn_server.is_none());
        assert!(mgr.gatherer_config.turn_username.is_none());
        assert!(mgr.gatherer_config.turn_password.is_none());
    }

    #[tokio::test]
    async fn initiate_creates_session() {
        let mut mgr = P2pManager::new(&default_config());
        let messages = mgr.initiate("bob").await;

        // Should have created a session
        assert!(mgr.session_state("bob").is_some());
        assert_eq!(mgr.session_state("bob"), Some(SessionState::OfferSent));

        // Should produce at least one signaling message (the OFFER)
        let offer_msgs: Vec<_> = messages
            .iter()
            .filter(|m| matches!(m.message.command, Command::Pirc(PircSubcommand::P2pOffer)))
            .collect();
        assert_eq!(offer_msgs.len(), 1);

        // OFFER params: [target, ...candidate_lines]
        assert_eq!(offer_msgs[0].message.params[0], "bob");
        assert!(offer_msgs[0].message.params.len() > 1);
    }

    #[tokio::test]
    async fn initiate_skips_if_session_exists() {
        let mut mgr = P2pManager::new(&default_config());
        mgr.initiate("bob").await;

        // Second initiate should return empty (no-op)
        let messages = mgr.initiate("bob").await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn handle_offer_creates_responder_session() {
        let mut mgr = P2pManager::new(&default_config());

        // Create a fake offer with a host candidate
        let candidate_lines = vec!["host1 1 udp 2130706431 127.0.0.1 9000 typ host".to_string()];
        let messages = mgr.handle_offer("alice", &candidate_lines).await;

        // Should have a session for alice
        assert!(mgr.session_state("alice").is_some());

        // Should produce ANSWER messages
        let answer_msgs: Vec<_> = messages
            .iter()
            .filter(|m| matches!(m.message.command, Command::Pirc(PircSubcommand::P2pAnswer)))
            .collect();
        assert_eq!(answer_msgs.len(), 1);
        assert_eq!(answer_msgs[0].message.params[0], "alice");
    }

    #[tokio::test]
    async fn handle_offer_with_no_valid_candidates() {
        let mut mgr = P2pManager::new(&default_config());
        let messages = mgr.handle_offer("alice", &[]).await;
        assert!(messages.is_empty());
        assert!(mgr.session_state("alice").is_none());
    }

    #[tokio::test]
    async fn handle_answer_without_session_returns_empty() {
        let mut mgr = P2pManager::new(&default_config());
        let candidate_lines = vec!["host1 1 udp 2130706431 127.0.0.1 9000 typ host".to_string()];
        let messages = mgr.handle_answer("alice", &candidate_lines).await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn handle_answer_processes_after_initiate() {
        let mut mgr = P2pManager::new(&default_config());

        // Initiate to create an OfferSent session
        mgr.initiate("bob").await;
        assert_eq!(mgr.session_state("bob"), Some(SessionState::OfferSent));

        // Provide answer with a reachable candidate
        let candidate_lines = vec!["host1 1 udp 2130706431 127.0.0.1 9000 typ host".to_string()];
        let messages = mgr.handle_answer("bob", &candidate_lines).await;

        // Session should have transitioned (Connected or Failed based on reachability)
        let state = mgr.session_state("bob").unwrap();
        assert!(
            state == SessionState::Connected || state == SessionState::Failed,
            "expected Connected or Failed, got {state:?}"
        );

        // Should produce signaling messages (ESTABLISHED or FAILED)
        assert!(!messages.is_empty());
    }

    #[tokio::test]
    async fn handle_ice_candidate_adds_to_session() {
        let mut mgr = P2pManager::new(&default_config());
        mgr.initiate("bob").await;

        // Add a trickle ICE candidate
        mgr.handle_ice_candidate("bob", "host2 1 udp 2130706430 10.0.0.1 8000 typ host");
        // Verify the session still exists (candidate was added, no crash)
        assert!(mgr.session_state("bob").is_some());
    }

    #[test]
    fn handle_ice_candidate_without_session() {
        let mut mgr = P2pManager::new(&default_config());
        // Should not panic
        mgr.handle_ice_candidate("unknown", "host1 1 udp 100 1.2.3.4 5000 typ host");
    }

    #[test]
    fn handle_failed_removes_session() {
        let config = default_config();
        let mut mgr = P2pManager::new(&config);

        // Manually insert a pseudo-session by going through initiate
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(mgr.initiate("bob"));
        assert!(mgr.session_state("bob").is_some());

        mgr.handle_failed("bob", "timeout");
        assert!(mgr.session_state("bob").is_none());
    }

    #[test]
    fn handle_established_does_not_crash_without_session() {
        let mut mgr = P2pManager::new(&default_config());
        mgr.handle_established("unknown");
    }

    #[test]
    fn remove_session_works() {
        let config = default_config();
        let mut mgr = P2pManager::new(&config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(mgr.initiate("bob"));
        assert!(mgr.session_state("bob").is_some());

        mgr.remove_session("bob");
        assert!(mgr.session_state("bob").is_none());
    }

    #[test]
    fn clear_removes_all_sessions() {
        let config = default_config();
        let mut mgr = P2pManager::new(&config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(mgr.initiate("bob"));
        rt.block_on(mgr.initiate("alice"));
        assert!(mgr.session_state("bob").is_some());
        assert!(mgr.session_state("alice").is_some());

        mgr.clear();
        assert!(mgr.session_state("bob").is_none());
        assert!(mgr.session_state("alice").is_none());
    }

    // --- Event-to-message conversion tests ---

    #[test]
    fn event_to_message_offer() {
        let event = P2pSessionEvent::SendOffer {
            target: "bob".to_string(),
            offer_data: vec!["host1 1 udp 100 1.2.3.4 5000 typ host".to_string()],
        };
        let msg = event_to_message(event);
        assert!(matches!(
            msg.message.command,
            Command::Pirc(PircSubcommand::P2pOffer)
        ));
        assert_eq!(msg.message.params[0], "bob");
        assert_eq!(msg.message.params[1], "host1 1 udp 100 1.2.3.4 5000 typ host");
    }

    #[test]
    fn event_to_message_answer() {
        let event = P2pSessionEvent::SendAnswer {
            target: "alice".to_string(),
            answer_data: vec!["host1 1 udp 200 5.6.7.8 6000 typ host".to_string()],
        };
        let msg = event_to_message(event);
        assert!(matches!(
            msg.message.command,
            Command::Pirc(PircSubcommand::P2pAnswer)
        ));
        assert_eq!(msg.message.params[0], "alice");
    }

    #[test]
    fn event_to_message_ice_candidate() {
        let event = P2pSessionEvent::SendIceCandidate {
            target: "bob".to_string(),
            candidate: "host1 1 udp 100 1.2.3.4 5000 typ host".to_string(),
        };
        let msg = event_to_message(event);
        assert!(matches!(
            msg.message.command,
            Command::Pirc(PircSubcommand::P2pIce)
        ));
        assert_eq!(msg.message.params[0], "bob");
        assert_eq!(msg.message.params[1], "host1 1 udp 100 1.2.3.4 5000 typ host");
    }

    #[test]
    fn event_to_message_established() {
        let event = P2pSessionEvent::ConnectionEstablished {
            target: "bob".to_string(),
            local_addr: "127.0.0.1:5000".parse().unwrap(),
            remote_addr: "127.0.0.1:6000".parse().unwrap(),
        };
        let msg = event_to_message(event);
        assert!(matches!(
            msg.message.command,
            Command::Pirc(PircSubcommand::P2pEstablished)
        ));
        assert_eq!(msg.message.params[0], "bob");
    }

    #[test]
    fn event_to_message_failed() {
        let event = P2pSessionEvent::ConnectionFailed {
            target: "bob".to_string(),
            reason: "timeout".to_string(),
        };
        let msg = event_to_message(event);
        assert!(matches!(
            msg.message.command,
            Command::Pirc(PircSubcommand::P2pFailed)
        ));
        assert_eq!(msg.message.params[0], "bob");
        assert_eq!(msg.message.params[1], "timeout");
    }
}
