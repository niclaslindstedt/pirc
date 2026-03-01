use std::sync::Arc;
use std::time::Duration;

use pirc_protocol::{Command, Message};
use tokio::sync::RwLock;

use super::{spawn_failover_expiry_task, FailoverMessageQueue, SharedFailoverQueue};

fn make_msg(text: &str) -> Message {
    Message::builder(Command::Privmsg)
        .param("target")
        .trailing(text)
        .build()
}

fn new_queue(max: usize, ttl_secs: u64) -> FailoverMessageQueue {
    FailoverMessageQueue::new(max, Duration::from_secs(ttl_secs))
}

fn new_shared(max: usize, ttl_secs: u64) -> SharedFailoverQueue {
    Arc::new(RwLock::new(new_queue(max, ttl_secs)))
}

// ---- Happy paths ----

#[test]
fn enqueue_single_user_drain() {
    let mut q = new_queue(10, 60);
    assert!(q.enqueue("alice", make_msg("hello")));
    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].params[1], "hello");
}

#[test]
fn multiple_messages_fifo_order() {
    let mut q = new_queue(100, 60);
    for i in 0..50 {
        q.enqueue("alice", make_msg(&format!("m{i}")));
    }
    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 50);
    for (i, msg) in msgs.iter().enumerate() {
        assert_eq!(msg.params[1], format!("m{i}"));
    }
}

#[test]
fn multiple_users_independent() {
    let mut q = new_queue(100, 60);
    q.enqueue("alice", make_msg("a1"));
    q.enqueue("alice", make_msg("a2"));
    q.enqueue("bob", make_msg("b1"));

    assert_eq!(q.message_count("alice"), 2);
    assert_eq!(q.message_count("bob"), 1);

    let alice = q.drain("alice");
    assert_eq!(alice.len(), 2);
    assert_eq!(alice[0].params[1], "a1");

    let bob = q.drain("bob");
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0].params[1], "b1");
}

#[test]
fn drain_clears_queue() {
    let mut q = new_queue(10, 60);
    q.enqueue("alice", make_msg("hi"));
    assert_eq!(q.user_count(), 1);
    q.drain("alice");
    assert_eq!(q.user_count(), 0);
    assert_eq!(q.message_count("alice"), 0);
}

#[test]
fn remove_clears_user() {
    let mut q = new_queue(10, 60);
    q.enqueue("alice", make_msg("hi"));
    q.remove("alice");
    assert_eq!(q.message_count("alice"), 0);
    assert_eq!(q.user_count(), 0);
}

#[test]
fn rename_moves_messages_preserving_order() {
    let mut q = new_queue(10, 60);
    q.enqueue("alice", make_msg("x"));
    q.enqueue("alice", make_msg("y"));
    q.rename("alice", "alicia");

    assert_eq!(q.message_count("alice"), 0);
    assert_eq!(q.message_count("alicia"), 2);

    let msgs = q.drain("alicia");
    assert_eq!(msgs[0].params[1], "x");
    assert_eq!(msgs[1].params[1], "y");
}

#[test]
fn case_insensitive_all_ops() {
    let mut q = new_queue(10, 60);
    q.enqueue("ALICE", make_msg("hi"));
    assert_eq!(q.message_count("alice"), 1);
    assert_eq!(q.message_count("Alice"), 1);
    assert_eq!(q.message_count("ALICE"), 1);

    let msgs = q.drain("Alice");
    assert_eq!(msgs.len(), 1);
    assert_eq!(q.message_count("alice"), 0);
}

// ---- Edge cases ----

#[test]
fn drain_nonexistent_returns_empty() {
    let mut q = new_queue(10, 60);
    assert!(q.drain("nobody").is_empty());
}

#[test]
fn double_drain_second_returns_empty() {
    let mut q = new_queue(10, 60);
    q.enqueue("alice", make_msg("hi"));
    assert_eq!(q.drain("alice").len(), 1);
    assert!(q.drain("alice").is_empty());
}

#[test]
fn message_count_nonexistent_returns_zero() {
    let q = new_queue(10, 60);
    assert_eq!(q.message_count("nobody"), 0);
}

