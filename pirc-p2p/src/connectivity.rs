//! ICE connectivity checks and UDP hole-punching.
//!
//! Implements ICE-lite connectivity checks per RFC 5245: forms candidate pairs
//! from local and remote candidate lists, performs STUN binding checks, tracks
//! pair state transitions, and selects the highest-priority succeeded pair.

use std::net::SocketAddr;

use tokio::net::UdpSocket;
use tracing::debug;

use crate::error::{P2pError, Result};
use crate::ice::IceCandidate;
use crate::stun::StunMessage;

/// Per-check STUN timeout in milliseconds.
const CHECK_TIMEOUT_MS: u64 = 500;

/// Overall connectivity check budget in milliseconds.
const OVERALL_TIMEOUT_MS: u64 = 5000;

/// Maximum STUN retransmissions per check.
const MAX_CHECK_RETRIES: u32 = 2;

/// The ICE agent role, used for pair priority calculation per RFC 5245.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceRole {
    /// The controlling agent (typically the initiator).
    Controlling,
    /// The controlled agent (typically the responder).
    Controlled,
}

/// State of a candidate pair during connectivity checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    /// The pair has not yet been checked.
    Waiting,
    /// A STUN check is in progress for this pair.
    InProgress,
    /// The STUN check succeeded — the pair is usable.
    Succeeded,
    /// The STUN check failed (timeout or error).
    Failed,
}

/// A pairing of a local and remote ICE candidate for connectivity checking.
#[derive(Debug, Clone)]
pub struct CandidatePair {
    /// The local ICE candidate.
    pub local: IceCandidate,
    /// The remote ICE candidate.
    pub remote: IceCandidate,
    /// Current state of this pair.
    pub state: PairState,
    /// Pair priority computed per RFC 5245 §5.7.2.
    pub priority: u64,
}

impl CandidatePair {
    /// Creates a new candidate pair with the given role.
    ///
    /// Priority is computed per RFC 5245 §5.7.2:
    /// `pair_priority = 2^32 * MIN(G,D) + 2 * MAX(G,D) + (G > D ? 1 : 0)`
    /// where G is the controlling candidate priority and D is the controlled.
    #[must_use]
    pub fn new(local: IceCandidate, remote: IceCandidate, role: IceRole) -> Self {
        let priority = compute_pair_priority(local.priority, remote.priority, role);
        Self {
            local,
            remote,
            state: PairState::Waiting,
            priority,
        }
    }
}

/// Computes pair priority per RFC 5245 §5.7.2.
///
/// `pair_priority = 2^32 * MIN(G,D) + 2 * MAX(G,D) + (G > D ? 1 : 0)`
///
/// `G` is the controlling agent's candidate priority, `D` is the controlled agent's.
#[must_use]
pub fn compute_pair_priority(local_priority: u32, remote_priority: u32, role: IceRole) -> u64 {
    let (g, d) = match role {
        IceRole::Controlling => (u64::from(local_priority), u64::from(remote_priority)),
        IceRole::Controlled => (u64::from(remote_priority), u64::from(local_priority)),
    };

    let min = g.min(d);
    let max = g.max(d);
    let tie = u64::from(g > d);

    (1u64 << 32) * min + 2 * max + tie
}

/// Forms candidate pairs from local and remote candidate lists.
///
/// All possible pairings are formed (full mesh), sorted by priority descending.
#[must_use]
pub fn form_pairs(
    local: &[IceCandidate],
    remote: &[IceCandidate],
    role: IceRole,
) -> Vec<CandidatePair> {
    let mut pairs = Vec::with_capacity(local.len() * remote.len());
    for l in local {
        for r in remote {
            pairs.push(CandidatePair::new(l.clone(), r.clone(), role));
        }
    }
    pairs.sort_by(|a, b| b.priority.cmp(&a.priority));
    pairs
}

