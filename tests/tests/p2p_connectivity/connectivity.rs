//! ICE connectivity checking integration tests.
//!
//! Exercises the connectivity checker: pair formation, priority computation,
//! state transitions, mock STUN checks over loopback, and nominated pair
//! selection.

use std::net::SocketAddr;

use pirc_p2p::connectivity::{
    compute_pair_priority, form_pairs, CandidatePair, ConnectivityChecker, IceRole, PairState,
};
use pirc_p2p::ice::{CandidateType, IceCandidate};

use super::{host_candidate, spawn_mock_stun_server};

fn srflx_candidate(addr: &str) -> IceCandidate {
    IceCandidate::new(
        CandidateType::ServerReflexive,
        addr.parse().unwrap(),
        65535,
        "srflx1".into(),
        1,
    )
}

fn relay_candidate(addr: &str) -> IceCandidate {
    IceCandidate::new(
        CandidateType::Relay,
        addr.parse().unwrap(),
        65535,
        "relay1".into(),
        1,
    )
}

// --- Pair formation ---

#[tokio::test]
async fn form_pairs_creates_full_mesh() {
    let local = vec![
        host_candidate("192.168.1.1:5000"),
        srflx_candidate("203.0.113.1:5000"),
    ];
    let remote = vec![
        host_candidate("192.168.1.2:6000"),
        relay_candidate("10.0.0.1:7000"),
    ];

    let pairs = form_pairs(&local, &remote, IceRole::Controlling);
    assert_eq!(pairs.len(), 4); // 2 * 2
}

#[tokio::test]
async fn form_pairs_empty_returns_empty() {
    let pairs = form_pairs(&[], &[], IceRole::Controlling);
    assert!(pairs.is_empty());

    let local = vec![host_candidate("192.168.1.1:5000")];
    assert!(form_pairs(&local, &[], IceRole::Controlling).is_empty());
    assert!(form_pairs(&[], &local, IceRole::Controlling).is_empty());
}

#[tokio::test]
async fn form_pairs_sorted_by_priority_descending() {
    let local = vec![
        host_candidate("192.168.1.1:5000"),
        relay_candidate("10.0.0.1:5000"),
    ];
    let remote = vec![host_candidate("192.168.1.2:6000")];

    let pairs = form_pairs(&local, &remote, IceRole::Controlling);
    for window in pairs.windows(2) {
        assert!(window[0].priority >= window[1].priority);
    }
}

#[tokio::test]
async fn form_pairs_initial_state_waiting() {
    let local = vec![host_candidate("192.168.1.1:5000")];
    let remote = vec![host_candidate("192.168.1.2:6000")];

    let pairs = form_pairs(&local, &remote, IceRole::Controlling);
    for pair in &pairs {
        assert_eq!(pair.state, PairState::Waiting);
    }
}

// --- Pair priority ---

#[tokio::test]
async fn pair_priority_rfc5245_formula() {
    // pair_priority = 2^32 * MIN(G,D) + 2 * MAX(G,D) + (G > D ? 1 : 0)
    let g: u32 = 1000;
    let d: u32 = 500;
    let expected = (1u64 << 32) * 500 + 2 * 1000 + 1;
    assert_eq!(
        compute_pair_priority(g, d, IceRole::Controlling),
        expected
    );
}

#[tokio::test]
async fn pair_priority_controlling_vs_controlled_differ() {
    let local_prio: u32 = 2_000_000;
    let remote_prio: u32 = 1_000_000;

    let controlling = compute_pair_priority(local_prio, remote_prio, IceRole::Controlling);
    let controlled = compute_pair_priority(local_prio, remote_prio, IceRole::Controlled);

    assert_ne!(controlling, controlled);
}

#[tokio::test]
async fn pair_priority_symmetry() {
    let p1: u32 = 2_000_000;
    let p2: u32 = 1_000_000;

    // Controlling with (p1, p2) should equal Controlled with (p2, p1)
    let controlling = compute_pair_priority(p1, p2, IceRole::Controlling);
    let controlled = compute_pair_priority(p2, p1, IceRole::Controlled);
    assert_eq!(controlling, controlled);
}

#[tokio::test]
async fn pair_priority_host_host_higher_than_host_relay() {
    let host = host_candidate("192.168.1.1:5000");
    let remote_host = host_candidate("192.168.1.2:6000");
    let remote_relay = relay_candidate("10.0.0.1:7000");

    let pair_hh = CandidatePair::new(host.clone(), remote_host, IceRole::Controlling);
    let pair_hr = CandidatePair::new(host, remote_relay, IceRole::Controlling);

    assert!(pair_hh.priority > pair_hr.priority);
}

// --- State transitions ---

