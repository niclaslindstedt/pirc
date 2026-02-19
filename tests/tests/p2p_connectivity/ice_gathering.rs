//! ICE candidate gathering and prioritization integration tests.
//!
//! Exercises the ICE candidate API: priority computation, SDP serialization,
//! candidate type ordering, and the gatherer with mock STUN.

use pirc_p2p::ice::{
    compute_priority, CandidateGatherer, CandidateType, GathererConfig, IceCandidate,
};

use super::spawn_mock_stun_server;

// --- Priority computation ---

#[tokio::test]
async fn host_priority_higher_than_srflx_higher_than_relay() {
    let host = compute_priority(CandidateType::Host, 65535, 1);
    let srflx = compute_priority(CandidateType::ServerReflexive, 65535, 1);
    let relay = compute_priority(CandidateType::Relay, 65535, 1);

    assert!(host > srflx);
    assert!(srflx > relay);
}

#[tokio::test]
async fn priority_matches_rfc5245_formula() {
    // RFC 5245 §4.1.2.1:
    // priority = (2^24) * type_preference + (2^8) * local_preference + (256 - component_id)
    let prio = compute_priority(CandidateType::Host, 65535, 1);
    let expected = (126 << 24) + (65535 << 8) + (256 - 1);
    assert_eq!(prio, expected);

    let prio = compute_priority(CandidateType::ServerReflexive, 1000, 2);
    let expected = (100 << 24) + (1000 << 8) + (256 - 2);
    assert_eq!(prio, expected);

    let prio = compute_priority(CandidateType::Relay, 0, 1);
    let expected = 256 - 1;
    assert_eq!(prio, expected);
}

#[tokio::test]
async fn priority_component_id_tiebreak() {
    let prio1 = compute_priority(CandidateType::Host, 65535, 1);
    let prio2 = compute_priority(CandidateType::Host, 65535, 2);
    assert!(prio1 > prio2);
    assert_eq!(prio1 - prio2, 1);
}

// --- SDP serialization ---

#[tokio::test]
async fn sdp_roundtrip_host() {
    let candidate = IceCandidate::new(
        CandidateType::Host,
        "192.168.1.100:5000".parse().unwrap(),
        65535,
        "host1".to_string(),
        1,
    );

    let sdp = candidate.to_sdp_string();
    let parsed = IceCandidate::from_sdp_string(&sdp).unwrap();

    assert_eq!(parsed.foundation, "host1");
    assert_eq!(parsed.component, 1);
    assert_eq!(parsed.priority, candidate.priority);
    assert_eq!(parsed.address, candidate.address);
    assert_eq!(parsed.candidate_type, CandidateType::Host);
}

#[tokio::test]
async fn sdp_roundtrip_srflx() {
    let candidate = IceCandidate::new(
        CandidateType::ServerReflexive,
        "203.0.113.42:5060".parse().unwrap(),
        65535,
        "srflx1".to_string(),
        1,
    );

    let sdp = candidate.to_sdp_string();
    assert!(sdp.contains("typ srflx"));

    let parsed = IceCandidate::from_sdp_string(&sdp).unwrap();
    assert_eq!(parsed.candidate_type, CandidateType::ServerReflexive);
    assert_eq!(parsed.address, candidate.address);
}

#[tokio::test]
async fn sdp_roundtrip_relay() {
    let candidate = IceCandidate::new(
        CandidateType::Relay,
        "10.0.0.1:49152".parse().unwrap(),
        65535,
        "relay1".to_string(),
        1,
    );

    let sdp = candidate.to_sdp_string();
    assert!(sdp.contains("typ relay"));

    let parsed = IceCandidate::from_sdp_string(&sdp).unwrap();
    assert_eq!(parsed.candidate_type, CandidateType::Relay);
}

#[tokio::test]
async fn sdp_roundtrip_ipv6() {
    let candidate = IceCandidate::new(
        CandidateType::Host,
        "[::1]:8080".parse().unwrap(),
        65535,
        "host2".to_string(),
        1,
    );

    let sdp = candidate.to_sdp_string();
    let parsed = IceCandidate::from_sdp_string(&sdp).unwrap();
    assert_eq!(parsed.address, candidate.address);
}

