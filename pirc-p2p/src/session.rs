//! P2P session state machine for connection lifecycle management.
//!
//! Orchestrates candidate gathering, offer/answer signaling exchange, and
//! ICE connectivity checks to establish a direct P2P UDP connection between
//! two peers. Supports both initiator and responder roles with a 5-second
//! overall timeout for connection establishment.

use std::net::SocketAddr;

use tracing::{debug, warn};

use crate::connectivity::{ConnectivityChecker, IceRole};
use crate::error::{P2pError, Result};
use crate::ice::{CandidateGatherer, GathererConfig, IceCandidate};

/// Overall timeout for connection establishment in milliseconds.
const SESSION_TIMEOUT_MS: u64 = 5000;

/// State of a P2P session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session created, not yet started.
    Idle,
    /// Gathering local ICE candidates.
    GatheringCandidates,
    /// Initiator: offer sent, waiting for answer.
    OfferSent,
    /// Responder: answer sent, proceeding to connectivity checks.
    AnswerSent,
    /// Running ICE connectivity checks.
    Checking,
    /// Connection established successfully.
    Connected,
    /// Connection failed.
    Failed,
}

/// Events emitted by a [`P2pSession`] for the signaling layer to handle.
#[derive(Debug)]
pub enum P2pSessionEvent {
    /// Send an offer containing local candidates to the target peer.
    SendOffer {
        /// The target peer identifier (opaque string for signaling).
        target: String,
        /// Serialized local candidates (SDP lines).
        offer_data: Vec<String>,
    },
    /// Send an answer containing local candidates in response to an offer.
    SendAnswer {
        /// The target peer identifier.
        target: String,
        /// Serialized local candidates (SDP lines).
        answer_data: Vec<String>,
    },
    /// Send a trickle ICE candidate to the target peer.
    SendIceCandidate {
        /// The target peer identifier.
        target: String,
        /// The serialized candidate.
        candidate: String,
    },
    /// A connection has been established.
    ConnectionEstablished {
        /// The target peer identifier.
        target: String,
        /// The local address of the established connection.
        local_addr: SocketAddr,
        /// The remote address of the established connection.
        remote_addr: SocketAddr,
    },
    /// Connection establishment failed.
    ConnectionFailed {
        /// The target peer identifier.
        target: String,
        /// Reason for failure.
        reason: String,
    },
}

/// Manages the lifecycle of a P2P connection with a single peer.
///
/// Drives the session through candidate gathering, offer/answer exchange,
/// and connectivity checks. Events are collected and can be drained by the
/// signaling layer after each state transition.
pub struct P2pSession {
    /// Current session state.
    state: SessionState,
    /// Identifier of the remote peer.
    target: String,
    /// Configuration for candidate gathering.
    gatherer_config: GathererConfig,
    /// Locally gathered ICE candidates.
    local_candidates: Vec<IceCandidate>,
    /// Remote ICE candidates received via signaling.
    remote_candidates: Vec<IceCandidate>,
    /// The nominated local address after connection is established.
    nominated_local: Option<SocketAddr>,
    /// The nominated remote address after connection is established.
    nominated_remote: Option<SocketAddr>,
    /// Outbound events pending delivery.
    events: Vec<P2pSessionEvent>,
}

impl P2pSession {
    /// Creates a new session targeting the given peer.
    #[must_use]
    pub fn new(target: String, gatherer_config: GathererConfig) -> Self {
        Self {
            state: SessionState::Idle,
            target,
            gatherer_config,
            local_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            nominated_local: None,
            nominated_remote: None,
            events: Vec::new(),
        }
    }

    /// Returns the current session state.
    #[must_use]
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Returns the target peer identifier.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the locally gathered candidates.
    #[must_use]
    pub fn local_candidates(&self) -> &[IceCandidate] {
        &self.local_candidates
    }

    /// Returns the remote candidates received via signaling.
    #[must_use]
    pub fn remote_candidates(&self) -> &[IceCandidate] {
        &self.remote_candidates
    }

