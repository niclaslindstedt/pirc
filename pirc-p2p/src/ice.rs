//! ICE-lite candidate gathering (RFC 5245).
//!
//! Defines ICE candidate types, priority calculation, serialization for
//! signaling exchange, and a [`CandidateGatherer`] that collects host,
//! server-reflexive (STUN), and relay (TURN) candidates.

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use tokio::net::UdpSocket;
use tracing::{debug, warn};

use crate::error::{P2pError, Result};
use crate::stun::discover_reflexive_address;
use crate::turn::{allocate, Allocation};

// --- RFC 5245 §4.1.2.1 type preferences ---

/// Type preference for host candidates (highest priority).
const TYPE_PREF_HOST: u32 = 126;

/// Type preference for server-reflexive candidates.
const TYPE_PREF_SRFLX: u32 = 100;

/// Type preference for relay candidates (lowest priority).
const TYPE_PREF_RELAY: u32 = 0;

/// ICE component ID for RTP (the only component we use).
const COMPONENT_RTP: u16 = 1;

/// The type of an ICE candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateType {
    /// A host candidate discovered from a local network interface.
    Host,
    /// A server-reflexive candidate obtained via STUN.
    ServerReflexive,
    /// A relay candidate obtained via TURN.
    Relay,
}

impl CandidateType {
    /// Returns the RFC 5245 type preference value.
    #[must_use]
    fn type_preference(self) -> u32 {
        match self {
            Self::Host => TYPE_PREF_HOST,
            Self::ServerReflexive => TYPE_PREF_SRFLX,
            Self::Relay => TYPE_PREF_RELAY,
        }
    }
}

impl fmt::Display for CandidateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host => write!(f, "host"),
            Self::ServerReflexive => write!(f, "srflx"),
            Self::Relay => write!(f, "relay"),
        }
    }
}

impl FromStr for CandidateType {
    type Err = P2pError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "host" => Ok(Self::Host),
            "srflx" => Ok(Self::ServerReflexive),
            "relay" => Ok(Self::Relay),
            other => Err(P2pError::Ice(format!("unknown candidate type: {other}"))),
        }
    }
}

/// An ICE candidate for connectivity checks and media transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceCandidate {
    /// The candidate type.
    pub candidate_type: CandidateType,
    /// The transport address (IP + port).
    pub address: SocketAddr,
    /// Priority value computed per RFC 5245 §4.1.2.1.
    pub priority: u32,
    /// Foundation string — same for candidates sharing type, base address, and protocol.
    pub foundation: String,
    /// Component ID (1 for RTP).
    pub component: u16,
}

impl IceCandidate {
    /// Creates a new ICE candidate with priority computed per RFC 5245.
    ///
    /// `local_preference` is a value 0–65535 used to distinguish candidates
    /// of the same type (e.g. multiple host interfaces).
    #[must_use]
    pub fn new(
        candidate_type: CandidateType,
        address: SocketAddr,
        local_preference: u16,
        foundation: String,
        component: u16,
    ) -> Self {
        let priority = compute_priority(candidate_type, local_preference, component);
        Self {
            candidate_type,
            address,
            priority,
            foundation,
            component,
        }
    }

    /// Serializes this candidate to a string suitable for signaling exchange.
    ///
    /// Format: `<foundation> <component> udp <priority> <addr> <port> typ <type>`
    #[must_use]
    pub fn to_sdp_string(&self) -> String {
        format!(
            "{} {} udp {} {} {} typ {}",
            self.foundation,
            self.component,
            self.priority,
            self.address.ip(),
            self.address.port(),
            self.candidate_type,
        )
    }

    /// Parses a candidate from the signaling string format.
    ///
    /// Expected format: `<foundation> <component> udp <priority> <addr> <port> typ <type>`
    pub fn from_sdp_string(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 8 {
            return Err(P2pError::Ice(format!(
                "invalid candidate string: expected 8 fields, got {}",
                parts.len()
            )));
        }

        if parts[2] != "udp" {
            return Err(P2pError::Ice(format!(
                "unsupported transport: {} (expected udp)",
                parts[2]
            )));
        }

        if parts[6] != "typ" {
            return Err(P2pError::Ice(format!(
                "expected 'typ' keyword at position 6, got '{}'",
                parts[6]
            )));
        }

        let foundation = parts[0].to_string();
        let component: u16 = parts[1]
            .parse()
            .map_err(|e| P2pError::Ice(format!("invalid component: {e}")))?;
        let priority: u32 = parts[3]
            .parse()
            .map_err(|e| P2pError::Ice(format!("invalid priority: {e}")))?;
        let ip: std::net::IpAddr = parts[4]
            .parse()
            .map_err(|e| P2pError::Ice(format!("invalid IP address: {e}")))?;
        let port: u16 = parts[5]
            .parse()
            .map_err(|e| P2pError::Ice(format!("invalid port: {e}")))?;
        let candidate_type: CandidateType = parts[7].parse()?;

        let address = SocketAddr::new(ip, port);

        Ok(Self {
            candidate_type,
            address,
            priority,
            foundation,
            component,
        })
    }
}

