//! P2P session lifecycle integration tests.
//!
//! Exercises the P2P session state machine: initiator and responder flows,
//! state transitions, event emission, loopback connection establishment, and
//! failure modes.

use pirc_p2p::ice::{CandidateType, GathererConfig, IceCandidate};
use pirc_p2p::session::{P2pSession, P2pSessionEvent, SessionState};

use super::{host_candidate, spawn_mock_stun_server};

fn default_config() -> GathererConfig {
    GathererConfig {
        stun_server: None,
        turn_server: None,
        turn_username: None,
        turn_password: None,
    }
}

// --- Initial state ---

#[tokio::test]
async fn session_starts_idle() {
    let session = P2pSession::new("peer1".into(), default_config());

    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(session.target(), "peer1");
    assert!(session.local_candidates().is_empty());
    assert!(session.remote_candidates().is_empty());
    assert!(session.nominated_local().is_none());
    assert!(session.nominated_remote().is_none());
}

#[tokio::test]
async fn drain_events_empty_initially() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    assert!(session.drain_events().is_empty());
}

// --- Candidate management ---

#[tokio::test]
async fn set_remote_candidates_stores_them() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    let candidates = vec![host_candidate("192.168.1.2:6000")];
    session.set_remote_candidates(candidates.clone());

    assert_eq!(session.remote_candidates().len(), 1);
    assert_eq!(session.remote_candidates()[0].address, candidates[0].address);
}

#[tokio::test]
async fn add_remote_candidate_appends() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.add_remote_candidate(host_candidate("192.168.1.2:6000"));
    session.add_remote_candidate(host_candidate("192.168.1.3:7000"));
    assert_eq!(session.remote_candidates().len(), 2);
}

// --- Initiator flow ---

#[tokio::test]
async fn initiate_transitions_to_offer_sent() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    assert_eq!(session.state(), SessionState::OfferSent);
    assert!(!session.local_candidates().is_empty());
}

#[tokio::test]
async fn initiate_emits_offer_and_trickle_events() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    let events = session.drain_events();

    // Should have trickle ICE candidate events + SendOffer
    let trickle: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::SendIceCandidate { .. }))
        .collect();
    assert!(!trickle.is_empty());

    let offers: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::SendOffer { .. }))
        .collect();
    assert_eq!(offers.len(), 1);

    if let P2pSessionEvent::SendOffer { target, offer_data } = &offers[0] {
        assert_eq!(target, "peer1");
        assert!(!offer_data.is_empty());
        // Each SDP line should be parseable
        for sdp in offer_data {
            assert!(IceCandidate::from_sdp_string(sdp).is_ok());
        }
    }
}

#[tokio::test]
async fn initiate_rejects_non_idle_state() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    let result = session.initiate().await;
    assert!(result.is_err());
}

// --- Responder flow ---

#[tokio::test]
async fn respond_transitions_to_answer_sent() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    let remote = vec![host_candidate("192.168.1.2:6000")];
    session.respond(remote).await.unwrap();

    assert_eq!(session.state(), SessionState::AnswerSent);
    assert!(!session.local_candidates().is_empty());
    assert_eq!(session.remote_candidates().len(), 1);
}

#[tokio::test]
async fn respond_emits_answer_event() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    let remote = vec![host_candidate("192.168.1.2:6000")];
    session.respond(remote).await.unwrap();

    let events = session.drain_events();
    let answers: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::SendAnswer { .. }))
        .collect();
    assert_eq!(answers.len(), 1);

    if let P2pSessionEvent::SendAnswer { target, answer_data } = &answers[0] {
        assert_eq!(target, "peer1");
        assert!(!answer_data.is_empty());
        for sdp in answer_data {
            assert!(IceCandidate::from_sdp_string(sdp).is_ok());
        }
    }
}

#[tokio::test]
async fn respond_rejects_non_idle_state() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    let result = session.respond(vec![]).await;
    assert!(result.is_err());
}

// --- Connectivity checks ---

