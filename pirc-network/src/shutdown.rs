//! Graceful shutdown coordination.
//!
//! Provides [`ShutdownController`] and [`ShutdownSignal`] for cooperative
//! shutdown of listeners, connections, and pools. Built on top of
//! [`tokio::sync::broadcast`] so that a single controller can notify an
//! arbitrary number of signal holders.

use tokio::sync::broadcast;
use tracing::{debug, info};

/// Controls the shutdown process by sending a signal to all [`ShutdownSignal`]
/// holders.
///
/// There is exactly one controller per shutdown group. Calling
/// [`ShutdownController::shutdown`] notifies every outstanding
/// [`ShutdownSignal`] that it is time to stop.
#[derive(Debug)]
pub struct ShutdownController {
    tx: broadcast::Sender<()>,
}

impl ShutdownController {
    /// Trigger shutdown for all associated [`ShutdownSignal`] instances.
    pub fn shutdown(&self) {
        // It's OK if there are no active receivers — the signal has still been
        // "sent" in the logical sense.
        let n = self.tx.send(()).unwrap_or(0);
        info!(receivers = n, "shutdown signal sent");
    }
}

/// A cloneable handle that resolves when a shutdown has been requested.
///
/// Obtain a `ShutdownSignal` via [`ShutdownSignal::new`] and distribute clones
/// to any tasks that need to participate in graceful shutdown.
#[derive(Debug)]
pub struct ShutdownSignal {
    rx: broadcast::Receiver<()>,
    /// Keep a clone of the sender so we can hand out new receivers via
    /// `Clone`, and so we can detect shutdown even if the receiver was
    /// created after `shutdown()` was called (by checking `tx.subscribe()`).
    tx: broadcast::Sender<()>,
    shutdown: bool,
}

impl ShutdownSignal {
    /// Create a new shutdown group, returning the controller and the first
    /// signal handle.
    pub fn new() -> (ShutdownController, ShutdownSignal) {
        // A capacity of 1 is sufficient — we only ever send a single unit
        // value. If a receiver is lagging it will still observe the closed
        // channel on the next poll, which we treat the same as receiving the
        // message.
        let (tx, rx) = broadcast::channel(1);
        debug!("shutdown group created");
        (
            ShutdownController { tx: tx.clone() },
            ShutdownSignal {
                rx,
                tx,
                shutdown: false,
            },
        )
    }

    /// Wait until shutdown is signaled.
    ///
    /// This future completes when [`ShutdownController::shutdown`] is called.
    /// If shutdown was already triggered before this method is called, it
    /// returns immediately.
    pub async fn recv(&mut self) {
        if self.shutdown {
            return;
        }

        // Either we receive the `()` value, or we get `RecvError::Closed`
        // (sender dropped) / `RecvError::Lagged` (missed the message). All
        // three cases mean "shutdown has happened".
        let _ = self.rx.recv().await;
        self.shutdown = true;
    }

    /// Non-blocking check for whether shutdown has been signaled.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }
}

impl Clone for ShutdownSignal {
    fn clone(&self) -> Self {
        // Subscribe to a fresh receiver from the sender, preserving the
        // shutdown flag.
        Self {
            rx: self.tx.subscribe(),
            tx: self.tx.clone(),
            shutdown: self.shutdown,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn signal_recv_completes_on_shutdown() {
        let (controller, mut signal) = ShutdownSignal::new();
        assert!(!signal.is_shutdown());

        controller.shutdown();
        signal.recv().await;

        assert!(signal.is_shutdown());
    }

    #[tokio::test]
    async fn multiple_clones_all_receive_signal() {
        let (controller, signal) = ShutdownSignal::new();

        let mut s1 = signal.clone();
        let mut s2 = signal.clone();
        let mut s3 = signal.clone();
        drop(signal);

        controller.shutdown();

        s1.recv().await;
        s2.recv().await;
        s3.recv().await;

        assert!(s1.is_shutdown());
        assert!(s2.is_shutdown());
        assert!(s3.is_shutdown());
    }

    #[tokio::test]
    async fn recv_returns_immediately_if_already_shutdown() {
        let (controller, mut signal) = ShutdownSignal::new();
        controller.shutdown();
        signal.recv().await;

        // Second call should return immediately
        let result = tokio::time::timeout(Duration::from_millis(50), signal.recv()).await;
        assert!(result.is_ok(), "recv should return immediately");
    }

    #[tokio::test]
    async fn is_shutdown_false_before_signal() {
        let (_controller, signal) = ShutdownSignal::new();
        assert!(!signal.is_shutdown());
    }

    #[tokio::test]
    async fn shutdown_with_no_receivers() {
        let (controller, signal) = ShutdownSignal::new();
        drop(signal);
        // Should not panic
        controller.shutdown();
    }

    #[tokio::test]
    async fn clone_after_shutdown_is_already_shutdown() {
        let (controller, mut signal) = ShutdownSignal::new();
        controller.shutdown();
        signal.recv().await;

        let cloned = signal.clone();
        assert!(cloned.is_shutdown());
    }

    #[tokio::test]
    async fn concurrent_shutdown_signals() {
        let (controller, signal) = ShutdownSignal::new();

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let mut s = signal.clone();
                tokio::spawn(async move {
                    s.recv().await;
                    assert!(s.is_shutdown());
                })
            })
            .collect();

        // Give tasks time to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        controller.shutdown();

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
