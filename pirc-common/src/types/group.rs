use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::identifiers::GroupId;
use super::mode::GroupMemberRole;

/// Metadata about a group chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInfo {
    /// Unique identifier for the group.
    pub id: GroupId,
    /// Human-readable name of the group.
    pub name: String,
    /// Nickname of the user who created the group.
    pub creator: String,
    /// Unix timestamp (seconds) when the group was created.
    pub created_at: u64,
}

impl GroupInfo {
    /// Creates a new `GroupInfo`.
    pub fn new(id: GroupId, name: String, creator: String, created_at: u64) -> Self {
        Self {
            id,
            name,
            creator,
            created_at,
        }
    }
}

/// Information about a single member of a group chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    /// Nickname of the member.
    pub nickname: String,
    /// Unix timestamp (seconds) when the member joined.
    pub joined_at: u64,
    /// Role of the member within the group.
    pub role: GroupMemberRole,
}

impl GroupMember {
    /// Creates a new `GroupMember`.
    pub fn new(nickname: String, joined_at: u64, role: GroupMemberRole) -> Self {
        Self {
            nickname,
            joined_at,
            role,
        }
    }
}

/// Tracks the members belonging to a group chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMembership {
    /// The group this membership tracks.
    pub group_id: GroupId,
    /// Members keyed by nickname.
    pub members: HashMap<String, GroupMember>,
}

impl GroupMembership {
    /// Creates a new empty `GroupMembership` for the given group.
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            members: HashMap::new(),
        }
    }

    /// Adds a member to the group. Returns the previous member if the
    /// nickname was already present.
    pub fn add_member(&mut self, member: GroupMember) -> Option<GroupMember> {
        self.members.insert(member.nickname.clone(), member)
    }

    /// Removes a member by nickname. Returns the removed member if found.
    pub fn remove_member(&mut self, nickname: &str) -> Option<GroupMember> {
        self.members.remove(nickname)
    }

    /// Returns the member with the given nickname, if present.
    pub fn get_member(&self, nickname: &str) -> Option<&GroupMember> {
        self.members.get(nickname)
    }

    /// Returns the number of members in the group.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Returns `true` if the given nickname is a member of this group.
    pub fn contains(&self, nickname: &str) -> bool {
        self.members.contains_key(nickname)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- GroupInfo ----

    #[test]
    fn group_info_construction() {
        let info = GroupInfo::new(GroupId::new(1), "test-group".into(), "alice".into(), 1000);
        assert_eq!(info.id, GroupId::new(1));
        assert_eq!(info.name, "test-group");
        assert_eq!(info.creator, "alice");
        assert_eq!(info.created_at, 1000);
    }

    #[test]
    fn group_info_equality() {
        let a = GroupInfo::new(GroupId::new(1), "grp".into(), "alice".into(), 100);
        let b = GroupInfo::new(GroupId::new(1), "grp".into(), "alice".into(), 100);
        assert_eq!(a, b);
    }

    #[test]
    fn group_info_inequality_different_id() {
        let a = GroupInfo::new(GroupId::new(1), "grp".into(), "alice".into(), 100);
        let b = GroupInfo::new(GroupId::new(2), "grp".into(), "alice".into(), 100);
        assert_ne!(a, b);
    }

    #[test]
    fn group_info_clone() {
        let info = GroupInfo::new(GroupId::new(1), "grp".into(), "alice".into(), 100);
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    #[test]
    fn group_info_serde_roundtrip() {
        let info = GroupInfo::new(GroupId::new(42), "my-group".into(), "bob".into(), 9999);
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: GroupInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, deserialized);
    }

    // ---- GroupMember ----

    #[test]
    fn group_member_construction() {
        let member = GroupMember::new("alice".into(), 500, GroupMemberRole::Admin);
        assert_eq!(member.nickname, "alice");
        assert_eq!(member.joined_at, 500);
        assert_eq!(member.role, GroupMemberRole::Admin);
    }

    #[test]
    fn group_member_equality() {
        let a = GroupMember::new("alice".into(), 500, GroupMemberRole::Member);
        let b = GroupMember::new("alice".into(), 500, GroupMemberRole::Member);
        assert_eq!(a, b);
    }

    #[test]
    fn group_member_inequality_different_role() {
        let a = GroupMember::new("alice".into(), 500, GroupMemberRole::Member);
        let b = GroupMember::new("alice".into(), 500, GroupMemberRole::Admin);
        assert_ne!(a, b);
    }

    #[test]
    fn group_member_serde_roundtrip() {
        let member = GroupMember::new("bob".into(), 1000, GroupMemberRole::Admin);
        let json = serde_json::to_string(&member).unwrap();
        let deserialized: GroupMember = serde_json::from_str(&json).unwrap();
        assert_eq!(member, deserialized);
    }

    // ---- GroupMembership ----

    #[test]
    fn group_membership_new_is_empty() {
        let membership = GroupMembership::new(GroupId::new(1));
        assert_eq!(membership.group_id, GroupId::new(1));
        assert_eq!(membership.member_count(), 0);
    }

    #[test]
    fn group_membership_add_member() {
        let mut membership = GroupMembership::new(GroupId::new(1));
        let member = GroupMember::new("alice".into(), 100, GroupMemberRole::Admin);
        let prev = membership.add_member(member);
        assert!(prev.is_none());
        assert_eq!(membership.member_count(), 1);
        assert!(membership.contains("alice"));
    }

    #[test]
    fn group_membership_add_member_replaces() {
        let mut membership = GroupMembership::new(GroupId::new(1));
        membership.add_member(GroupMember::new("alice".into(), 100, GroupMemberRole::Member));
        let prev =
            membership.add_member(GroupMember::new("alice".into(), 200, GroupMemberRole::Admin));
        assert!(prev.is_some());
        assert_eq!(membership.member_count(), 1);
        assert_eq!(
            membership.get_member("alice").unwrap().role,
            GroupMemberRole::Admin
        );
    }

    #[test]
    fn group_membership_remove_member() {
        let mut membership = GroupMembership::new(GroupId::new(1));
        membership.add_member(GroupMember::new("alice".into(), 100, GroupMemberRole::Member));
        let removed = membership.remove_member("alice");
        assert!(removed.is_some());
        assert_eq!(membership.member_count(), 0);
        assert!(!membership.contains("alice"));
    }

    #[test]
    fn group_membership_remove_nonexistent() {
        let mut membership = GroupMembership::new(GroupId::new(1));
        let removed = membership.remove_member("ghost");
        assert!(removed.is_none());
    }

    #[test]
    fn group_membership_get_member() {
        let mut membership = GroupMembership::new(GroupId::new(1));
        membership.add_member(GroupMember::new("alice".into(), 100, GroupMemberRole::Admin));
        let member = membership.get_member("alice").unwrap();
        assert_eq!(member.nickname, "alice");
        assert_eq!(member.role, GroupMemberRole::Admin);
    }

    #[test]
    fn group_membership_get_member_not_found() {
        let membership = GroupMembership::new(GroupId::new(1));
        assert!(membership.get_member("ghost").is_none());
    }

    #[test]
    fn group_membership_serde_roundtrip() {
        let mut membership = GroupMembership::new(GroupId::new(5));
        membership.add_member(GroupMember::new("alice".into(), 100, GroupMemberRole::Admin));
        membership.add_member(GroupMember::new("bob".into(), 200, GroupMemberRole::Member));
        let json = serde_json::to_string(&membership).unwrap();
        let deserialized: GroupMembership = serde_json::from_str(&json).unwrap();
        assert_eq!(membership, deserialized);
    }
}
