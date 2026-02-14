use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use pirc_common::{Nickname, UserError};

use crate::user::UserSession;

/// Thread-safe registry mapping nicknames to user sessions.
///
/// Uses [`DashMap`] for lock-free concurrent reads with minimal write
/// contention. Nickname lookup is case-insensitive because [`Nickname`]'s
/// `Eq` and `Hash` implementations use ASCII-lowercased comparison.
pub struct UserRegistry {
    /// Nickname -> `UserSession` (case-insensitive lookup via Nickname's `Eq`/`Hash`).
    by_nick: DashMap<Nickname, Arc<RwLock<UserSession>>>,
    /// Connection ID -> Nickname (for reverse lookup on disconnect).
    by_connection: DashMap<u64, Nickname>,
    /// Current connection count (atomic for fast reads).
    connection_count: AtomicUsize,
}

impl UserRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            by_nick: DashMap::new(),
            by_connection: DashMap::new(),
            connection_count: AtomicUsize::new(0),
        }
    }

    /// Register a new user session.
    ///
    /// Inserts the session if the nickname is not already taken.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::NickInUse`] if a session with the same nickname
    /// (case-insensitive) is already registered.
    pub fn register(&self, session: UserSession) -> Result<(), UserError> {
        let nick = session.nickname.clone();
        let conn_id = session.connection_id;

        // Check-and-insert must be done without a race. DashMap's entry API
        // holds a shard lock, so no other thread can insert the same nick
        // between our check and our insert.
        match self.by_nick.entry(nick.clone()) {
            Entry::Occupied(_) => {
                return Err(UserError::NickInUse {
                    nick: nick.to_string(),
                });
            }
            Entry::Vacant(slot) => {
                slot.insert(Arc::new(RwLock::new(session)));
            }
        }

        self.by_connection.insert(conn_id, nick);
        self.connection_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Remove a user by connection ID (e.g. on disconnect).
    ///
    /// Returns the removed session if found.
    pub fn remove_by_connection(&self, connection_id: u64) -> Option<Arc<RwLock<UserSession>>> {
        let (_, nick) = self.by_connection.remove(&connection_id)?;
        let (_, session) = self.by_nick.remove(&nick)?;
        self.connection_count.fetch_sub(1, Ordering::Relaxed);
        Some(session)
    }

    /// Look up a session by nickname (case-insensitive).
    pub fn get_by_nick(&self, nick: &Nickname) -> Option<Arc<RwLock<UserSession>>> {
        self.by_nick.get(nick).map(|r| Arc::clone(r.value()))
    }

    /// Look up a session by connection ID.
    pub fn get_by_connection(&self, connection_id: u64) -> Option<Arc<RwLock<UserSession>>> {
        let nick = self.by_connection.get(&connection_id)?;
        self.by_nick
            .get(nick.value())
            .map(|r| Arc::clone(r.value()))
    }

    /// Check whether a nickname is already in use without locking any session.
    pub fn nick_in_use(&self, nick: &Nickname) -> bool {
        self.by_nick.contains_key(nick)
    }

    /// Atomically change a user's nickname.
    ///
    /// The old nick is removed and the new nick is inserted while the session
    /// remains accessible — there is no window where the session is unmapped.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::NickInUse`] if the new nickname is already taken.
    /// Returns [`UserError::NotFound`] if the old nickname does not exist.
    pub fn change_nick(&self, old: &Nickname, new: Nickname) -> Result<(), UserError> {
        // If old == new (case-insensitive), just update the casing in the session.
        if old == &new {
            if let Some(session_arc) = self.get_by_nick(old) {
                let mut session = session_arc.write().expect("session lock poisoned");
                session.nickname = new;
            }
            return Ok(());
        }

        // Ensure new nick is not taken.
        if self.by_nick.contains_key(&new) {
            return Err(UserError::NickInUse {
                nick: new.to_string(),
            });
        }

        // Remove old entry.
        let (_, session) = self
            .by_nick
            .remove(old)
            .ok_or_else(|| UserError::NotFound {
                nick: old.to_string(),
            })?;

        // Update the nickname inside the session.
        {
            let mut s = session.write().expect("session lock poisoned");
            s.nickname = new.clone();
            // Update the reverse lookup.
            self.by_connection.insert(s.connection_id, new.clone());
        }

        // Insert under the new nick.
        self.by_nick.insert(new, session);

        Ok(())
    }

    /// Number of currently connected users.
    pub fn connection_count(&self) -> usize {
        self.connection_count.load(Ordering::Relaxed)
    }

    /// Iterate over all sessions.
    ///
    /// Useful for broadcast operations. The returned iterator holds shard
    /// locks briefly for each element.
    pub fn iter_sessions(&self) -> impl Iterator<Item = Arc<RwLock<UserSession>>> + use<'_> {
        self.by_nick.iter().map(|r| Arc::clone(r.value()))
    }
}

