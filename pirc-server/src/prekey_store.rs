use std::collections::HashMap;
use std::sync::Mutex;

use dashmap::DashMap;
use pirc_common::Nickname;

/// Thread-safe in-memory store mapping nicknames to their serialized pre-key bundles.
///
/// Uses [`DashMap`] for lock-free concurrent reads with minimal write
/// contention, consistent with the [`crate::registry::UserRegistry`] pattern.
/// Nickname lookup is case-insensitive because [`Nickname`]'s `Eq` and `Hash`
/// implementations use ASCII-lowercased comparison.
pub struct PreKeyBundleStore {
    bundles: DashMap<Nickname, Vec<u8>>,
    /// Buffer for in-progress chunked bundle uploads. Keyed by nick.
    /// Value is (chunks_by_index, total_chunks).
    chunk_buffer: Mutex<HashMap<Nickname, (Vec<Option<String>>, usize)>>,
}

impl PreKeyBundleStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            bundles: DashMap::new(),
            chunk_buffer: Mutex::new(HashMap::new()),
        }
    }

    /// Accumulate one chunk of a chunked bundle upload.
    ///
    /// `n` is 1-based; `total` is the declared total chunk count.
    /// Returns the assembled base64 string once all `total` chunks have
    /// arrived, or `None` if more chunks are still expected.
    pub fn add_bundle_chunk(&self, nick: &Nickname, chunk: &str, n: usize, total: usize) -> Option<String> {
        let mut guard = self.chunk_buffer.lock().expect("chunk buffer lock poisoned");
        let entry = guard
            .entry(nick.clone())
            .or_insert_with(|| (vec![None; total], total));

        // Reset buffer if the total changed (new upload attempt).
        if entry.1 != total {
            *entry = (vec![None; total], total);
        }

        if n >= 1 && n <= total {
            entry.0[n - 1] = Some(chunk.to_string());
        }

        if entry.0.iter().all(|c| c.is_some()) {
            let assembled: String = entry.0.iter().map(|c| c.as_deref().unwrap_or("")).collect();
            guard.remove(nick);
            Some(assembled)
        } else {
            None
        }
    }

    /// Store or replace a user's serialized pre-key bundle.
    pub fn store_bundle(&self, nick: &Nickname, bundle_data: Vec<u8>) {
        self.bundles.insert(nick.clone(), bundle_data);
    }

    /// Retrieve a user's serialized pre-key bundle.
    pub fn get_bundle(&self, nick: &Nickname) -> Option<Vec<u8>> {
        self.bundles.get(nick).map(|r| r.value().clone())
    }

    /// Remove a user's pre-key bundle (e.g. on disconnect or nick change).
    pub fn remove_bundle(&self, nick: &Nickname) {
        self.bundles.remove(nick);
    }
}

impl Default for PreKeyBundleStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    #[test]
    fn store_and_get_bundle() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        let data = vec![1, 2, 3, 4];

        store.store_bundle(&alice, data.clone());
        let retrieved = store.get_bundle(&alice).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn get_missing_bundle_returns_none() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        assert!(store.get_bundle(&alice).is_none());
    }

    #[test]
    fn store_replaces_existing_bundle() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");

        store.store_bundle(&alice, vec![1, 2, 3]);
        store.store_bundle(&alice, vec![4, 5, 6]);

        let retrieved = store.get_bundle(&alice).unwrap();
        assert_eq!(retrieved, vec![4, 5, 6]);
    }

    #[test]
    fn remove_bundle_clears_entry() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");

        store.store_bundle(&alice, vec![1, 2, 3]);
        store.remove_bundle(&alice);
        assert!(store.get_bundle(&alice).is_none());
    }

    #[test]
    fn remove_nonexistent_bundle_is_noop() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        store.remove_bundle(&alice); // should not panic
    }

    #[test]
    fn case_insensitive_lookup() {
        let store = PreKeyBundleStore::new();
        let alice_upper = nick("Alice");
        let alice_lower = nick("alice");

        store.store_bundle(&alice_upper, vec![1, 2, 3]);
        let retrieved = store.get_bundle(&alice_lower).unwrap();
        assert_eq!(retrieved, vec![1, 2, 3]);
    }

    #[test]
    fn multiple_users_stored_independently() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        let bob = nick("Bob");

        store.store_bundle(&alice, vec![1, 2, 3]);
        store.store_bundle(&bob, vec![4, 5, 6]);

        assert_eq!(store.get_bundle(&alice).unwrap(), vec![1, 2, 3]);
        assert_eq!(store.get_bundle(&bob).unwrap(), vec![4, 5, 6]);
    }

    #[test]
    fn default_creates_empty_store() {
        let store = PreKeyBundleStore::default();
        let alice = nick("Alice");
        assert!(store.get_bundle(&alice).is_none());
    }

    // ── add_bundle_chunk ──────────────────────────────────────────

    #[test]
    fn chunk_single_returns_immediately() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        let result = store.add_bundle_chunk(&alice, "abc", 1, 1);
        assert_eq!(result, Some("abc".to_string()));
    }

    #[test]
    fn chunk_two_parts_assembled_in_order() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        assert!(store.add_bundle_chunk(&alice, "foo", 1, 2).is_none());
        let result = store.add_bundle_chunk(&alice, "bar", 2, 2);
        assert_eq!(result, Some("foobar".to_string()));
    }

    #[test]
    fn chunk_out_of_order_assembly() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        assert!(store.add_bundle_chunk(&alice, "bar", 2, 2).is_none());
        let result = store.add_bundle_chunk(&alice, "foo", 1, 2);
        assert_eq!(result, Some("foobar".to_string()));
    }

    #[test]
    fn chunk_buffer_cleared_after_assembly() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        store.add_bundle_chunk(&alice, "x", 1, 1);
        // A second upload should start fresh.
        let result = store.add_bundle_chunk(&alice, "y", 1, 1);
        assert_eq!(result, Some("y".to_string()));
    }

    #[test]
    fn chunk_total_change_resets_buffer() {
        let store = PreKeyBundleStore::new();
        let alice = nick("Alice");
        // Start a 3-chunk upload.
        assert!(store.add_bundle_chunk(&alice, "a", 1, 3).is_none());
        // New upload with total=2 should reset.
        assert!(store.add_bundle_chunk(&alice, "x", 1, 2).is_none());
        let result = store.add_bundle_chunk(&alice, "y", 2, 2);
        assert_eq!(result, Some("xy".to_string()));
    }
}
