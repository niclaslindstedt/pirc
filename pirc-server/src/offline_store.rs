use std::time::{Duration, Instant};

use dashmap::DashMap;
use pirc_common::Nickname;
use pirc_protocol::Message;

/// A queued offline message with metadata for expiry and size tracking.
struct QueuedMessage {
    message: Message,
    queued_at: Instant,
    /// Approximate size in bytes (serialized wire length).
    size: usize,
}

/// Thread-safe in-memory store for messages sent to offline users.
///
/// Messages are keyed by nickname (case-insensitive via [`Nickname`]) and
/// stored in order. The store enforces per-user message count limits, a
/// global byte-size cap, and message expiry.
pub struct OfflineMessageStore {
    queues: DashMap<Nickname, Vec<QueuedMessage>>,
    /// Maximum messages stored per user.
    max_messages_per_user: usize,
    /// Maximum total bytes across all queued messages.
    max_total_bytes: usize,
    /// Messages older than this are discarded on access.
    message_ttl: Duration,
    /// Current approximate total bytes stored (atomic via `DashMap` shard locking).
    total_bytes: std::sync::atomic::AtomicUsize,
}

impl OfflineMessageStore {
    pub fn new(max_messages_per_user: usize, max_total_bytes: usize, message_ttl: Duration) -> Self {
        Self {
            queues: DashMap::new(),
            max_messages_per_user,
            max_total_bytes,
            message_ttl,
            total_bytes: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Queue a message for delivery to `nick` when they reconnect.
    ///
    /// Returns `true` if the message was stored, `false` if it was dropped
    /// due to limits being exceeded.
    pub fn queue_message(&self, nick: &Nickname, message: Message) -> bool {
        let size = estimate_message_size(&message);

        // Check global size limit.
        let current_total = self.total_bytes.load(std::sync::atomic::Ordering::Relaxed);
        if current_total + size > self.max_total_bytes {
            return false;
        }

        let mut entry = self.queues.entry(nick.clone()).or_default();
        let queue = entry.value_mut();

        // Purge expired messages first.
        let now = Instant::now();
        let before_len = queue.len();
        let mut freed = 0usize;
        queue.retain(|qm| {
            if now.duration_since(qm.queued_at) < self.message_ttl {
                true
            } else {
                freed += qm.size;
                false
            }
        });
        if freed > 0 {
            self.total_bytes
                .fetch_sub(freed, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = before_len; // suppress unused warning

        // Check per-user count limit.
        if queue.len() >= self.max_messages_per_user {
            return false;
        }

        self.total_bytes
            .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
        queue.push(QueuedMessage {
            message,
            queued_at: now,
            size,
        });

        true
    }

    /// Take all queued messages for `nick`, removing them from the store.
    ///
    /// Expired messages are filtered out. The returned messages are in the
    /// order they were queued (FIFO).
    pub fn take_messages(&self, nick: &Nickname) -> Vec<Message> {
        let Some((_, queue)) = self.queues.remove(nick) else {
            return Vec::new();
        };

        let now = Instant::now();
        let mut messages = Vec::new();
        let mut freed = 0usize;

        for qm in queue {
            if now.duration_since(qm.queued_at) < self.message_ttl {
                messages.push(qm.message);
            }
            freed += qm.size;
        }

        self.total_bytes
            .fetch_sub(freed, std::sync::atomic::Ordering::Relaxed);

        messages
    }

    /// Number of users with queued messages.
    #[cfg(test)]
    pub fn user_count(&self) -> usize {
        self.queues.len()
    }

    /// Total messages across all users (including possibly expired ones).
    #[cfg(test)]
    pub fn total_message_count(&self) -> usize {
        self.queues
            .iter()
            .map(|entry| entry.value().len())
            .sum()
    }
}

impl Default for OfflineMessageStore {
    fn default() -> Self {
        Self::new(100, 10 * 1024 * 1024, Duration::from_secs(7 * 24 * 3600))
    }
}

/// Estimate the wire size of a message for quota tracking.
fn estimate_message_size(msg: &Message) -> usize {
    // Prefix + command + params. Use the Display representation as an approximation.
    let s = msg.to_string();
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirc_protocol::{Command, PircSubcommand};

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    fn encrypted_msg(target: &str, payload: &str) -> Message {
        Message::new(
            Command::Pirc(PircSubcommand::Encrypted),
            vec![target.to_owned(), payload.to_owned()],
        )
    }

    fn keyexchange_msg(target: &str, data: &str) -> Message {
        Message::new(
            Command::Pirc(PircSubcommand::KeyExchange),
            vec![target.to_owned(), data.to_owned()],
        )
    }

    #[test]
    fn queue_and_take_messages() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "hello")));
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "world")));

        let msgs = store.take_messages(&bob);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].params[1], "hello");
        assert_eq!(msgs[1].params[1], "world");

        // Queue is cleared after take.
        assert!(store.take_messages(&bob).is_empty());
    }

    #[test]
    fn take_from_empty_returns_empty() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");
        assert!(store.take_messages(&bob).is_empty());
    }

    #[test]
    fn per_user_limit_enforced() {
        let store = OfflineMessageStore::new(3, 10 * 1024 * 1024, Duration::from_secs(3600));
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "1")));
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "2")));
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "3")));
        // Fourth message should be rejected.
        assert!(!store.queue_message(&bob, encrypted_msg("Bob", "4")));

        let msgs = store.take_messages(&bob);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn global_size_limit_enforced() {
        // Tiny global limit.
        let store = OfflineMessageStore::new(100, 50, Duration::from_secs(3600));
        let bob = nick("Bob");

        // First message should fit.
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "short")));
        // Eventually hit the limit with a large payload.
        let large = "x".repeat(200);
        assert!(!store.queue_message(&bob, encrypted_msg("Bob", &large)));
    }

    #[test]
    fn expired_messages_purged_on_queue() {
        let store = OfflineMessageStore::new(3, 10 * 1024 * 1024, Duration::from_millis(1));
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "old")));

        // Wait for the message to expire.
        std::thread::sleep(Duration::from_millis(5));

        // Queuing a new message should purge expired ones, freeing a slot.
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "new")));

        let msgs = store.take_messages(&bob);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].params[1], "new");
    }

    #[test]
    fn expired_messages_filtered_on_take() {
        let store = OfflineMessageStore::new(100, 10 * 1024 * 1024, Duration::from_millis(1));
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "old")));
        std::thread::sleep(Duration::from_millis(5));

        let msgs = store.take_messages(&bob);
        assert!(msgs.is_empty());
    }

    #[test]
    fn case_insensitive_nickname() {
        let store = OfflineMessageStore::default();
        let bob_upper = nick("Bob");
        let bob_lower = nick("bob");

        assert!(store.queue_message(&bob_upper, encrypted_msg("Bob", "hello")));

        let msgs = store.take_messages(&bob_lower);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn multiple_users_independent() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");
        let carol = nick("Carol");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "for-bob")));
        assert!(store.queue_message(&carol, encrypted_msg("Carol", "for-carol")));

        let bob_msgs = store.take_messages(&bob);
        assert_eq!(bob_msgs.len(), 1);
        assert_eq!(bob_msgs[0].params[1], "for-bob");

        let carol_msgs = store.take_messages(&carol);
        assert_eq!(carol_msgs.len(), 1);
        assert_eq!(carol_msgs[0].params[1], "for-carol");
    }

    #[test]
    fn fifo_ordering_preserved() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");

        for i in 0..10 {
            assert!(store.queue_message(&bob, encrypted_msg("Bob", &i.to_string())));
        }

        let msgs = store.take_messages(&bob);
        assert_eq!(msgs.len(), 10);
        for (i, msg) in msgs.iter().enumerate() {
            assert_eq!(msg.params[1], i.to_string());
        }
    }

    #[test]
    fn keyexchange_and_encrypted_interleaved() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, keyexchange_msg("Bob", "ke-data")));
        assert!(store.queue_message(&bob, encrypted_msg("Bob", "enc-data")));

        let msgs = store.take_messages(&bob);
        assert_eq!(msgs.len(), 2);
        assert!(matches!(
            msgs[0].command,
            Command::Pirc(PircSubcommand::KeyExchange)
        ));
        assert!(matches!(
            msgs[1].command,
            Command::Pirc(PircSubcommand::Encrypted)
        ));
    }

    #[test]
    fn total_bytes_tracked() {
        let store = OfflineMessageStore::default();
        let bob = nick("Bob");

        assert!(store.queue_message(&bob, encrypted_msg("Bob", "hello")));
        let bytes_after_queue = store.total_bytes.load(std::sync::atomic::Ordering::Relaxed);
        assert!(bytes_after_queue > 0);

        store.take_messages(&bob);
        let bytes_after_take = store.total_bytes.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(bytes_after_take, 0);
    }

    #[test]
    fn default_limits() {
        let store = OfflineMessageStore::default();
        assert_eq!(store.max_messages_per_user, 100);
        assert_eq!(store.max_total_bytes, 10 * 1024 * 1024);
        assert_eq!(store.message_ttl, Duration::from_secs(7 * 24 * 3600));
    }
}