    /// Returns the nominated local address if connected.
    #[must_use]
    pub fn nominated_local(&self) -> Option<SocketAddr> {
        self.nominated_local
    }

    /// Returns the nominated remote address if connected.
    #[must_use]
    pub fn nominated_remote(&self) -> Option<SocketAddr> {
        self.nominated_remote
    }

    /// Drains all pending outbound events.
    pub fn drain_events(&mut self) -> Vec<P2pSessionEvent> {
        std::mem::take(&mut self.events)
    }

    /// Runs the initiator flow: gather candidates, send offer, then wait
    /// for an answer via [`set_remote_candidates`] before running checks.
    ///
    /// This method gathers candidates and emits a `SendOffer` event. The
    /// caller must then supply the remote answer via [`set_remote_candidates`]
    /// and call [`run_checks`] to complete the connection.
    pub async fn initiate(&mut self) -> Result<()> {
        if self.state != SessionState::Idle {
            return Err(P2pError::Ice(format!(
                "cannot initiate from state {:?}",
                self.state,
            )));
        }

        self.gather_candidates().await?;

        let offer_data = self
            .local_candidates
            .iter()
            .map(IceCandidate::to_sdp_string)
            .collect();

        self.state = SessionState::OfferSent;
        debug!(target = %self.target, "offer sent");

        self.events.push(P2pSessionEvent::SendOffer {
            target: self.target.clone(),
            offer_data,
        });

        Ok(())
    }

    /// Runs the responder flow: receive the remote offer, gather candidates,
    /// and send an answer. The caller must then call [`run_checks`] to
    /// complete the connection.
    pub async fn respond(&mut self, remote_candidates: Vec<IceCandidate>) -> Result<()> {
        if self.state != SessionState::Idle {
            return Err(P2pError::Ice(format!(
                "cannot respond from state {:?}",
                self.state,
            )));
        }

        self.remote_candidates = remote_candidates;

        self.gather_candidates().await?;

        let answer_data = self
            .local_candidates
            .iter()
            .map(IceCandidate::to_sdp_string)
            .collect();

        self.state = SessionState::AnswerSent;
        debug!(target = %self.target, "answer sent");

        self.events.push(P2pSessionEvent::SendAnswer {
            target: self.target.clone(),
            answer_data,
        });

        Ok(())
    }

    /// Sets remote candidates received via signaling (used by the initiator
    /// after receiving an answer).
    pub fn set_remote_candidates(&mut self, candidates: Vec<IceCandidate>) {
        self.remote_candidates = candidates;
    }

    /// Adds a trickle ICE candidate from the remote peer.
    pub fn add_remote_candidate(&mut self, candidate: IceCandidate) {
        self.remote_candidates.push(candidate);
    }

    /// Runs ICE connectivity checks and transitions to Connected or Failed.
    ///
    /// Must be called after both local and remote candidates are available
    /// (i.e., after `initiate` + `set_remote_candidates`, or after `respond`).
    ///
    /// Applies a 5-second overall timeout for the entire check phase.
    pub async fn run_checks(&mut self) -> Result<()> {
        if self.state != SessionState::OfferSent && self.state != SessionState::AnswerSent {
            return Err(P2pError::Ice(format!(
                "cannot run checks from state {:?}",
                self.state,
            )));
        }

        if self.remote_candidates.is_empty() {
            self.fail("no remote candidates available".into());
            return Err(P2pError::Ice("no remote candidates available".into()));
        }

        // Determine role before transitioning state: initiator (OfferSent)
        // is Controlling, responder (AnswerSent) is Controlled.
        let check_role = if self.state == SessionState::OfferSent {
            IceRole::Controlling
        } else {
            IceRole::Controlled
        };

        self.state = SessionState::Checking;
        debug!(
            target = %self.target,
            local_count = self.local_candidates.len(),
            remote_count = self.remote_candidates.len(),
            "starting connectivity checks",
        );

        let timeout = std::time::Duration::from_millis(SESSION_TIMEOUT_MS);
        let result = tokio::time::timeout(timeout, async {
            let mut checker = ConnectivityChecker::new(
                &self.local_candidates,
                &self.remote_candidates,
                check_role,
            );
            checker.run_checks().await.map(|pair| {
                (pair.local.address, pair.remote.address)
            })
        })
        .await;

        match result {
            Ok(Ok((local_addr, remote_addr))) => {
                self.state = SessionState::Connected;
                self.nominated_local = Some(local_addr);
                self.nominated_remote = Some(remote_addr);

                debug!(
                    target = %self.target,
                    local = %local_addr,
                    remote = %remote_addr,
                    "connection established",
                );

                self.events.push(P2pSessionEvent::ConnectionEstablished {
                    target: self.target.clone(),
                    local_addr,
                    remote_addr,
                });

                Ok(())
            }
            Ok(Err(e)) => {
                let reason = e.to_string();
                self.fail(reason);
                Err(e)
            }
            Err(_) => {
                let reason = "session timeout: connection not established within 5 seconds".into();
                self.fail(reason);
                Err(P2pError::Ice(
                    "session timeout: connection not established within 5 seconds".into(),
                ))
            }
        }
    }