impl Default for UserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pirc_common::{Nickname, UserMode};
    use pirc_protocol::Message;
    use tokio::sync::mpsc;
    use tokio::time::Instant;

    use super::*;
    use crate::user::UserSession;

    /// Helper: create a UserSession with reasonable defaults.
    fn make_session(conn_id: u64, nick: &str) -> (UserSession, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let now = Instant::now();
        let session = UserSession {
            connection_id: conn_id,
            nickname: Nickname::new(nick).unwrap(),
            username: format!("user{conn_id}"),
            realname: format!("Real Name {conn_id}"),
            hostname: "127.0.0.1".to_owned(),
            modes: HashSet::new(),
            away_message: None,
            connected_at: now,
            signon_time: 0,
            last_active: now,
            registered: false,
            sender: tx,
        };
        (session, rx)
    }

    #[test]
    fn register_and_lookup_by_nick() {
        let registry = UserRegistry::new();
        let (session, _rx) = make_session(1, "Alice");
        registry.register(session).unwrap();

        let lookup = Nickname::new("Alice").unwrap();
        let found = registry.get_by_nick(&lookup).unwrap();
        let s = found.read().unwrap();
        assert_eq!(s.connection_id, 1);
        assert_eq!(s.nickname, Nickname::new("Alice").unwrap());
    }

    #[test]
    fn nick_collision_returns_error() {
        let registry = UserRegistry::new();
        let (s1, _rx1) = make_session(1, "Alice");
        let (s2, _rx2) = make_session(2, "alice");

        registry.register(s1).unwrap();
        let err = registry.register(s2).unwrap_err();
        assert!(matches!(err, UserError::NickInUse { .. }));
    }

    #[test]
    fn case_insensitive_lookup() {
        let registry = UserRegistry::new();
        let (session, _rx) = make_session(1, "Nick");
        registry.register(session).unwrap();

        // All case variants should find the same session.
        for variant in &["Nick", "nick", "NICK", "nIcK"] {
            let lookup = Nickname::new(variant).unwrap();
            assert!(
                registry.get_by_nick(&lookup).is_some(),
                "lookup failed for '{variant}'"
            );
        }
    }

    #[test]
    fn nick_in_use_check() {
        let registry = UserRegistry::new();
        let nick = Nickname::new("Alice").unwrap();
        assert!(!registry.nick_in_use(&nick));

        let (session, _rx) = make_session(1, "Alice");
        registry.register(session).unwrap();
        assert!(registry.nick_in_use(&nick));

        let lower = Nickname::new("alice").unwrap();
        assert!(registry.nick_in_use(&lower));
    }

    #[test]
    fn remove_by_connection_cleans_both_maps() {
        let registry = UserRegistry::new();
        let (session, _rx) = make_session(1, "Alice");
        registry.register(session).unwrap();
        assert_eq!(registry.connection_count(), 1);

        let removed = registry.remove_by_connection(1).unwrap();
        let s = removed.read().unwrap();
        assert_eq!(s.connection_id, 1);
        drop(s);

        // Both maps should be empty now.
        let lookup = Nickname::new("Alice").unwrap();
        assert!(registry.get_by_nick(&lookup).is_none());
        assert_eq!(registry.connection_count(), 0);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let registry = UserRegistry::new();
        assert!(registry.remove_by_connection(999).is_none());
    }

    #[test]
    fn change_nick_updates_both_maps() {
        let registry = UserRegistry::new();
        let (session, _rx) = make_session(1, "Alice");
        registry.register(session).unwrap();

        let old = Nickname::new("Alice").unwrap();
        let new = Nickname::new("Bob").unwrap();
        registry.change_nick(&old, new).unwrap();

        // Old nick should be gone.
        let alice = Nickname::new("Alice").unwrap();
        assert!(registry.get_by_nick(&alice).is_none());

        // New nick should work.
        let bob = Nickname::new("Bob").unwrap();
        let found = registry.get_by_nick(&bob).unwrap();
        let s = found.read().unwrap();
        assert_eq!(s.nickname, bob);
        assert_eq!(s.connection_id, 1);
    }

    #[test]
    fn change_nick_collision_returns_error() {
        let registry = UserRegistry::new();
        let (s1, _rx1) = make_session(1, "Alice");
        let (s2, _rx2) = make_session(2, "Bob");
        registry.register(s1).unwrap();
        registry.register(s2).unwrap();

        let old = Nickname::new("Alice").unwrap();
        let new = Nickname::new("Bob").unwrap();
        let err = registry.change_nick(&old, new).unwrap_err();
        assert!(matches!(err, UserError::NickInUse { .. }));

        // Alice should still be there after the failed change.
        let alice = Nickname::new("Alice").unwrap();
        assert!(registry.get_by_nick(&alice).is_some());
    }

    #[test]
    fn change_nick_not_found_returns_error() {
        let registry = UserRegistry::new();
        let old = Nickname::new("Ghost").unwrap();
        let new = Nickname::new("Bob").unwrap();
        let err = registry.change_nick(&old, new).unwrap_err();
        assert!(matches!(err, UserError::NotFound { .. }));
    }

    #[test]
    fn change_nick_same_case_update() {
        let registry = UserRegistry::new();
        let (session, _rx) = make_session(1, "alice");
        registry.register(session).unwrap();

        let old = Nickname::new("alice").unwrap();
        let new = Nickname::new("Alice").unwrap();
        registry.change_nick(&old, new).unwrap();

        // Should still be findable.
        let lookup = Nickname::new("alice").unwrap();
        let found = registry.get_by_nick(&lookup).unwrap();
        let s = found.read().unwrap();
        // Display casing should be updated.
        assert_eq!(s.nickname.to_string(), "Alice");
    }

    #[test]
    fn connection_count_tracks_additions_and_removals() {
        let registry = UserRegistry::new();
        assert_eq!(registry.connection_count(), 0);

        let (s1, _rx1) = make_session(1, "Alice");
        let (s2, _rx2) = make_session(2, "Bob");
        let (s3, _rx3) = make_session(3, "Carol");

        registry.register(s1).unwrap();
        assert_eq!(registry.connection_count(), 1);

        registry.register(s2).unwrap();
        registry.register(s3).unwrap();
        assert_eq!(registry.connection_count(), 3);

        registry.remove_by_connection(2);
        assert_eq!(registry.connection_count(), 2);

        registry.remove_by_connection(1);
        registry.remove_by_connection(3);
        assert_eq!(registry.connection_count(), 0);
    }

    #[test]
    fn iter_sessions_returns_all() {
        let registry = UserRegistry::new();
        let (s1, _rx1) = make_session(1, "Alice");
        let (s2, _rx2) = make_session(2, "Bob");
        let (s3, _rx3) = make_session(3, "Carol");

        registry.register(s1).unwrap();
        registry.register(s2).unwrap();
        registry.register(s3).unwrap();

        let mut conn_ids: Vec<u64> = registry
            .iter_sessions()
            .map(|s| s.read().unwrap().connection_id)
            .collect();
        conn_ids.sort();
        assert_eq!(conn_ids, vec![1, 2, 3]);
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = UserRegistry::default();
        assert_eq!(registry.connection_count(), 0);
    }

    #[test]
    fn register_preserves_all_fields() {
        let registry = UserRegistry::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let now = Instant::now();
        let mut modes = HashSet::new();
        modes.insert(UserMode::Operator);

        let session = UserSession {
            connection_id: 42,
            nickname: Nickname::new("TestUser").unwrap(),
            username: "tuser".to_owned(),
            realname: "Test User".to_owned(),
            hostname: "10.0.0.1".to_owned(),
            modes: modes.clone(),
            away_message: Some("BRB".to_owned()),
            connected_at: now,
            signon_time: 1700000000,
            last_active: now,
            registered: true,
            sender: tx,
        };

        registry.register(session).unwrap();

        let nick = Nickname::new("TestUser").unwrap();
        let found = registry.get_by_nick(&nick).unwrap();
        let s = found.read().unwrap();
        assert_eq!(s.connection_id, 42);
        assert_eq!(s.username, "tuser");
        assert_eq!(s.realname, "Test User");
        assert_eq!(s.hostname, "10.0.0.1");
        assert_eq!(s.modes, modes);
        assert_eq!(s.away_message.as_deref(), Some("BRB"));
        assert!(s.registered);
    }
}
