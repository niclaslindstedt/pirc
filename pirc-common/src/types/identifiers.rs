use std::fmt;

use serde::{Deserialize, Serialize};

/// A unique identifier for a server (Raft node).
///
/// Wraps a `u64` and is `Copy`, making it lightweight for use as keys
/// in maps and sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ServerId(u64);

impl ServerId {
    /// Create a new `ServerId` from a raw `u64`.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the underlying `u64` value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A unique identifier for a group chat.
///
/// Wraps a `u64` and is `Copy`, making it lightweight for use as keys
/// in maps and sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(u64);

impl GroupId {
    /// Create a new `GroupId` from a raw `u64`.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the underlying `u64` value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for GroupId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse::<u64>().map(Self)
    }
}

/// A unique identifier for a user (internal tracking).
///
/// Distinct from [`Nickname`](super::Nickname), which is the user-visible display name.
/// `UserId` is used for internal bookkeeping and is `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UserId(u64);

impl UserId {
    /// Create a new `UserId` from a raw `u64`.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the underlying `u64` value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ServerId ----

    #[test]
    fn server_id_construction() {
        let id = ServerId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn server_id_zero() {
        let id = ServerId::new(0);
        assert_eq!(id.as_u64(), 0);
    }

    #[test]
    fn server_id_max() {
        let id = ServerId::new(u64::MAX);
        assert_eq!(id.as_u64(), u64::MAX);
    }

    #[test]
    fn server_id_equality() {
        assert_eq!(ServerId::new(1), ServerId::new(1));
        assert_ne!(ServerId::new(1), ServerId::new(2));
    }

    #[test]
    fn server_id_ordering() {
        assert!(ServerId::new(1) < ServerId::new(2));
        assert!(ServerId::new(10) > ServerId::new(5));
    }

    #[test]
    fn server_id_display() {
        let id = ServerId::new(42);
        assert_eq!(format!("{id}"), "42");
    }

    #[test]
    fn server_id_display_zero() {
        assert_eq!(ServerId::new(0).to_string(), "0");
    }

    #[test]
    fn server_id_copy() {
        let id = ServerId::new(7);
        let copied = id;
        assert_eq!(id, copied); // id still usable after copy
    }

    #[test]
    fn server_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ServerId::new(1));
        set.insert(ServerId::new(2));
        set.insert(ServerId::new(1)); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn server_id_btreemap() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(ServerId::new(3), "c");
        map.insert(ServerId::new(1), "a");
        map.insert(ServerId::new(2), "b");
        let keys: Vec<_> = map.keys().collect();
        assert_eq!(
            keys,
            vec![&ServerId::new(1), &ServerId::new(2), &ServerId::new(3)]
        );
    }

    #[test]
    fn server_id_serde_roundtrip() {
        let id = ServerId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ServerId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn server_id_serde_value() {
        let id = ServerId::new(99);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "99");
    }

    // ---- UserId ----

    #[test]
    fn user_id_construction() {
        let id = UserId::new(1);
        assert_eq!(id.as_u64(), 1);
    }

    #[test]
    fn user_id_zero() {
        let id = UserId::new(0);
        assert_eq!(id.as_u64(), 0);
    }

    #[test]
    fn user_id_max() {
        let id = UserId::new(u64::MAX);
        assert_eq!(id.as_u64(), u64::MAX);
    }

    #[test]
    fn user_id_equality() {
        assert_eq!(UserId::new(1), UserId::new(1));
        assert_ne!(UserId::new(1), UserId::new(2));
    }

    #[test]
    fn user_id_ordering() {
        assert!(UserId::new(1) < UserId::new(2));
        assert!(UserId::new(10) > UserId::new(5));
    }

    #[test]
    fn user_id_display() {
        let id = UserId::new(7);
        assert_eq!(format!("{id}"), "7");
    }

    #[test]
    fn user_id_display_zero() {
        assert_eq!(UserId::new(0).to_string(), "0");
    }

    #[test]
    fn user_id_copy() {
        let id = UserId::new(42);
        let copied = id;
        assert_eq!(id, copied);
    }

    #[test]
    fn user_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(UserId::new(1));
        set.insert(UserId::new(2));
        set.insert(UserId::new(1));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn user_id_btreemap() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(UserId::new(3), "c");
        map.insert(UserId::new(1), "a");
        map.insert(UserId::new(2), "b");
        let keys: Vec<_> = map.keys().collect();
        assert_eq!(
            keys,
            vec![&UserId::new(1), &UserId::new(2), &UserId::new(3)]
        );
    }

    #[test]
    fn user_id_serde_roundtrip() {
        let id = UserId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn user_id_serde_value() {
        let id = UserId::new(99);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "99");
    }

    // ---- GroupId ----

    #[test]
    fn group_id_construction() {
        let id = GroupId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn group_id_zero() {
        let id = GroupId::new(0);
        assert_eq!(id.as_u64(), 0);
    }

    #[test]
    fn group_id_max() {
        let id = GroupId::new(u64::MAX);
        assert_eq!(id.as_u64(), u64::MAX);
    }

    #[test]
    fn group_id_equality() {
        assert_eq!(GroupId::new(1), GroupId::new(1));
        assert_ne!(GroupId::new(1), GroupId::new(2));
    }

    #[test]
    fn group_id_ordering() {
        assert!(GroupId::new(1) < GroupId::new(2));
        assert!(GroupId::new(10) > GroupId::new(5));
    }

    #[test]
    fn group_id_display() {
        let id = GroupId::new(42);
        assert_eq!(format!("{id}"), "42");
    }

    #[test]
    fn group_id_display_zero() {
        assert_eq!(GroupId::new(0).to_string(), "0");
    }

    #[test]
    fn group_id_copy() {
        let id = GroupId::new(7);
        let copied = id;
        assert_eq!(id, copied);
    }

    #[test]
    fn group_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(GroupId::new(1));
        set.insert(GroupId::new(2));
        set.insert(GroupId::new(1));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn group_id_btreemap() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(GroupId::new(3), "c");
        map.insert(GroupId::new(1), "a");
        map.insert(GroupId::new(2), "b");
        let keys: Vec<_> = map.keys().collect();
        assert_eq!(
            keys,
            vec![&GroupId::new(1), &GroupId::new(2), &GroupId::new(3)]
        );
    }

    #[test]
    fn group_id_serde_roundtrip() {
        let id = GroupId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: GroupId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn group_id_serde_value() {
        let id = GroupId::new(99);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "99");
    }

    #[test]
    fn group_id_from_str() {
        let id: GroupId = "42".parse().unwrap();
        assert_eq!(id, GroupId::new(42));
    }

    #[test]
    fn group_id_from_str_invalid() {
        let result: std::result::Result<GroupId, _> = "abc".parse();
        assert!(result.is_err());
    }

    // ---- Cross-type ----

    #[test]
    fn server_id_and_user_id_are_distinct_types() {
        // Ensure type safety: ServerId and UserId cannot be mixed.
        // This is a compile-time guarantee; we verify at runtime that
        // the same numeric value produces different debug output.
        let server = ServerId::new(1);
        let user = UserId::new(1);
        assert_eq!(format!("{server:?}"), "ServerId(1)");
        assert_eq!(format!("{user:?}"), "UserId(1)");
    }

    #[test]
    fn group_id_is_distinct_type() {
        let group = GroupId::new(1);
        assert_eq!(format!("{group:?}"), "GroupId(1)");
    }
}