    /// Gathers local ICE candidates.
    async fn gather_candidates(&mut self) -> Result<()> {
        self.state = SessionState::GatheringCandidates;
        debug!(target = %self.target, "gathering candidates");

        let gatherer = CandidateGatherer::new(GathererConfig {
            stun_server: self.gatherer_config.stun_server,
            turn_server: self.gatherer_config.turn_server,
            turn_username: self.gatherer_config.turn_username.clone(),
            turn_password: self.gatherer_config.turn_password.clone(),
        });

        self.local_candidates = gatherer.gather().await?;

        // Emit trickle ICE candidates
        for candidate in &self.local_candidates {
            self.events.push(P2pSessionEvent::SendIceCandidate {
                target: self.target.clone(),
                candidate: candidate.to_sdp_string(),
            });
        }

        debug!(
            target = %self.target,
            count = self.local_candidates.len(),
            "candidate gathering complete",
        );

        Ok(())
    }

    /// Transitions to the Failed state and emits a `ConnectionFailed` event.
    fn fail(&mut self, reason: String) {
        self.state = SessionState::Failed;
        warn!(target = %self.target, reason = %reason, "session failed");
        self.events.push(P2pSessionEvent::ConnectionFailed {
            target: self.target.clone(),
            reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ice::{CandidateType, IceCandidate};
    use crate::stun::{StunAttribute, StunMessage};
    use tokio::net::UdpSocket;

    fn default_config() -> GathererConfig {
        GathererConfig {
            stun_server: None,
            turn_server: None,
            turn_username: None,
            turn_password: None,
        }
    }

    fn host_candidate(addr: &str) -> IceCandidate {
        IceCandidate::new(
            CandidateType::Host,
            addr.parse().unwrap(),
            65535,
            "host1".into(),
            1,
        )
    }

    // --- State machine creation tests ---

    #[test]
    fn session_starts_idle() {
        let session = P2pSession::new("peer1".into(), default_config());
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.target(), "peer1");
        assert!(session.local_candidates().is_empty());
        assert!(session.remote_candidates().is_empty());
        assert!(session.nominated_local().is_none());
        assert!(session.nominated_remote().is_none());
    }

    #[test]
    fn session_state_equality() {
        assert_eq!(SessionState::Idle, SessionState::Idle);
        assert_eq!(SessionState::GatheringCandidates, SessionState::GatheringCandidates);
        assert_eq!(SessionState::OfferSent, SessionState::OfferSent);
        assert_eq!(SessionState::AnswerSent, SessionState::AnswerSent);
        assert_eq!(SessionState::Checking, SessionState::Checking);
        assert_eq!(SessionState::Connected, SessionState::Connected);
        assert_eq!(SessionState::Failed, SessionState::Failed);
        assert_ne!(SessionState::Idle, SessionState::Failed);
    }

    #[test]
    fn drain_events_returns_empty_initially() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        let events = session.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn set_remote_candidates_stores_them() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        let candidates = vec![host_candidate("192.168.1.2:6000")];
        session.set_remote_candidates(candidates.clone());
        assert_eq!(session.remote_candidates().len(), 1);
        assert_eq!(session.remote_candidates()[0].address, candidates[0].address);
    }

    #[test]
    fn add_remote_candidate_appends() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.add_remote_candidate(host_candidate("192.168.1.2:6000"));
        session.add_remote_candidate(host_candidate("192.168.1.3:7000"));
        assert_eq!(session.remote_candidates().len(), 2);
    }

