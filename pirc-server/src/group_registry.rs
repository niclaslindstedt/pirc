//! Server-side group chat registry.
//!
//! Tracks active group chats and their membership. Similar in spirit to
//! [`ChannelRegistry`](crate::channel_registry::ChannelRegistry) but
//! tailored for P2P encrypted group chats.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use pirc_common::types::GroupId;

/// Information about a single group member on the server side.
#[derive(Debug, Clone)]
pub struct ServerGroupMember {
    /// The member's nickname.
    pub nickname: String,
    /// Unix timestamp (seconds) when the member joined.
    pub joined_at: u64,
    /// Whether this member is an admin.
    pub is_admin: bool,
}

/// Server-side state for a single group chat.
#[derive(Debug)]
pub struct ServerGroup {
    /// Unique group identifier.
    pub id: GroupId,
    /// Human-readable group name.
    pub name: String,
    /// Nickname of the group creator.
    pub creator: String,
    /// Members keyed by nickname.
    pub members: HashMap<String, ServerGroupMember>,
}

impl ServerGroup {
    /// Returns `true` if the group has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Returns the number of members.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Returns `true` if the given nickname is a member.
    pub fn contains(&self, nickname: &str) -> bool {
        self.members.contains_key(nickname)
    }

    /// Returns the longest-tenured non-leaving member, excluding `exclude`.
    /// Used for admin transfer when the current admin leaves.
    pub fn longest_tenured_member(&self, exclude: &str) -> Option<&ServerGroupMember> {
        self.members
            .values()
            .filter(|m| m.nickname != exclude)
            .min_by_key(|m| m.joined_at)
    }
}

/// Thread-safe registry for server-side group chat management.
///
/// Uses [`DashMap`] for lock-free concurrent reads. Each group is
/// wrapped in a `std::sync::RwLock` for fine-grained mutable access.
pub struct GroupRegistry {
    groups: DashMap<GroupId, ServerGroup>,
    next_id: AtomicU64,
}

impl GroupRegistry {
    /// Creates an empty group registry.
    pub fn new() -> Self {
        Self {
            groups: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Allocates the next unique group ID.
    pub fn next_group_id(&self) -> GroupId {
        GroupId::new(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Creates a new group and returns its ID.
    ///
    /// The creator is automatically added as an admin member.
    pub fn create_group(&self, name: String, creator: String, now: u64) -> GroupId {
        let id = self.next_group_id();
        let creator_member = ServerGroupMember {
            nickname: creator.clone(),
            joined_at: now,
            is_admin: true,
        };
        let mut members = HashMap::new();
        members.insert(creator.clone(), creator_member);
        let group = ServerGroup {
            id,
            name,
            creator,
            members,
        };
        self.groups.insert(id, group);
        id
    }

    /// Returns `true` if a group with the given ID exists.
    pub fn exists(&self, group_id: GroupId) -> bool {
        self.groups.contains_key(&group_id)
    }

    /// Returns `true` if the given nickname is a member of the group.
    pub fn is_member(&self, group_id: GroupId, nickname: &str) -> bool {
        self.groups
            .get(&group_id)
            .is_some_and(|g| g.contains(nickname))
    }

    /// Adds a member to an existing group.
    ///
    /// Returns `true` if the member was added, `false` if the group
    /// doesn't exist or the member is already present.
    pub fn add_member(&self, group_id: GroupId, nickname: String, now: u64) -> bool {
        if let Some(mut group) = self.groups.get_mut(&group_id) {
            if group.members.contains_key(&nickname) {
                return false;
            }
            group.members.insert(
                nickname.clone(),
                ServerGroupMember {
                    nickname,
                    joined_at: now,
                    is_admin: false,
                },
            );
            true
        } else {
            false
        }
    }

    /// Removes a member from a group.
    ///
    /// If the leaving member is an admin, transfers admin to the
    /// longest-tenured remaining member. Returns the list of remaining
    /// member nicknames, or `None` if the group doesn't exist.
    ///
    /// If the group becomes empty, it is removed from the registry.
    pub fn remove_member(
        &self,
        group_id: GroupId,
        nickname: &str,
    ) -> Option<RemoveMemberResult> {
        let mut result = None;

        // We need to check inside the entry and potentially remove it
        self.groups.remove_if_mut(&group_id, |_, group| {
            let was_admin = group
                .members
                .get(nickname)
                .is_some_and(|m| m.is_admin);

            group.members.remove(nickname);

            if group.is_empty() {
                result = Some(RemoveMemberResult {
                    remaining: vec![],
                    new_admin: None,
                    group_destroyed: true,
                });
                return true; // remove the entry
            }

            // Transfer admin if the leaver was admin
            let new_admin = if was_admin {
                if let Some(successor) = group.longest_tenured_member(nickname) {
                    let successor_nick = successor.nickname.clone();
                    if let Some(member) = group.members.get_mut(&successor_nick) {
                        member.is_admin = true;
                    }
                    Some(successor_nick)
                } else {
                    None
                }
            } else {
                None
            };

            let remaining: Vec<String> = group.members.keys().cloned().collect();

            result = Some(RemoveMemberResult {
                remaining,
                new_admin,
                group_destroyed: false,
            });

            false // keep the entry
        });

        result
    }

    /// Returns the nicknames of all members in a group.
    pub fn members(&self, group_id: GroupId) -> Vec<String> {
        self.groups
            .get(&group_id)
            .map(|g| g.members.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns the group name, or `None` if the group doesn't exist.
    pub fn group_name(&self, group_id: GroupId) -> Option<String> {
        self.groups.get(&group_id).map(|g| g.name.clone())
    }

    /// Returns the number of tracked groups.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Returns `true` if the given member is an admin of the group.
    pub fn is_admin(&self, group_id: GroupId, nickname: &str) -> bool {
        self.groups
            .get(&group_id)
            .is_some_and(|g| {
                g.members
                    .get(nickname)
                    .is_some_and(|m| m.is_admin)
            })
    }

    /// Returns all group IDs that the given nickname is a member of.
    pub fn groups_for_member(&self, nickname: &str) -> Vec<GroupId> {
        self.groups
            .iter()
            .filter(|entry| entry.value().contains(nickname))
            .map(|entry| *entry.key())
            .collect()
    }
}

impl Default for GroupRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of removing a member from a group.
#[derive(Debug)]
pub struct RemoveMemberResult {
    /// Nicknames of members still in the group.
    pub remaining: Vec<String>,
    /// If admin was transferred, the new admin's nickname.
    pub new_admin: Option<String>,
    /// Whether the group was destroyed (no members left).
    pub group_destroyed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        1_000_000
    }

    // ── Construction ────────────────────────────────────────────

    #[test]
    fn new_registry_is_empty() {
        let registry = GroupRegistry::new();
        assert_eq!(registry.group_count(), 0);
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = GroupRegistry::default();
        assert_eq!(registry.group_count(), 0);
    }

    // ── Group creation ──────────────────────────────────────────

    #[test]
    fn create_group_assigns_unique_ids() {
        let registry = GroupRegistry::new();
        let id1 = registry.create_group("g1".into(), "alice".into(), now());
        let id2 = registry.create_group("g2".into(), "bob".into(), now());
        assert_ne!(id1, id2);
        assert_eq!(registry.group_count(), 2);
    }

    #[test]
    fn create_group_adds_creator_as_admin() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        assert!(registry.is_member(id, "alice"));
        assert!(registry.is_admin(id, "alice"));
    }

    #[test]
    fn group_name_returns_name() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("my-group".into(), "alice".into(), now());
        assert_eq!(registry.group_name(id), Some("my-group".into()));
    }

    #[test]
    fn group_name_nonexistent_returns_none() {
        let registry = GroupRegistry::new();
        assert!(registry.group_name(GroupId::new(999)).is_none());
    }

    // ── Membership ──────────────────────────────────────────────

    #[test]
    fn add_member_to_group() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        assert!(registry.add_member(id, "bob".into(), now() + 1));
        assert!(registry.is_member(id, "bob"));
        assert!(!registry.is_admin(id, "bob"));
    }

    #[test]
    fn add_duplicate_member_returns_false() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        assert!(!registry.add_member(id, "alice".into(), now()));
    }

    #[test]
    fn add_member_nonexistent_group_returns_false() {
        let registry = GroupRegistry::new();
        assert!(!registry.add_member(GroupId::new(999), "alice".into(), now()));
    }

    #[test]
    fn is_member_false_for_nonmember() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        assert!(!registry.is_member(id, "bob"));
    }