/// Computes candidate priority per RFC 5245 §4.1.2.1:
///
/// `priority = (2^24) * type_preference + (2^8) * local_preference + (256 - component_id)`
#[must_use]
pub fn compute_priority(candidate_type: CandidateType, local_preference: u16, component: u16) -> u32 {
    let type_pref = candidate_type.type_preference();
    let local_pref = u32::from(local_preference);
    let comp = u32::from(component);

    (type_pref << 24) + (local_pref << 8) + (256 - comp)
}

/// Configuration for the [`CandidateGatherer`].
pub struct GathererConfig {
    /// STUN server address for server-reflexive candidate discovery.
    pub stun_server: Option<SocketAddr>,
    /// TURN server address for relay candidate allocation.
    pub turn_server: Option<SocketAddr>,
    /// TURN username (required if `turn_server` is set).
    pub turn_username: Option<String>,
    /// TURN password (required if `turn_server` is set).
    pub turn_password: Option<String>,
}

/// Gathers ICE candidates from local interfaces, STUN, and TURN.
pub struct CandidateGatherer {
    config: GathererConfig,
}

impl CandidateGatherer {
    /// Creates a new gatherer with the given configuration.
    #[must_use]
    pub fn new(config: GathererConfig) -> Self {
        Self { config }
    }