#[test]
fn rename_to_same_nick_is_noop() {
    let mut q = new_queue(10, 60);
    q.enqueue("alice", make_msg("hi"));
    q.rename("alice", "Alice"); // same nick, different case
    assert_eq!(q.message_count("alice"), 1);
    assert_eq!(q.user_count(), 1);
}

#[test]
fn rename_collision_merges_queues() {
    let mut q = new_queue(100, 60);
    // new_nick already has messages
    q.enqueue("bob", make_msg("b1"));
    q.enqueue("bob", make_msg("b2"));
    // old_nick has messages
    q.enqueue("alice", make_msg("a1"));
    q.enqueue("alice", make_msg("a2"));

    // rename alice → bob: bob's existing messages come first, then alice's
    q.rename("alice", "bob");

    assert_eq!(q.message_count("alice"), 0);
    assert_eq!(q.message_count("bob"), 4);
    assert_eq!(q.user_count(), 1);

    let msgs = q.drain("bob");
    assert_eq!(msgs[0].params[1], "b1");
    assert_eq!(msgs[1].params[1], "b2");
    assert_eq!(msgs[2].params[1], "a1");
    assert_eq!(msgs[3].params[1], "a2");
}

#[test]
fn rename_nonexistent_nick_is_noop() {
    let mut q = new_queue(10, 60);
    q.enqueue("bob", make_msg("hi"));
    q.rename("alice", "bob"); // alice has no queue
    assert_eq!(q.message_count("bob"), 1);
    assert_eq!(q.user_count(), 1);
}

#[test]
fn capacity_evicts_oldest() {
    let mut q = new_queue(3, 60);
    assert!(q.enqueue("alice", make_msg("m1")));
    assert!(q.enqueue("alice", make_msg("m2")));
    assert!(q.enqueue("alice", make_msg("m3")));
    assert!(!q.enqueue("alice", make_msg("m4"))); // evicts m1

    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].params[1], "m2");
    assert_eq!(msgs[1].params[1], "m3");
    assert_eq!(msgs[2].params[1], "m4");
}

#[test]
fn ttl_expiry_on_drain() {
    let mut q = FailoverMessageQueue::new(100, Duration::from_millis(50));
    q.enqueue("alice", make_msg("old"));
    std::thread::sleep(Duration::from_millis(70));
    q.enqueue("alice", make_msg("new"));

    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].params[1], "new");
}

#[test]
fn ttl_boundary_just_within() {
    let mut q = FailoverMessageQueue::new(100, Duration::from_millis(200));
    q.enqueue("alice", make_msg("fresh"));
    // Don't sleep — message should survive
    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].params[1], "fresh");
}

#[test]
fn expire_all_removes_stale_cleans_empty_queues() {
    let mut q = FailoverMessageQueue::new(100, Duration::from_millis(50));
    q.enqueue("alice", make_msg("old1"));
    q.enqueue("bob", make_msg("old2"));
    std::thread::sleep(Duration::from_millis(70));
    q.enqueue("alice", make_msg("fresh"));

    q.expire_all();

    assert_eq!(q.user_count(), 1);
    assert_eq!(q.message_count("alice"), 1);
    assert_eq!(q.message_count("bob"), 0);
}

#[test]
fn expire_all_idempotent() {
    let mut q = FailoverMessageQueue::new(100, Duration::from_millis(50));
    q.enqueue("alice", make_msg("hi"));
    std::thread::sleep(Duration::from_millis(70));

    q.expire_all();
    q.expire_all(); // second call must not panic
    assert_eq!(q.user_count(), 0);
}

#[test]
fn expire_all_then_drain_returns_empty() {
    let mut q = FailoverMessageQueue::new(100, Duration::from_millis(50));
    q.enqueue("alice", make_msg("hi"));
    std::thread::sleep(Duration::from_millis(70));

    q.expire_all();
    assert!(q.drain("alice").is_empty());
}

#[test]
fn user_count_tracks_correctly() {
    let mut q = new_queue(10, 60);
    assert_eq!(q.user_count(), 0);

    q.enqueue("alice", make_msg("a"));
    q.enqueue("bob", make_msg("b"));
    assert_eq!(q.user_count(), 2);

    q.drain("alice");
    assert_eq!(q.user_count(), 1);

    q.remove("bob");
    assert_eq!(q.user_count(), 0);

    // expire_all on fresh message should leave user count at 1
    q.enqueue("carol", make_msg("c"));
    q.expire_all();
    assert_eq!(q.user_count(), 1);
}