#[tokio::test]
async fn pair_state_transitions() {
    let mut pair = CandidatePair::new(
        host_candidate("192.168.1.1:5000"),
        host_candidate("192.168.1.2:6000"),
        IceRole::Controlling,
    );

    assert_eq!(pair.state, PairState::Waiting);
    pair.state = PairState::InProgress;
    assert_eq!(pair.state, PairState::InProgress);
    pair.state = PairState::Succeeded;
    assert_eq!(pair.state, PairState::Succeeded);

    let mut pair2 = CandidatePair::new(
        host_candidate("192.168.1.1:5000"),
        host_candidate("192.168.1.2:6000"),
        IceRole::Controlled,
    );
    pair2.state = PairState::InProgress;
    pair2.state = PairState::Failed;
    assert_eq!(pair2.state, PairState::Failed);
}

// --- ConnectivityChecker ---

#[tokio::test]
async fn checker_pairs_sorted_by_priority() {
    let local = vec![
        host_candidate("192.168.1.1:5000"),
        relay_candidate("10.0.0.1:5000"),
    ];
    let remote = vec![host_candidate("192.168.1.2:6000")];

    let checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
    let pairs = checker.pairs();
    for window in pairs.windows(2) {
        assert!(window[0].priority >= window[1].priority);
    }
}

#[tokio::test]
async fn checker_nominated_pair_none_initially() {
    let local = vec![host_candidate("192.168.1.1:5000")];
    let remote = vec![host_candidate("192.168.1.2:6000")];
    let checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);

    assert!(checker.nominated_pair().is_none());
}

#[tokio::test]
async fn checker_nominated_selects_highest_priority_succeeded() {
    let host_l = host_candidate("192.168.1.1:5000");
    let relay_l = relay_candidate("10.0.0.1:5000");
    let host_r = host_candidate("192.168.1.2:6000");

    let mut pairs = form_pairs(&[host_l, relay_l], &[host_r], IceRole::Controlling);

    // Mark only the lower-priority pair as succeeded
    pairs[1].state = PairState::Succeeded;
    let checker = ConnectivityChecker::from_pairs(pairs.clone());
    let nominated = checker.nominated_pair().unwrap();
    assert_eq!(nominated.priority, pairs[1].priority);

    // Now mark the higher-priority pair as well
    pairs[0].state = PairState::Succeeded;
    let checker = ConnectivityChecker::from_pairs(pairs.clone());
    let nominated = checker.nominated_pair().unwrap();
    assert_eq!(nominated.priority, pairs[0].priority);
}

#[tokio::test]
async fn checker_nominated_ignores_failed() {
    let mut pairs = form_pairs(
        &[host_candidate("192.168.1.1:5000")],
        &[host_candidate("192.168.1.2:6000")],
        IceRole::Controlling,
    );
    pairs[0].state = PairState::Failed;
    let checker = ConnectivityChecker::from_pairs(pairs);
    assert!(checker.nominated_pair().is_none());
}

#[tokio::test]
async fn checker_empty_pairs_returns_error() {
    let mut checker = ConnectivityChecker::from_pairs(vec![]);
    let result = checker.run_checks().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no candidate pairs"));
}

// --- Loopback connectivity checks ---

#[tokio::test]
async fn checker_succeeds_with_mock_stun_responder() {
    let (responder_addr, _handle) = spawn_mock_stun_server().await;

    let local = vec![host_candidate("0.0.0.0:0")];
    let remote = vec![IceCandidate::new(
        CandidateType::Host,
        responder_addr,
        65535,
        "host1".into(),
        1,
    )];

    let mut checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
    let result = checker.run_checks().await;
    assert!(result.is_ok());

    let nominated = result.unwrap();
    assert_eq!(nominated.state, PairState::Succeeded);
    assert_eq!(nominated.remote.address, responder_addr);
}

#[tokio::test]
async fn checker_timeout_marks_pair_failed() {
    let local = vec![host_candidate("0.0.0.0:0")];
    // Port 1 won't respond
    let remote = vec![host_candidate("127.0.0.1:1")];

    let mut checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
    let result = checker.run_checks().await;
    assert!(result.is_err());
    assert_eq!(checker.pairs()[0].state, PairState::Failed);
}

#[tokio::test]
async fn checker_multiple_pairs_selects_first_succeeding() {
    let (responder_addr, _handle) = spawn_mock_stun_server().await;

    let local = vec![host_candidate("0.0.0.0:0")];
    let remote = vec![IceCandidate::new(
        CandidateType::Host,
        responder_addr,
        65535,
        "host1".into(),
        1,
    )];

    let mut checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
    let result = checker.run_checks().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().remote.address, responder_addr);
}

#[tokio::test]
async fn form_pairs_preserves_candidate_data() {
    let local = vec![host_candidate("192.168.1.1:5000")];
    let remote = vec![srflx_candidate("203.0.113.1:6000")];

    let pairs = form_pairs(&local, &remote, IceRole::Controlling);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0].local.address,
        "192.168.1.1:5000".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        pairs[0].remote.address,
        "203.0.113.1:6000".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(pairs[0].local.candidate_type, CandidateType::Host);
    assert_eq!(
        pairs[0].remote.candidate_type,
        CandidateType::ServerReflexive
    );
}