    /// Gathers all available ICE candidates.
    ///
    /// 1. Binds a UDP socket to discover the host candidate.
    /// 2. Queries the STUN server (if configured) for a server-reflexive candidate.
    /// 3. Queries the TURN server (if configured) for a relay candidate.
    ///
    /// Returns candidates sorted by priority (highest first).
    pub async fn gather(&self) -> Result<Vec<IceCandidate>> {
        let mut candidates = Vec::new();

        // Bind a local UDP socket for candidate gathering
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let local_addr = socket.local_addr()?;

        // Host candidate
        let host = IceCandidate::new(
            CandidateType::Host,
            local_addr,
            65535, // highest local preference for our single host candidate
            "host1".to_string(),
            COMPONENT_RTP,
        );
        debug!(addr = %local_addr, priority = host.priority, "gathered host candidate");
        candidates.push(host);

        // Server-reflexive candidate via STUN
        if let Some(stun_server) = self.config.stun_server {
            match discover_reflexive_address(&socket, stun_server).await {
                Ok(srflx_addr) => {
                    let srflx = IceCandidate::new(
                        CandidateType::ServerReflexive,
                        srflx_addr,
                        65535,
                        "srflx1".to_string(),
                        COMPONENT_RTP,
                    );
                    debug!(addr = %srflx_addr, priority = srflx.priority, "gathered srflx candidate");
                    candidates.push(srflx);
                }
                Err(e) => {
                    warn!(error = %e, "failed to gather srflx candidate");
                }
            }
        }

        // Relay candidate via TURN
        if let Some(turn_server) = self.config.turn_server {
            if let (Some(username), Some(password)) = (
                self.config.turn_username.as_deref(),
                self.config.turn_password.as_deref(),
            ) {
                match allocate(&socket, turn_server, username, password).await {
                    Ok(Allocation { relay_addr, .. }) => {
                        let relay = IceCandidate::new(
                            CandidateType::Relay,
                            relay_addr,
                            65535,
                            "relay1".to_string(),
                            COMPONENT_RTP,
                        );
                        debug!(addr = %relay_addr, priority = relay.priority, "gathered relay candidate");
                        candidates.push(relay);
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to gather relay candidate");
                    }
                }
            } else {
                warn!("TURN server configured but credentials missing");
            }
        }

        // Sort by priority descending
        candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

        debug!(count = candidates.len(), "candidate gathering complete");
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_host_higher_than_srflx() {
        let host_prio = compute_priority(CandidateType::Host, 65535, 1);
        let srflx_prio = compute_priority(CandidateType::ServerReflexive, 65535, 1);
        let relay_prio = compute_priority(CandidateType::Relay, 65535, 1);

        assert!(host_prio > srflx_prio);
        assert!(srflx_prio > relay_prio);
    }

    #[test]
    fn priority_formula_matches_rfc() {
        // RFC 5245 §4.1.2.1:
        // priority = (2^24) * type_preference + (2^8) * local_preference + (256 - component_id)
        let prio = compute_priority(CandidateType::Host, 65535, 1);
        let expected = (126 << 24) + (65535 << 8) + (256 - 1);
        assert_eq!(prio, expected);

        let prio = compute_priority(CandidateType::ServerReflexive, 1000, 2);
        let expected = (100 << 24) + (1000 << 8) + (256 - 2);
        assert_eq!(prio, expected);

        let prio = compute_priority(CandidateType::Relay, 0, 1);
        let expected = (0 << 24) + (0 << 8) + (256 - 1);
        assert_eq!(prio, expected);
    }

    #[test]
    fn priority_component_affects_result() {
        let prio1 = compute_priority(CandidateType::Host, 65535, 1);
        let prio2 = compute_priority(CandidateType::Host, 65535, 2);
        assert!(prio1 > prio2);
        assert_eq!(prio1 - prio2, 1);
    }

    #[test]
    fn candidate_type_display() {
        assert_eq!(CandidateType::Host.to_string(), "host");
        assert_eq!(CandidateType::ServerReflexive.to_string(), "srflx");
        assert_eq!(CandidateType::Relay.to_string(), "relay");
    }

    #[test]
    fn candidate_type_parse() {
        assert_eq!("host".parse::<CandidateType>().unwrap(), CandidateType::Host);
        assert_eq!("srflx".parse::<CandidateType>().unwrap(), CandidateType::ServerReflexive);
        assert_eq!("relay".parse::<CandidateType>().unwrap(), CandidateType::Relay);
        assert!("unknown".parse::<CandidateType>().is_err());
    }

    #[test]
    fn sdp_serialization_roundtrip() {
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

    #[test]
    fn sdp_serialization_srflx() {
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

    #[test]
    fn sdp_serialization_relay() {
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

    #[test]
    fn sdp_parse_rejects_invalid_field_count() {
        let result = IceCandidate::from_sdp_string("too few fields");
        assert!(result.is_err());
    }

    #[test]
    fn sdp_parse_rejects_non_udp_transport() {
        let result = IceCandidate::from_sdp_string("f1 1 tcp 100 1.2.3.4 5000 typ host");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unsupported transport"));
    }

    #[test]
    fn sdp_parse_rejects_missing_typ_keyword() {
        let result = IceCandidate::from_sdp_string("f1 1 udp 100 1.2.3.4 5000 foo host");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("typ"));
    }

    #[test]
    fn sdp_parse_rejects_unknown_type() {
        let result = IceCandidate::from_sdp_string("f1 1 udp 100 1.2.3.4 5000 typ prflx");
        assert!(result.is_err());
    }

    #[test]
    fn sdp_ipv6_roundtrip() {
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

    #[test]
    fn candidates_sort_by_priority() {
        let host = IceCandidate::new(
            CandidateType::Host,
            "192.168.1.1:5000".parse().unwrap(),
            65535,
            "host1".to_string(),
            1,
        );
        let srflx = IceCandidate::new(
            CandidateType::ServerReflexive,
            "203.0.113.1:5000".parse().unwrap(),
            65535,
            "srflx1".to_string(),
            1,
        );
        let relay = IceCandidate::new(
            CandidateType::Relay,
            "10.0.0.1:5000".parse().unwrap(),
            65535,
            "relay1".to_string(),
            1,
        );

        let mut candidates = vec![relay.clone(), host.clone(), srflx.clone()];
        candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

        assert_eq!(candidates[0].candidate_type, CandidateType::Host);
        assert_eq!(candidates[1].candidate_type, CandidateType::ServerReflexive);
        assert_eq!(candidates[2].candidate_type, CandidateType::Relay);
    }

    #[test]
    fn ice_candidate_new_computes_priority() {
        let candidate = IceCandidate::new(
            CandidateType::Host,
            "127.0.0.1:5000".parse().unwrap(),
            65535,
            "host1".to_string(),
            1,
        );
        let expected = compute_priority(CandidateType::Host, 65535, 1);
        assert_eq!(candidate.priority, expected);
    }

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
    async fn gatherer_with_stun_loopback() {
        use crate::stun::{StunAttribute, StunMessage};

        // Spawn a mock STUN server
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (len, src) = server_sock.recv_from(&mut buf).await.unwrap();
            let request = StunMessage::from_bytes(&buf[..len]).unwrap();

            let reflexive_addr: SocketAddr = "203.0.113.42:5060".parse().unwrap();
            let response = StunMessage {
                msg_type: 0x0101, // Binding Response
                transaction_id: request.transaction_id,
                attributes: vec![StunAttribute::XorMappedAddress(reflexive_addr)],
            };
            server_sock.send_to(&response.to_bytes(), src).await.unwrap();
        });

        let config = GathererConfig {
            stun_server: Some(server_addr),
            turn_server: None,
            turn_username: None,
            turn_password: None,
        };
        let gatherer = CandidateGatherer::new(config);
        let candidates = gatherer.gather().await.unwrap();

        // Should have host + srflx
        assert_eq!(candidates.len(), 2);

        // Sorted by priority: host first, then srflx
        assert_eq!(candidates[0].candidate_type, CandidateType::Host);
        assert_eq!(candidates[1].candidate_type, CandidateType::ServerReflexive);
        assert_eq!(
            candidates[1].address,
            "203.0.113.42:5060".parse::<SocketAddr>().unwrap()
        );

        server_handle.await.unwrap();
    }

    #[test]
    fn gatherer_config_defaults() {
        let config = GathererConfig {
            stun_server: None,
            turn_server: None,
            turn_username: None,
            turn_password: None,
        };
        let _gatherer = CandidateGatherer::new(config);
    }
}