#[test]
fn enqueue_return_value_false_on_eviction() {
    let mut q = new_queue(2, 60);
    assert!(q.enqueue("alice", make_msg("m1")));
    assert!(q.enqueue("alice", make_msg("m2")));
    // At capacity — next enqueue evicts oldest, returns false
    assert!(!q.enqueue("alice", make_msg("m3")));
}

#[test]
fn zero_capacity_documents_quirk() {
    // With max_per_user=0: len()==0 >= 0 triggers pop_front (no-op on empty),
    // then push_back — so the message is actually stored. This is a known quirk
    // since 0 is not a realistic config value.
    let mut q = new_queue(0, 60);
    assert!(!q.enqueue("alice", make_msg("hi")));
    let msgs = q.drain("alice");
    assert_eq!(msgs.len(), 1);
}

// ---- Concurrency (async) ----

#[tokio::test]
async fn concurrent_enqueue_from_multiple_tasks() {
    let shared = new_shared(200, 60);
    let mut handles = Vec::new();

    for user_idx in 0..10u32 {
        for _ in 0..10 {
            let q = Arc::clone(&shared);
            let nick = format!("user{user_idx}");
            handles.push(tokio::spawn(async move {
                let mut guard = q.write().await;
                guard.enqueue(&nick, make_msg("msg"));
            }));
        }
    }

    for h in handles {
        h.await.unwrap();
    }

    let guard = shared.read().await;
    assert_eq!(guard.user_count(), 10);
    for i in 0..10u32 {
        assert_eq!(guard.message_count(&format!("user{i}")), 10);
    }
}

#[tokio::test]
async fn concurrent_enqueue_and_drain_no_panic() {
    let shared = new_shared(50, 60);

    // Pre-load some messages
    {
        let mut q = shared.write().await;
        for i in 0..20 {
            q.enqueue("alice", make_msg(&format!("pre{i}")));
        }
    }

    let mut handles = Vec::new();

    // Enqueuers
    for i in 0..5 {
        let q = Arc::clone(&shared);
        handles.push(tokio::spawn(async move {
            let mut guard = q.write().await;
            guard.enqueue("alice", make_msg(&format!("new{i}")));
        }));
    }

    // Drainers
    for _ in 0..3 {
        let q = Arc::clone(&shared);
        handles.push(tokio::spawn(async move {
            let mut guard = q.write().await;
            let _ = guard.drain("alice");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // No panic = success; queue state is coherent (whatever was left is valid)
    let guard = shared.read().await;
    let _ = guard.message_count("alice");
}

// ---- Background expiry task ----

#[tokio::test]
async fn spawn_expiry_task_expires_messages() {
    let shared = new_shared(100, 60);

    // Override TTL with a very short one
    let shared_short = Arc::new(RwLock::new(FailoverMessageQueue::new(
        100,
        Duration::from_millis(80),
    )));

    {
        let mut q = shared_short.write().await;
        q.enqueue("alice", make_msg("will expire"));
        q.enqueue("bob", make_msg("also expires"));
    }

    let handle = spawn_failover_expiry_task(Arc::clone(&shared_short), Duration::from_millis(50));

    // Wait for: TTL (80ms) + at least one interval tick (50ms) + buffer
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();

    let q = shared_short.read().await;
    assert_eq!(q.user_count(), 0, "all messages should have been expired");
    assert_eq!(q.message_count("alice"), 0);
    assert_eq!(q.message_count("bob"), 0);

    // Suppress unused variable warning
    drop(shared);
}

#[tokio::test]
async fn spawn_expiry_task_abort_is_clean() {
    let shared = new_shared(10, 60);

    {
        let mut q = shared.write().await;
        q.enqueue("alice", make_msg("hi"));
    }

    let handle = spawn_failover_expiry_task(Arc::clone(&shared), Duration::from_secs(60));
    handle.abort();
    // Give the runtime a moment to process the abort
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Queue must still be usable after abort
    let mut q = shared.write().await;
    q.enqueue("bob", make_msg("still works"));
    assert_eq!(q.message_count("bob"), 1);
    assert_eq!(q.message_count("alice"), 1);
}