#[tokio::test]
async fn run_checks_rejects_idle_state() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    let result = session.run_checks().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn run_checks_fails_without_remote_candidates() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    let result = session.run_checks().await;
    assert!(result.is_err());
    assert_eq!(session.state(), SessionState::Failed);

    let events = session.drain_events();
    let fails: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::ConnectionFailed { .. }))
        .collect();
    assert_eq!(fails.len(), 1);
}

// --- Full session flows with mock STUN ---

#[tokio::test]
async fn initiator_full_flow_connects() {
    let (responder_addr, _handle) = spawn_mock_stun_server().await;

    let mut session = P2pSession::new("peer1".into(), default_config());

    // Step 1: Initiate
    session.initiate().await.unwrap();
    assert_eq!(session.state(), SessionState::OfferSent);

    // Step 2: Receive answer
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
    let connected: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::ConnectionEstablished { .. }))
        .collect();
    assert_eq!(connected.len(), 1);
}

#[tokio::test]
async fn responder_full_flow_connects() {
    let (responder_addr, _handle) = spawn_mock_stun_server().await;

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
    let connected: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::ConnectionEstablished { .. }))
        .collect();
    assert_eq!(connected.len(), 1);
}

// --- Failure modes ---

#[tokio::test]
async fn connectivity_failure_transitions_to_failed() {
    let mut session = P2pSession::new("peer1".into(), default_config());
    session.initiate().await.unwrap();

    // Unreachable remote
    session.set_remote_candidates(vec![host_candidate("127.0.0.1:1")]);

    let result = session.run_checks().await;
    assert!(result.is_err());
    assert_eq!(session.state(), SessionState::Failed);

    let events = session.drain_events();
    let fails: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, P2pSessionEvent::ConnectionFailed { .. }))
        .collect();
    assert_eq!(fails.len(), 1);

    if let P2pSessionEvent::ConnectionFailed { target, reason } = &fails[0] {
        assert_eq!(target, "peer1");
        assert!(!reason.is_empty());
    }
}

// --- State enumeration ---

#[tokio::test]
async fn session_state_all_variants() {
    assert_eq!(SessionState::Idle, SessionState::Idle);
    assert_eq!(
        SessionState::GatheringCandidates,
        SessionState::GatheringCandidates
    );
    assert_eq!(SessionState::OfferSent, SessionState::OfferSent);
    assert_eq!(SessionState::AnswerSent, SessionState::AnswerSent);
    assert_eq!(SessionState::Checking, SessionState::Checking);
    assert_eq!(SessionState::Connected, SessionState::Connected);
    assert_eq!(SessionState::Failed, SessionState::Failed);
    assert_ne!(SessionState::Idle, SessionState::Failed);
}

// --- Two sessions connect to each other ---

#[tokio::test]
async fn two_sessions_connect_via_loopback() {
    // Both sessions will gather host candidates, then we cross-wire them
    // and use STUN responders to verify connectivity.

    // Spawn two STUN responders to serve as "remote" for each session
    let (responder_a_addr, _h1) = spawn_mock_stun_server().await;
    let (responder_b_addr, _h2) = spawn_mock_stun_server().await;

    let mut session_a = P2pSession::new("B".into(), default_config());
    let mut session_b = P2pSession::new("A".into(), default_config());

    // A initiates
    session_a.initiate().await.unwrap();
    assert_eq!(session_a.state(), SessionState::OfferSent);

    // B responds with A's candidates
    // In reality B would receive A's offer, but we simulate by giving B
    // a known reachable candidate (the STUN responder for A's side)
    let remote_for_b = vec![IceCandidate::new(
        CandidateType::Host,
        responder_a_addr,
        65535,
        "host1".into(),
        1,
    )];
    session_b.respond(remote_for_b).await.unwrap();
    assert_eq!(session_b.state(), SessionState::AnswerSent);

    // A receives B's answer
    let remote_for_a = vec![IceCandidate::new(
        CandidateType::Host,
        responder_b_addr,
        65535,
        "host1".into(),
        1,
    )];
    session_a.set_remote_candidates(remote_for_a);

    // Both run checks
    session_a.run_checks().await.unwrap();
    session_b.run_checks().await.unwrap();

    assert_eq!(session_a.state(), SessionState::Connected);
    assert_eq!(session_b.state(), SessionState::Connected);
}