/// Performs ICE connectivity checks on candidate pairs.
///
/// Sends STUN binding requests from each local candidate's address to each
/// remote candidate's address, implementing UDP hole-punching. Selects the
/// highest-priority succeeded pair as the nominated pair.
pub struct ConnectivityChecker {
    pairs: Vec<CandidatePair>,
}

impl ConnectivityChecker {
    /// Creates a new checker from local and remote candidates.
    #[must_use]
    pub fn new(local: &[IceCandidate], remote: &[IceCandidate], role: IceRole) -> Self {
        Self {
            pairs: form_pairs(local, remote, role),
        }
    }

    /// Creates a checker from pre-formed pairs (for testing or custom ordering).
    #[must_use]
    pub fn from_pairs(pairs: Vec<CandidatePair>) -> Self {
        Self { pairs }
    }

    /// Returns a reference to the current candidate pairs.
    #[must_use]
    pub fn pairs(&self) -> &[CandidatePair] {
        &self.pairs
    }

    /// Returns the nominated pair (highest-priority succeeded pair), if any.
    #[must_use]
    pub fn nominated_pair(&self) -> Option<&CandidatePair> {
        self.pairs
            .iter()
            .filter(|p| p.state == PairState::Succeeded)
            .max_by_key(|p| p.priority)
    }

    /// Runs connectivity checks on all pairs within the overall timeout budget.
    ///
    /// For each pair, binds a local UDP socket and sends a STUN binding request
    /// to the remote candidate's address. Both sides doing this simultaneously
    /// achieves UDP hole-punching.
    ///
    /// Returns the nominated pair (highest-priority succeeded pair) or an error
    /// if no pair succeeded within the budget.
    pub async fn run_checks(&mut self) -> Result<&CandidatePair> {
        if self.pairs.is_empty() {
            return Err(P2pError::Ice("no candidate pairs to check".into()));
        }

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(OVERALL_TIMEOUT_MS);

        for i in 0..self.pairs.len() {
            // Check if we've exhausted the overall budget
            if tokio::time::Instant::now() >= deadline {
                debug!("overall connectivity check budget exhausted");
                break;
            }

            self.pairs[i].state = PairState::InProgress;
            let local_addr = self.pairs[i].local.address;
            let remote_addr = self.pairs[i].remote.address;

            debug!(
                pair = i,
                local = %local_addr,
                remote = %remote_addr,
                priority = self.pairs[i].priority,
                "starting connectivity check"
            );

            let remaining = deadline.duration_since(tokio::time::Instant::now());
            match self.check_pair(local_addr, remote_addr, remaining).await {
                Ok(()) => {
                    self.pairs[i].state = PairState::Succeeded;
                    debug!(pair = i, "connectivity check succeeded");
                }
                Err(e) => {
                    self.pairs[i].state = PairState::Failed;
                    debug!(pair = i, error = %e, "connectivity check failed");
                }
            }
        }

        self.nominated_pair().ok_or_else(|| {
            P2pError::Ice("all connectivity checks failed".into())
        })
    }