    #[test]
    fn is_member_false_for_nonexistent_group() {
        let registry = GroupRegistry::new();
        assert!(!registry.is_member(GroupId::new(999), "alice"));
    }

    #[test]
    fn members_lists_all() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        registry.add_member(id, "bob".into(), now() + 1);
        let mut members = registry.members(id);
        members.sort();
        assert_eq!(members, vec!["alice", "bob"]);
    }

    #[test]
    fn members_empty_for_nonexistent_group() {
        let registry = GroupRegistry::new();
        assert!(registry.members(GroupId::new(999)).is_empty());
    }

    // ── Remove member ───────────────────────────────────────────

    #[test]
    fn remove_regular_member() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        registry.add_member(id, "bob".into(), now() + 1);

        let result = registry.remove_member(id, "bob").unwrap();
        assert!(!result.group_destroyed);
        assert!(result.new_admin.is_none());
        assert_eq!(result.remaining, vec!["alice"]);
        assert!(!registry.is_member(id, "bob"));
    }

    #[test]
    fn remove_admin_transfers_to_longest_tenured() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        registry.add_member(id, "bob".into(), now() + 1);
        registry.add_member(id, "charlie".into(), now() + 2);

        let result = registry.remove_member(id, "alice").unwrap();
        assert!(!result.group_destroyed);
        assert_eq!(result.new_admin, Some("bob".into()));
        assert!(registry.is_admin(id, "bob"));
    }

    #[test]
    fn remove_last_member_destroys_group() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());

        let result = registry.remove_member(id, "alice").unwrap();
        assert!(result.group_destroyed);
        assert!(result.remaining.is_empty());
        assert!(!registry.exists(id));
        assert_eq!(registry.group_count(), 0);
    }

    #[test]
    fn remove_from_nonexistent_group_returns_none() {
        let registry = GroupRegistry::new();
        assert!(registry.remove_member(GroupId::new(999), "alice").is_none());
    }

    #[test]
    fn remove_nonexistent_member() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        let result = registry.remove_member(id, "ghost").unwrap();
        assert!(!result.group_destroyed);
        assert_eq!(result.remaining, vec!["alice"]);
    }

    // ── exists ──────────────────────────────────────────────────

    #[test]
    fn exists_true_for_created_group() {
        let registry = GroupRegistry::new();
        let id = registry.create_group("g1".into(), "alice".into(), now());
        assert!(registry.exists(id));
    }

    #[test]
    fn exists_false_for_unknown_id() {
        let registry = GroupRegistry::new();
        assert!(!registry.exists(GroupId::new(999)));
    }
}