    // --- Initiator flow tests ---

    #[tokio::test]
    async fn initiate_gathers_and_sends_offer() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();

        assert_eq!(session.state(), SessionState::OfferSent);
        assert!(!session.local_candidates().is_empty());

        let events = session.drain_events();
        // Should have trickle ICE candidates + SendOffer
        let offer_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::SendOffer { .. }))
            .collect();
        assert_eq!(offer_events.len(), 1);

        if let P2pSessionEvent::SendOffer { target, offer_data } = &offer_events[0] {
            assert_eq!(target, "peer1");
            assert!(!offer_data.is_empty());
        }
    }

    #[tokio::test]
    async fn initiate_rejects_non_idle_state() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();
        // Second call should fail
        let result = session.initiate().await;
        assert!(result.is_err());
    }

    // --- Responder flow tests ---

    #[tokio::test]
    async fn respond_gathers_and_sends_answer() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        let remote = vec![host_candidate("192.168.1.2:6000")];
        session.respond(remote).await.unwrap();

        assert_eq!(session.state(), SessionState::AnswerSent);
        assert!(!session.local_candidates().is_empty());
        assert_eq!(session.remote_candidates().len(), 1);

        let events = session.drain_events();
        let answer_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::SendAnswer { .. }))
            .collect();
        assert_eq!(answer_events.len(), 1);

        if let P2pSessionEvent::SendAnswer { target, answer_data } = &answer_events[0] {
            assert_eq!(target, "peer1");
            assert!(!answer_data.is_empty());
        }
    }

    #[tokio::test]
    async fn respond_rejects_non_idle_state() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();
        let result = session.respond(vec![]).await;
        assert!(result.is_err());
    }

    // --- Connectivity check tests ---

    #[tokio::test]
    async fn run_checks_rejects_invalid_state() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        // Should fail from Idle state
        let result = session.run_checks().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_checks_fails_without_remote_candidates() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();
        // Don't set remote candidates
        let result = session.run_checks().await;
        assert!(result.is_err());
        assert_eq!(session.state(), SessionState::Failed);

        let events = session.drain_events();
        let fail_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::ConnectionFailed { .. }))
            .collect();
        assert_eq!(fail_events.len(), 1);
    }

    #[tokio::test]
    async fn initiator_full_flow_with_mock_stun() {
        // Set up a mock STUN responder
        let responder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let responder_addr = responder.local_addr().unwrap();

        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = responder.recv_from(&mut buf).await.unwrap();
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();

            let response = StunMessage {
                msg_type: 0x0101,
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(src)],
            };
            responder.send_to(&response.to_bytes(), src).await.unwrap();
        });

        let mut session = P2pSession::new("peer1".into(), default_config());

        // Step 1: Initiate
        session.initiate().await.unwrap();
        assert_eq!(session.state(), SessionState::OfferSent);

        // Step 2: Receive answer (remote candidates)
        let remote = vec![IceCandidate::new(
            CandidateType::Host,
            responder_addr,
            65535,
            "host1".into(),
            1,
        )];
        session.set_remote_candidates(remote);

        // Step 3: Run checks
        session.run_checks().await.unwrap();
        assert_eq!(session.state(), SessionState::Connected);
        assert!(session.nominated_local().is_some());
        assert!(session.nominated_remote().is_some());

        let events = session.drain_events();
        let connected_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::ConnectionEstablished { .. }))
            .collect();
        assert_eq!(connected_events.len(), 1);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn responder_full_flow_with_mock_stun() {
        let responder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let responder_addr = responder.local_addr().unwrap();

        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = responder.recv_from(&mut buf).await.unwrap();
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();

            let response = StunMessage {
                msg_type: 0x0101,
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(src)],
            };
            responder.send_to(&response.to_bytes(), src).await.unwrap();
        });

        let mut session = P2pSession::new("peer1".into(), default_config());

        // Step 1: Respond to remote offer
        let remote = vec![IceCandidate::new(
            CandidateType::Host,
            responder_addr,
            65535,
            "host1".into(),
            1,
        )];
        session.respond(remote).await.unwrap();
        assert_eq!(session.state(), SessionState::AnswerSent);

        // Step 2: Run checks
        session.run_checks().await.unwrap();
        assert_eq!(session.state(), SessionState::Connected);
        assert!(session.nominated_local().is_some());
        assert_eq!(session.nominated_remote().unwrap(), responder_addr);

        let events = session.drain_events();
        let connected_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::ConnectionEstablished { .. }))
            .collect();
        assert_eq!(connected_events.len(), 1);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn connectivity_check_failure_transitions_to_failed() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();

        // Set remote candidate to unreachable address
        let remote = vec![host_candidate("127.0.0.1:1")];
        session.set_remote_candidates(remote);

        let result = session.run_checks().await;
        assert!(result.is_err());
        assert_eq!(session.state(), SessionState::Failed);

        let events = session.drain_events();
        let fail_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::ConnectionFailed { .. }))
            .collect();
        assert_eq!(fail_events.len(), 1);
    }

    // --- Trickle ICE event tests ---

    #[tokio::test]
    async fn trickle_ice_candidates_emitted_during_gathering() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();

        let events = session.drain_events();
        let trickle_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, P2pSessionEvent::SendIceCandidate { .. }))
            .collect();
        // At least one trickle candidate (the host candidate)
        assert!(!trickle_events.is_empty());
    }

    // --- Event variant tests ---

    #[test]
    fn event_debug_formatting() {
        let event = P2pSessionEvent::SendOffer {
            target: "peer1".into(),
            offer_data: vec!["host1 1 udp 100 1.2.3.4 5000 typ host".into()],
        };
        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("SendOffer"));

        let event = P2pSessionEvent::ConnectionFailed {
            target: "peer1".into(),
            reason: "timeout".into(),
        };
        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("ConnectionFailed"));
    }

    #[tokio::test]
    async fn offer_data_contains_sdp_candidates() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        session.initiate().await.unwrap();

        let events = session.drain_events();
        let offer = events
            .iter()
            .find(|e| matches!(e, P2pSessionEvent::SendOffer { .. }))
            .unwrap();

        if let P2pSessionEvent::SendOffer { offer_data, .. } = offer {
            for sdp_line in offer_data {
                // Each line should be parseable as an ICE candidate
                let parsed = IceCandidate::from_sdp_string(sdp_line);
                assert!(parsed.is_ok(), "failed to parse SDP line: {sdp_line}");
            }
        }
    }

    #[tokio::test]
    async fn answer_data_contains_sdp_candidates() {
        let mut session = P2pSession::new("peer1".into(), default_config());
        let remote = vec![host_candidate("192.168.1.2:6000")];
        session.respond(remote).await.unwrap();

        let events = session.drain_events();
        let answer = events
            .iter()
            .find(|e| matches!(e, P2pSessionEvent::SendAnswer { .. }))
            .unwrap();

        if let P2pSessionEvent::SendAnswer { answer_data, .. } = answer {
            for sdp_line in answer_data {
                let parsed = IceCandidate::from_sdp_string(sdp_line);
                assert!(parsed.is_ok(), "failed to parse SDP line: {sdp_line}");
            }
        }
    }
}