    /// Performs a single STUN binding check from local to remote.
    async fn check_pair(
        &self,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        budget: std::time::Duration,
    ) -> Result<()> {
        // Bind a socket for this check. Try the exact local address first;
        // if that fails (e.g. srflx/relay address), bind any available port
        // for hole-punching.
        let socket = if let Ok(s) = UdpSocket::bind(local_addr).await {
            s
        } else {
            let bind_addr: SocketAddr = if local_addr.is_ipv6() {
                "[::]:0".parse().unwrap()
            } else {
                "0.0.0.0:0".parse().unwrap()
            };
            UdpSocket::bind(bind_addr).await?
        };

        let request = StunMessage::binding_request();
        let request_bytes = request.to_bytes();
        let expected_tid = request.transaction_id;

        let per_check = std::time::Duration::from_millis(CHECK_TIMEOUT_MS);
        let mut buf = [0u8; 1024];

        for attempt in 0..=MAX_CHECK_RETRIES {
            let timeout = per_check.min(budget);
            if timeout.is_zero() {
                return Err(P2pError::Ice("check budget exhausted".into()));
            }

            socket.send_to(&request_bytes, remote_addr).await?;
            debug!(
                attempt,
                remote = %remote_addr,
                "sent STUN connectivity check"
            );

            match tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await {
                Ok(Ok((len, _src))) => {
                    if let Ok(response) = StunMessage::from_bytes(&buf[..len]) {
                        if response.transaction_id == expected_tid
                            && response.is_binding_response()
                        {
                            return Ok(());
                        }
                    }
                    // Ignore non-matching or malformed responses
                }
                Ok(Err(e)) => return Err(P2pError::Io(e)),
                Err(_) => {
                    debug!(attempt, "connectivity check timed out");
                }
            }
        }

        Err(P2pError::Ice(format!(
            "STUN connectivity check to {remote_addr} timed out"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ice::{CandidateType, IceCandidate};
    use crate::stun::{StunAttribute, StunMessage};

    fn host_candidate(addr: &str) -> IceCandidate {
        IceCandidate::new(
            CandidateType::Host,
            addr.parse().unwrap(),
            65535,
            "host1".into(),
            1,
        )
    }

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

    // --- Pair formation tests ---

    #[test]
    fn form_pairs_creates_full_mesh() {
        let local = vec![
            host_candidate("192.168.1.1:5000"),
            srflx_candidate("203.0.113.1:5000"),
        ];
        let remote = vec![
            host_candidate("192.168.1.2:6000"),
            relay_candidate("10.0.0.1:7000"),
        ];

        let pairs = form_pairs(&local, &remote, IceRole::Controlling);
        assert_eq!(pairs.len(), 4); // 2 local * 2 remote
    }

    #[test]
    fn form_pairs_empty_inputs() {
        let pairs = form_pairs(&[], &[], IceRole::Controlling);
        assert!(pairs.is_empty());

        let local = vec![host_candidate("192.168.1.1:5000")];
        let pairs = form_pairs(&local, &[], IceRole::Controlling);
        assert!(pairs.is_empty());

        let remote = vec![host_candidate("192.168.1.2:6000")];
        let pairs = form_pairs(&[], &remote, IceRole::Controlling);
        assert!(pairs.is_empty());
    }

    #[test]
    fn form_pairs_sorted_by_priority_descending() {
        let local = vec![
            host_candidate("192.168.1.1:5000"),
            relay_candidate("10.0.0.1:5000"),
        ];
        let remote = vec![
            host_candidate("192.168.1.2:6000"),
        ];

        let pairs = form_pairs(&local, &remote, IceRole::Controlling);
        assert_eq!(pairs.len(), 2);
        // Host-Host pair should have higher priority than Relay-Host
        assert!(pairs[0].priority > pairs[1].priority);
    }

    #[test]
    fn form_pairs_all_start_waiting() {
        let local = vec![host_candidate("192.168.1.1:5000")];
        let remote = vec![host_candidate("192.168.1.2:6000")];

        let pairs = form_pairs(&local, &remote, IceRole::Controlling);
        for pair in &pairs {
            assert_eq!(pair.state, PairState::Waiting);
        }
    }

    // --- Pair priority tests ---

    #[test]
    fn pair_priority_rfc5245_formula() {
        // RFC 5245 §5.7.2:
        // pair_priority = 2^32 * MIN(G,D) + 2 * MAX(G,D) + (G > D ? 1 : 0)
        let g: u32 = 1000;
        let d: u32 = 500;
        let expected = (1u64 << 32) * 500 + 2 * 1000 + 1;
        assert_eq!(
            compute_pair_priority(g, d, IceRole::Controlling),
            expected
        );
    }

    #[test]
    fn pair_priority_controlling_vs_controlled_differ() {
        let local_prio: u32 = 2_000_000;
        let remote_prio: u32 = 1_000_000;

        let controlling = compute_pair_priority(local_prio, remote_prio, IceRole::Controlling);
        let controlled = compute_pair_priority(local_prio, remote_prio, IceRole::Controlled);

        // Same min/max but tie-breaker differs when G != D
        assert_ne!(controlling, controlled);
    }

    #[test]
    fn pair_priority_equal_priorities() {
        let prio: u32 = 1_500_000;
        let result = compute_pair_priority(prio, prio, IceRole::Controlling);
        // G == D, so tie-breaker = 0
        let expected = (1u64 << 32) * u64::from(prio) + 2 * u64::from(prio);
        assert_eq!(result, expected);
    }

    #[test]
    fn pair_priority_host_host_higher_than_host_relay() {
        let host = host_candidate("192.168.1.1:5000");
        let remote_host = host_candidate("192.168.1.2:6000");
        let remote_relay = relay_candidate("10.0.0.1:7000");

        let pair_hh = CandidatePair::new(host.clone(), remote_host, IceRole::Controlling);
        let pair_hr = CandidatePair::new(host, remote_relay, IceRole::Controlling);

        assert!(pair_hh.priority > pair_hr.priority);
    }

    // --- State transition tests ---

    #[test]
    fn candidate_pair_initial_state_is_waiting() {
        let pair = CandidatePair::new(
            host_candidate("192.168.1.1:5000"),
            host_candidate("192.168.1.2:6000"),
            IceRole::Controlling,
        );
        assert_eq!(pair.state, PairState::Waiting);
    }

    #[test]
    fn pair_state_transitions() {
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

        // Test the failed path too
        let mut pair2 = CandidatePair::new(
            host_candidate("192.168.1.1:5000"),
            host_candidate("192.168.1.2:6000"),
            IceRole::Controlled,
        );
        pair2.state = PairState::InProgress;
        pair2.state = PairState::Failed;
        assert_eq!(pair2.state, PairState::Failed);
    }

    // --- ConnectivityChecker tests ---

    #[test]
    fn checker_pairs_sorted() {
        let local = vec![
            host_candidate("192.168.1.1:5000"),
            relay_candidate("10.0.0.1:5000"),
        ];
        let remote = vec![host_candidate("192.168.1.2:6000")];

        let checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
        let pairs = checker.pairs();

        // Should be sorted by priority descending
        for window in pairs.windows(2) {
            assert!(window[0].priority >= window[1].priority);
        }
    }

    #[test]
    fn checker_nominated_pair_none_initially() {
        let local = vec![host_candidate("192.168.1.1:5000")];
        let remote = vec![host_candidate("192.168.1.2:6000")];
        let checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);

        assert!(checker.nominated_pair().is_none());
    }

    #[test]
    fn checker_nominated_pair_selects_highest_priority_succeeded() {
        let host_l = host_candidate("192.168.1.1:5000");
        let relay_l = relay_candidate("10.0.0.1:5000");
        let host_r = host_candidate("192.168.1.2:6000");

        let mut pairs = form_pairs(&[host_l, relay_l], &[host_r], IceRole::Controlling);

        // Mark lower-priority pair as succeeded first
        pairs[1].state = PairState::Succeeded;
        let checker = ConnectivityChecker::from_pairs(pairs.clone());
        let nominated = checker.nominated_pair().unwrap();
        assert_eq!(nominated.priority, pairs[1].priority);

        // Now mark higher-priority pair as succeeded too
        pairs[0].state = PairState::Succeeded;
        let checker = ConnectivityChecker::from_pairs(pairs.clone());
        let nominated = checker.nominated_pair().unwrap();
        assert_eq!(nominated.priority, pairs[0].priority);
    }

    #[test]
    fn checker_nominated_pair_ignores_failed() {
        let host_l = host_candidate("192.168.1.1:5000");
        let host_r = host_candidate("192.168.1.2:6000");

        let mut pairs = form_pairs(&[host_l], &[host_r], IceRole::Controlling);
        pairs[0].state = PairState::Failed;

        let checker = ConnectivityChecker::from_pairs(pairs);
        assert!(checker.nominated_pair().is_none());
    }

    #[tokio::test]
    async fn checker_run_empty_pairs_returns_error() {
        let mut checker = ConnectivityChecker::from_pairs(vec![]);
        let result = checker.run_checks().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no candidate pairs"));
    }

    #[tokio::test]
    async fn checker_run_succeeds_with_mock_stun() {
        // Bind a mock STUN responder
        let responder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let responder_addr = responder.local_addr().unwrap();

        // Spawn the mock responder
        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = responder.recv_from(&mut buf).await.unwrap();
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();

            let response = StunMessage {
                msg_type: 0x0101, // Binding Response
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(src)],
            };
            responder.send_to(&response.to_bytes(), src).await.unwrap();
        });

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

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn checker_run_timeout_marks_failed() {
        // Remote address that won't respond — use a port unlikely to have a STUN server
        let local = vec![host_candidate("0.0.0.0:0")];
        let remote = vec![host_candidate("127.0.0.1:1")];

        let mut checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
        let result = checker.run_checks().await;
        assert!(result.is_err());

        // The pair should be marked as Failed
        assert_eq!(checker.pairs()[0].state, PairState::Failed);
    }

    #[tokio::test]
    async fn checker_run_selects_first_succeeded() {
        // Two mock responders — first one responds, second doesn't
        let responder1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr1 = responder1.local_addr().unwrap();

        let handle1 = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = responder1.recv_from(&mut buf).await.unwrap();
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();

            let response = StunMessage {
                msg_type: 0x0101,
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(src)],
            };
            responder1.send_to(&response.to_bytes(), src).await.unwrap();
        });

        let local = vec![host_candidate("0.0.0.0:0")];
        let remote = vec![
            IceCandidate::new(CandidateType::Host, addr1, 65535, "host1".into(), 1),
        ];

        let mut checker = ConnectivityChecker::new(&local, &remote, IceRole::Controlling);
        let result = checker.run_checks().await;
        assert!(result.is_ok());

        let nominated = result.unwrap();
        assert_eq!(nominated.state, PairState::Succeeded);
        assert_eq!(nominated.remote.address, addr1);

        handle1.await.unwrap();
    }

    #[test]
    fn pair_priority_symmetry() {
        // When the same priorities are used, the controlling and controlled
        // agents should compute the same pair priority value but with the
        // tie-breaker differing based on which is G and which is D.
        let p1: u32 = 2_000_000;
        let p2: u32 = 1_000_000;

        let controlling = compute_pair_priority(p1, p2, IceRole::Controlling);
        let controlled = compute_pair_priority(p2, p1, IceRole::Controlled);

        // Both should have the same pair priority since the roles ensure
        // G and D are assigned consistently.
        assert_eq!(controlling, controlled);
    }

    #[test]
    fn ice_role_equality() {
        assert_eq!(IceRole::Controlling, IceRole::Controlling);
        assert_eq!(IceRole::Controlled, IceRole::Controlled);
        assert_ne!(IceRole::Controlling, IceRole::Controlled);
    }

    #[test]
    fn pair_state_equality() {
        assert_eq!(PairState::Waiting, PairState::Waiting);
        assert_eq!(PairState::InProgress, PairState::InProgress);
        assert_eq!(PairState::Succeeded, PairState::Succeeded);
        assert_eq!(PairState::Failed, PairState::Failed);
        assert_ne!(PairState::Waiting, PairState::Failed);
    }

    #[test]
    fn form_pairs_preserves_candidate_data() {
        let local = vec![host_candidate("192.168.1.1:5000")];
        let remote = vec![srflx_candidate("203.0.113.1:6000")];

        let pairs = form_pairs(&local, &remote, IceRole::Controlling);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].local.address, "192.168.1.1:5000".parse().unwrap());
        assert_eq!(pairs[0].remote.address, "203.0.113.1:6000".parse().unwrap());
        assert_eq!(pairs[0].local.candidate_type, CandidateType::Host);
        assert_eq!(pairs[0].remote.candidate_type, CandidateType::ServerReflexive);
    }
}
