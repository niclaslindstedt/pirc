//! Protocol version types for pirc version negotiation.
//!
//! The pirc protocol uses `MAJOR.MINOR` versioning. Two versions are compatible
//! if they share the same major version number. Negotiation selects the minimum
//! of two compatible versions.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::error::ProtocolError;

/// Current protocol version.
pub const PROTOCOL_VERSION_CURRENT: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// A protocol version with major and minor components.
///
/// Versions follow `MAJOR.MINOR` semantics:
/// - **Major** changes are breaking and incompatible.
/// - **Minor** changes are backwards-compatible additions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    /// Create a new protocol version.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns `true` if this version is compatible with `other`.
    ///
    /// Compatibility requires matching major version numbers.
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }

    /// Negotiate the effective protocol version between local and remote.
    ///
    /// Returns the minimum compatible version (lower of the two), or `None`
    /// if the versions are incompatible (different major versions).
    pub fn negotiate(local: &Self, remote: &Self) -> Option<Self> {
        if !local.is_compatible(remote) {
            return None;
        }
        Some(std::cmp::min(*local, *remote))
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ProtocolVersion {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (major_str, minor_str) = s
            .split_once('.')
            .ok_or_else(|| ProtocolError::InvalidVersion(format!("missing '.' in version: {s}")))?;

        let major = major_str.parse::<u16>().map_err(|_| {
            ProtocolError::InvalidVersion(format!("invalid major version: {major_str}"))
        })?;

        let minor = minor_str.parse::<u16>().map_err(|_| {
            ProtocolError::InvalidVersion(format!("invalid minor version: {minor_str}"))
        })?;

        Ok(Self { major, minor })
    }
}

impl PartialOrd for ProtocolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProtocolVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Construction ----

    #[test]
    fn new_version() {
        let v = ProtocolVersion::new(1, 0);
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
    }

    #[test]
    fn current_version_is_1_0() {
        assert_eq!(PROTOCOL_VERSION_CURRENT, ProtocolVersion::new(1, 0));
    }

    // ---- Display ----

    #[test]
    fn display_version() {
        assert_eq!(ProtocolVersion::new(1, 0).to_string(), "1.0");
        assert_eq!(ProtocolVersion::new(2, 3).to_string(), "2.3");
        assert_eq!(ProtocolVersion::new(0, 1).to_string(), "0.1");
    }

    // ---- FromStr ----

    #[test]
    fn parse_valid_version() {
        let v: ProtocolVersion = "1.0".parse().unwrap();
        assert_eq!(v, ProtocolVersion::new(1, 0));
    }

    #[test]
    fn parse_version_2_3() {
        let v: ProtocolVersion = "2.3".parse().unwrap();
        assert_eq!(v, ProtocolVersion::new(2, 3));
    }

    #[test]
    fn parse_version_0_1() {
        let v: ProtocolVersion = "0.1".parse().unwrap();
        assert_eq!(v, ProtocolVersion::new(0, 1));
    }

    #[test]
    fn parse_invalid_no_dot() {
        let err = "10".parse::<ProtocolVersion>().unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidVersion(_)));
    }

    #[test]
    fn parse_invalid_major() {
        let err = "abc.0".parse::<ProtocolVersion>().unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidVersion(_)));
    }

    #[test]
    fn parse_invalid_minor() {
        let err = "1.xyz".parse::<ProtocolVersion>().unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidVersion(_)));
    }

    #[test]
    fn parse_empty_string() {
        let err = "".parse::<ProtocolVersion>().unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidVersion(_)));
    }

    // ---- Roundtrip Display -> FromStr ----

    #[test]
    fn roundtrip_display_parse() {
        let v = ProtocolVersion::new(3, 7);
        let parsed: ProtocolVersion = v.to_string().parse().unwrap();
        assert_eq!(v, parsed);
    }

    // ---- Ordering ----

    #[test]
    fn version_ordering() {
        let v1_0 = ProtocolVersion::new(1, 0);
        let v1_1 = ProtocolVersion::new(1, 1);
        let v1_2 = ProtocolVersion::new(1, 2);
        let v2_0 = ProtocolVersion::new(2, 0);

        assert!(v1_0 < v1_1);
        assert!(v1_1 < v1_2);
        assert!(v1_2 < v2_0);
        assert!(v1_0 < v2_0);
    }

    #[test]
    fn version_equality_ordering() {
        let a = ProtocolVersion::new(1, 0);
        let b = ProtocolVersion::new(1, 0);
        assert_eq!(a.cmp(&b), Ordering::Equal);
        assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));
    }

    // ---- Compatibility ----

    #[test]
    fn compatible_same_major() {
        let v1_0 = ProtocolVersion::new(1, 0);
        let v1_5 = ProtocolVersion::new(1, 5);
        assert!(v1_0.is_compatible(&v1_5));
        assert!(v1_5.is_compatible(&v1_0));
    }

    #[test]
    fn incompatible_different_major() {
        let v1_0 = ProtocolVersion::new(1, 0);
        let v2_0 = ProtocolVersion::new(2, 0);
        assert!(!v1_0.is_compatible(&v2_0));
        assert!(!v2_0.is_compatible(&v1_0));
    }

    #[test]
    fn compatible_same_version() {
        let v = ProtocolVersion::new(1, 0);
        assert!(v.is_compatible(&v));
    }

    // ---- Negotiation ----

    #[test]
    fn negotiate_same_version() {
        let v = ProtocolVersion::new(1, 0);
        assert_eq!(ProtocolVersion::negotiate(&v, &v), Some(v));
    }

    #[test]
    fn negotiate_compatible_picks_minimum() {
        let v1_0 = ProtocolVersion::new(1, 0);
        let v1_3 = ProtocolVersion::new(1, 3);
        assert_eq!(ProtocolVersion::negotiate(&v1_0, &v1_3), Some(v1_0));
        assert_eq!(ProtocolVersion::negotiate(&v1_3, &v1_0), Some(v1_0));
    }

    #[test]
    fn negotiate_incompatible_returns_none() {
        let v1_0 = ProtocolVersion::new(1, 0);
        let v2_0 = ProtocolVersion::new(2, 0);
        assert_eq!(ProtocolVersion::negotiate(&v1_0, &v2_0), None);
    }

    // ---- Clone / Copy / Hash ----

    #[test]
    fn version_clone() {
        let v = ProtocolVersion::new(1, 2);
        let cloned = v.clone();
        assert_eq!(v, cloned);
    }

    #[test]
    fn version_copy() {
        let v = ProtocolVersion::new(1, 2);
        let copied = v;
        assert_eq!(v, copied); // v is still usable (Copy)
    }

    #[test]
    fn version_debug() {
        let debug = format!("{:?}", ProtocolVersion::new(1, 0));
        assert!(debug.contains("major: 1"));
        assert!(debug.contains("minor: 0"));
    }
}
