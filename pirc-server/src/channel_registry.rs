use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use pirc_common::ChannelName;

use crate::channel::Channel;

/// Thread-safe registry mapping channel names to channel state.
///
/// Uses [`DashMap`] for lock-free concurrent reads with minimal write
/// contention. Channel name lookup is case-insensitive because
/// [`ChannelName`]'s `Eq` and `Hash` implementations use
/// ASCII-lowercased comparison.
pub struct ChannelRegistry {
    /// Channel name -> `Channel` (case-insensitive lookup via ChannelName's `Eq`/`Hash`).
    by_name: DashMap<ChannelName, Arc<RwLock<Channel>>>,
}

impl ChannelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            by_name: DashMap::new(),
        }
    }

    /// Get an existing channel or create a new empty one.
    ///
    /// If the channel already exists, returns the existing `Arc<RwLock<Channel>>`.
    /// Otherwise, creates a new empty channel, inserts it, and returns it.
    pub fn get_or_create(&self, name: ChannelName) -> Arc<RwLock<Channel>> {
        // Use entry API to avoid TOCTOU race between get and insert.
        let entry = self.by_name.entry(name.clone());
        Arc::clone(
            entry.or_insert_with(|| Arc::new(RwLock::new(Channel::new(name)))).value(),
        )
    }

    /// Look up a channel by name (case-insensitive).
    pub fn get(&self, name: &ChannelName) -> Option<Arc<RwLock<Channel>>> {
        self.by_name.get(name).map(|r| Arc::clone(r.value()))
    }

    /// Remove a channel if it is empty (has no members).
    ///
    /// Returns `true` if the channel was removed, `false` if it was not found
    /// or still has members.
    pub fn remove_if_empty(&self, name: &ChannelName) -> bool {
        // Use remove_if to atomically check emptiness and remove.
        self.by_name
            .remove_if(name, |_, channel| {
                let ch = channel.read().expect("channel lock poisoned");
                ch.is_empty()
            })
            .is_some()
    }

    /// List all channels with their member count and topic text.
    ///
    /// Returns a vector of `(name, member_count, topic_text)` tuples,
    /// suitable for building IRC LIST replies.
    pub fn list(&self) -> Vec<(ChannelName, usize, Option<String>)> {
        self.by_name
            .iter()
            .map(|entry| {
                let ch = entry.value().read().expect("channel lock poisoned");
                let topic_text = ch.topic.as_ref().map(|(text, _, _)| text.clone());
                (ch.name.clone(), ch.member_count(), topic_text)
            })
            .collect()
    }

    /// List all channels with their Arc handles.
    ///
    /// Returns a vector of `(name, channel_arc)` tuples for use by handlers
    /// that need direct access to channel state (e.g., LIST filtering by modes).
    pub fn list_all(&self) -> Vec<(ChannelName, Arc<RwLock<Channel>>)> {
        self.by_name
            .iter()
            .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
            .collect()
    }

    /// Returns the number of channels in the registry.
    pub fn channel_count(&self) -> usize {
        self.by_name.len()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use pirc_common::Nickname;

    use super::*;
    use crate::channel::MemberStatus;

    fn channel_name(s: &str) -> ChannelName {
        ChannelName::new(s).unwrap()
    }

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    // ---- Construction ----

    #[test]
    fn new_registry_is_empty() {
        let registry = ChannelRegistry::new();
        assert_eq!(registry.channel_count(), 0);
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = ChannelRegistry::default();
        assert_eq!(registry.channel_count(), 0);
    }

    // ---- get_or_create ----

    #[test]
    fn get_or_create_creates_new_channel() {
        let registry = ChannelRegistry::new();
        let ch = registry.get_or_create(channel_name("#test"));
        let ch_read = ch.read().unwrap();
        assert_eq!(ch_read.name, channel_name("#test"));
        assert!(ch_read.is_empty());
        assert_eq!(registry.channel_count(), 1);
    }

    #[test]
    fn get_or_create_returns_existing() {
        let registry = ChannelRegistry::new();

        // Create channel and add a member.
        let ch = registry.get_or_create(channel_name("#test"));
        {
            let mut ch_write = ch.write().unwrap();
            ch_write.members.insert(nick("Alice"), MemberStatus::Operator);
        }

        // get_or_create with same name should return the same channel.
        let ch2 = registry.get_or_create(channel_name("#test"));
        let ch2_read = ch2.read().unwrap();
        assert_eq!(ch2_read.member_count(), 1);
        assert_eq!(registry.channel_count(), 1);
    }

    #[test]
    fn get_or_create_case_insensitive() {
        let registry = ChannelRegistry::new();
        let ch1 = registry.get_or_create(channel_name("#Test"));
        {
            let mut ch = ch1.write().unwrap();
            ch.members.insert(nick("Alice"), MemberStatus::Normal);
        }

        // Lookup with different casing should return same channel.
        let ch2 = registry.get_or_create(channel_name("#test"));
        let ch2_read = ch2.read().unwrap();
        assert_eq!(ch2_read.member_count(), 1);
        assert_eq!(registry.channel_count(), 1);
    }

    // ---- get ----

    #[test]
    fn get_existing_channel() {
        let registry = ChannelRegistry::new();
        registry.get_or_create(channel_name("#test"));

        let found = registry.get(&channel_name("#test"));
        assert!(found.is_some());
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let registry = ChannelRegistry::new();
        assert!(registry.get(&channel_name("#nope")).is_none());
    }

    #[test]
    fn get_case_insensitive() {
        let registry = ChannelRegistry::new();
        registry.get_or_create(channel_name("#General"));

        assert!(registry.get(&channel_name("#general")).is_some());
        assert!(registry.get(&channel_name("#GENERAL")).is_some());
    }

    // ---- remove_if_empty ----

    #[test]
    fn remove_if_empty_removes_empty_channel() {
        let registry = ChannelRegistry::new();
        registry.get_or_create(channel_name("#test"));
        assert_eq!(registry.channel_count(), 1);

        assert!(registry.remove_if_empty(&channel_name("#test")));
        assert_eq!(registry.channel_count(), 0);
    }

    #[test]
    fn remove_if_empty_keeps_nonempty_channel() {
        let registry = ChannelRegistry::new();
        let ch = registry.get_or_create(channel_name("#test"));
        {
            let mut ch_write = ch.write().unwrap();
            ch_write.members.insert(nick("Alice"), MemberStatus::Normal);
        }

        assert!(!registry.remove_if_empty(&channel_name("#test")));
        assert_eq!(registry.channel_count(), 1);
    }

    #[test]
    fn remove_if_empty_nonexistent_returns_false() {
        let registry = ChannelRegistry::new();
        assert!(!registry.remove_if_empty(&channel_name("#nope")));
    }

    #[test]
    fn remove_if_empty_case_insensitive() {
        let registry = ChannelRegistry::new();
        registry.get_or_create(channel_name("#Test"));

        assert!(registry.remove_if_empty(&channel_name("#test")));
        assert_eq!(registry.channel_count(), 0);
    }

    // ---- list ----

    #[test]
    fn list_empty_registry() {
        let registry = ChannelRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn list_returns_all_channels() {
        let registry = ChannelRegistry::new();

        // Create #general with a topic and a member.
        let ch1 = registry.get_or_create(channel_name("#general"));
        {
            let mut ch = ch1.write().unwrap();
            ch.members.insert(nick("Alice"), MemberStatus::Operator);
            ch.topic = Some(("Welcome!".to_owned(), "Alice".to_owned(), 100));
        }

        // Create #random with no topic.
        let ch2 = registry.get_or_create(channel_name("#random"));
        {
            let mut ch = ch2.write().unwrap();
            ch.members.insert(nick("Bob"), MemberStatus::Normal);
            ch.members.insert(nick("Carol"), MemberStatus::Normal);
        }

        let mut entries = registry.list();
        entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));

        assert_eq!(entries.len(), 2);

        // #general: 1 member, has topic
        assert_eq!(entries[0].0, channel_name("#general"));
        assert_eq!(entries[0].1, 1);
        assert_eq!(entries[0].2.as_deref(), Some("Welcome!"));

        // #random: 2 members, no topic
        assert_eq!(entries[1].0, channel_name("#random"));
        assert_eq!(entries[1].1, 2);
        assert!(entries[1].2.is_none());
    }

    // ---- channel_count ----

    #[test]
    fn channel_count_tracks_additions() {
        let registry = ChannelRegistry::new();
        assert_eq!(registry.channel_count(), 0);

        registry.get_or_create(channel_name("#a"));
        assert_eq!(registry.channel_count(), 1);

        registry.get_or_create(channel_name("#b"));
        assert_eq!(registry.channel_count(), 2);

        // Duplicate should not increment.
        registry.get_or_create(channel_name("#a"));
        assert_eq!(registry.channel_count(), 2);
    }

    #[test]
    fn channel_count_tracks_removals() {
        let registry = ChannelRegistry::new();
        registry.get_or_create(channel_name("#a"));
        registry.get_or_create(channel_name("#b"));
        assert_eq!(registry.channel_count(), 2);

        registry.remove_if_empty(&channel_name("#a"));
        assert_eq!(registry.channel_count(), 1);

        registry.remove_if_empty(&channel_name("#b"));
        assert_eq!(registry.channel_count(), 0);
    }
}