#[tokio::test]
async fn sdp_parse_rejects_invalid_input() {
    assert!(IceCandidate::from_sdp_string("too few fields").is_err());
    assert!(IceCandidate::from_sdp_string("f1 1 tcp 100 1.2.3.4 5000 typ host").is_err());
    assert!(IceCandidate::from_sdp_string("f1 1 udp 100 1.2.3.4 5000 foo host").is_err());
    assert!(IceCandidate::from_sdp_string("f1 1 udp 100 1.2.3.4 5000 typ prflx").is_err());
}

// --- Candidate type display/parse ---

#[tokio::test]
async fn candidate_type_display_and_parse() {
    assert_eq!(CandidateType::Host.to_string(), "host");
    assert_eq!(CandidateType::ServerReflexive.to_string(), "srflx");
    assert_eq!(CandidateType::Relay.to_string(), "relay");

    assert_eq!("host".parse::<CandidateType>().unwrap(), CandidateType::Host);
    assert_eq!(
        "srflx".parse::<CandidateType>().unwrap(),
        CandidateType::ServerReflexive
    );
    assert_eq!(
        "relay".parse::<CandidateType>().unwrap(),
        CandidateType::Relay
    );
}

// --- Candidates sort by priority ---

#[tokio::test]
async fn candidates_sort_host_first() {
    let host = IceCandidate::new(
        CandidateType::Host,
        "192.168.1.1:5000".parse().unwrap(),
        65535,
        "host1".into(),
        1,
    );
    let srflx = IceCandidate::new(
        CandidateType::ServerReflexive,
        "203.0.113.1:5000".parse().unwrap(),
        65535,
        "srflx1".into(),
        1,
    );
    let relay = IceCandidate::new(
        CandidateType::Relay,
        "10.0.0.1:5000".parse().unwrap(),
        65535,
        "relay1".into(),
        1,
    );

    let mut candidates = vec![relay, host, srflx];
    candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

    assert_eq!(candidates[0].candidate_type, CandidateType::Host);
    assert_eq!(candidates[1].candidate_type, CandidateType::ServerReflexive);
    assert_eq!(candidates[2].candidate_type, CandidateType::Relay);
}

// --- CandidateGatherer ---

#[tokio::test]
async fn gatherer_collects_host_candidate() {
    let config = GathererConfig {
        stun_server: None,
        turn_server: None,
        turn_username: None,
        turn_password: None,
    };
    let gatherer = CandidateGatherer::new(config);
    let candidates = gatherer.gather().await.unwrap();

    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].candidate_type, CandidateType::Host);
    assert_eq!(candidates[0].component, 1);
    assert_eq!(candidates[0].foundation, "host1");
}

#[tokio::test]
async fn gatherer_with_stun_gets_host_and_srflx() {
    let (stun_addr, _handle) = spawn_mock_stun_server().await;

    let config = GathererConfig {
        stun_server: Some(stun_addr),
        turn_server: None,
        turn_username: None,
        turn_password: None,
    };
    let gatherer = CandidateGatherer::new(config);
    let candidates = gatherer.gather().await.unwrap();

    // Should have host + srflx
    assert_eq!(candidates.len(), 2);

    // Sorted by priority: host first
    assert_eq!(candidates[0].candidate_type, CandidateType::Host);
    assert_eq!(candidates[1].candidate_type, CandidateType::ServerReflexive);
}

#[tokio::test]
async fn gatherer_results_sorted_by_priority_descending() {
    let (stun_addr, _handle) = spawn_mock_stun_server().await;

    let config = GathererConfig {
        stun_server: Some(stun_addr),
        turn_server: None,
        turn_username: None,
        turn_password: None,
    };
    let gatherer = CandidateGatherer::new(config);
    let candidates = gatherer.gather().await.unwrap();

    for window in candidates.windows(2) {
        assert!(
            window[0].priority >= window[1].priority,
            "candidates not sorted: {} < {}",
            window[0].priority,
            window[1].priority,
        );
    }
}

#[tokio::test]
async fn gatherer_sdp_candidates_parseable() {
    let config = GathererConfig {
        stun_server: None,
        turn_server: None,
        turn_username: None,
        turn_password: None,
    };
    let gatherer = CandidateGatherer::new(config);
    let candidates = gatherer.gather().await.unwrap();

    for candidate in &candidates {
        let sdp = candidate.to_sdp_string();
        let parsed = IceCandidate::from_sdp_string(&sdp);
        assert!(parsed.is_ok(), "failed to parse SDP: {sdp}");
    }
}
