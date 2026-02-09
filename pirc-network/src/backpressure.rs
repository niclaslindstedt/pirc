//! Backpressure handling and flow control.
//!
//! Provides mechanisms to prevent fast producers from overwhelming slow consumers:
//!
//! - [`WriteConfig`] — configurable high/low water marks for write buffering
//! - [`BackpressureController`] — tracks write buffer state and signals when
//!   backpressure should be applied or released
//! - [`ReadLimiter`] — bounds the number of unprocessed inbound messages
//! - [`BoundedChannel`] — bounded async message-passing channel

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{info, trace};

use crate::error::NetworkError;

// ---------------------------------------------------------------------------
// WriteConfig
// ---------------------------------------------------------------------------

/// Configuration for write-side backpressure.
///
/// When the number of buffered outbound bytes exceeds [`high_water_mark`],
/// the connection signals that it is not ready for more writes. Once the
/// buffer drains below [`low_water_mark`], the connection becomes writable
/// again.
///
/// The hysteresis between the two marks avoids rapid toggling when the buffer
/// size fluctuates around a single threshold.
#[derive(Debug, Clone, Copy)]
pub struct WriteConfig {
    /// Buffer size (in bytes) at which backpressure engages.
    pub high_water_mark: usize,
    /// Buffer size (in bytes) at which backpressure disengages.
    pub low_water_mark: usize,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            high_water_mark: 64 * 1024, // 64 KB
            low_water_mark: 16 * 1024,  // 16 KB
        }
    }
}

// ---------------------------------------------------------------------------
// BackpressureController
// ---------------------------------------------------------------------------

/// Tracks write buffer state and signals backpressure.
///
/// This controller is designed to sit alongside a framed writer. The caller
/// reports bytes added to (and flushed from) the write buffer, and the
/// controller determines whether the connection is ready for more writes.
#[derive(Debug)]
pub struct BackpressureController {
    config: WriteConfig,
    buffered_bytes: usize,
    backpressured: bool,
}

impl BackpressureController {
    /// Creates a new controller with the given configuration.
    pub fn new(config: WriteConfig) -> Self {
        Self {
            config,
            buffered_bytes: 0,
            backpressured: false,
        }
    }

    /// Records that `n` bytes have been added to the write buffer.
    ///
    /// Returns `true` if the connection just became backpressured (i.e., this
    /// call crossed the high-water mark).
    pub fn record_buffered(&mut self, n: usize) -> bool {
        self.buffered_bytes = self.buffered_bytes.saturating_add(n);
        if !self.backpressured && self.buffered_bytes >= self.config.high_water_mark {
            self.backpressured = true;
            info!(
                buffered = self.buffered_bytes,
                high_water = self.config.high_water_mark,
                "write backpressure engaged"
            );
            return true;
        }
        false
    }

    /// Records that `n` bytes have been flushed from the write buffer.
    ///
    /// Returns `true` if backpressure was just released (i.e., the buffer
    /// dropped below the low-water mark).
    pub fn record_flushed(&mut self, n: usize) -> bool {
        self.buffered_bytes = self.buffered_bytes.saturating_sub(n);
        if self.backpressured && self.buffered_bytes <= self.config.low_water_mark {
            self.backpressured = false;
            info!(
                buffered = self.buffered_bytes,
                low_water = self.config.low_water_mark,
                "write backpressure released"
            );
            return true;
        }
        false
    }

    /// Returns `true` if the connection is ready for more writes (not
    /// backpressured).
    pub fn is_write_ready(&self) -> bool {
        !self.backpressured
    }

    /// Returns the current number of buffered bytes.
    pub fn buffered_bytes(&self) -> usize {
        self.buffered_bytes
    }

    /// Returns the configuration.
    pub fn config(&self) -> &WriteConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// ReadLimiter
// ---------------------------------------------------------------------------

/// Bounds the number of unprocessed inbound messages.
///
/// When the count reaches `max_messages`, the connection should stop reading
/// from the socket to let TCP flow control propagate backpressure to the
/// sender.
#[derive(Debug)]
pub struct ReadLimiter {
    max_messages: usize,
    current: usize,
    paused: bool,
}

impl ReadLimiter {
    /// Creates a new read limiter.
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            current: 0,
            paused: false,
        }
    }

    /// Records that a message has been received but not yet processed.
    ///
    /// Returns `true` if reading should be paused (limit reached).
    pub fn record_received(&mut self) -> bool {
        self.current = self.current.saturating_add(1);
        if !self.paused && self.current >= self.max_messages {
            self.paused = true;
            info!(
                queued = self.current,
                max = self.max_messages,
                "read backpressure engaged — pausing reads"
            );
            return true;
        }
        false
    }

    /// Records that a previously received message has been processed/consumed.
    ///
    /// Returns `true` if reading was paused and should now resume.
    pub fn record_consumed(&mut self) -> bool {
        self.current = self.current.saturating_sub(1);
        if self.paused && self.current < self.max_messages {
            self.paused = false;
            info!(
                queued = self.current,
                max = self.max_messages,
                "read backpressure released — resuming reads"
            );
            return true;
        }
        false
    }

    /// Returns `true` if reading is currently paused.
    pub fn is_read_paused(&self) -> bool {
        self.paused
    }

    /// Returns the number of unprocessed messages.
    pub fn pending(&self) -> usize {
        self.current
    }

    /// Returns the maximum number of unprocessed messages.
    pub fn max_messages(&self) -> usize {
        self.max_messages
    }
}

/// Default maximum number of unprocessed inbound messages.
pub const DEFAULT_READ_LIMIT: usize = 256;

// ---------------------------------------------------------------------------
// BoundedChannel
// ---------------------------------------------------------------------------

/// A bounded async channel for message passing between tasks.
///
/// Wraps [`tokio::sync::mpsc`] with a fixed capacity. When the channel is
/// full, [`BoundedSender::send`] will wait and [`BoundedSender::try_send`]
/// will return an error immediately.
pub struct BoundedChannel;

impl BoundedChannel {
    /// Creates a bounded channel with the given capacity.
    ///
    /// Returns a `(BoundedSender, BoundedReceiver)` pair.
    pub fn channel<T>(capacity: usize) -> (BoundedSender<T>, BoundedReceiver<T>) {
        let (tx, rx) = mpsc::channel(capacity);
        let full_count = Arc::new(AtomicUsize::new(0));
        let was_full = Arc::new(AtomicBool::new(false));
        (
            BoundedSender {
                inner: tx,
                capacity,
                full_count: full_count.clone(),
                was_full: was_full.clone(),
            },
            BoundedReceiver {
                inner: rx,
                capacity,
                was_full,
            },
        )
    }
}

/// Sending half of a [`BoundedChannel`].
#[derive(Debug)]
pub struct BoundedSender<T> {
    inner: mpsc::Sender<T>,
    capacity: usize,
    full_count: Arc<AtomicUsize>,
    was_full: Arc<AtomicBool>,
}

impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            capacity: self.capacity,
            full_count: self.full_count.clone(),
            was_full: self.was_full.clone(),
        }
    }
}

impl<T: Send> BoundedSender<T> {
    /// Sends a value, waiting if the channel is full.
    ///
    /// Returns an error if the receiver has been dropped.
    pub async fn send(&self, value: T) -> Result<(), NetworkError> {
        self.inner
            .send(value)
            .await
            .map_err(|_| NetworkError::ChannelClosed)?;
        Ok(())
    }

    /// Attempts to send a value without waiting.
    ///
    /// Returns an error if the channel is full or the receiver has been dropped.
    pub fn try_send(&self, value: T) -> Result<(), NetworkError> {
        match self.inner.try_send(value) {
            Ok(()) => {
                // Check if we were previously full and now accepted
                if self.was_full.load(Ordering::Relaxed) {
                    trace!(capacity = self.capacity, "bounded channel no longer full");
                    self.was_full.store(false, Ordering::Relaxed);
                }
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                let count = self.full_count.fetch_add(1, Ordering::Relaxed) + 1;
                if !self.was_full.swap(true, Ordering::Relaxed) {
                    info!(
                        capacity = self.capacity,
                        full_count = count,
                        "bounded channel full — backpressure active"
                    );
                }
                Err(NetworkError::ChannelFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(NetworkError::ChannelClosed),
        }
    }

    /// Returns the channel capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Receiving half of a [`BoundedChannel`].
#[derive(Debug)]
pub struct BoundedReceiver<T> {
    inner: mpsc::Receiver<T>,
    capacity: usize,
    was_full: Arc<AtomicBool>,
}

impl<T> BoundedReceiver<T> {
    /// Receives the next value, waiting if the channel is empty.
    ///
    /// Returns `None` if all senders have been dropped.
    pub async fn recv(&mut self) -> Option<T> {
        let value = self.inner.recv().await;
        if value.is_some() && self.was_full.load(Ordering::Relaxed) {
            trace!(
                capacity = self.capacity,
                "bounded channel drained — backpressure released"
            );
            self.was_full.store(false, Ordering::Relaxed);
        }
        value
    }

    /// Attempts to receive without waiting.
    ///
    /// Returns `None` if the channel is empty or all senders have been dropped.
    pub fn try_recv(&mut self) -> Option<T> {
        self.inner.try_recv().ok()
    }

    /// Returns the channel capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WriteConfig tests --

    #[test]
    fn write_config_defaults() {
        let cfg = WriteConfig::default();
        assert_eq!(cfg.high_water_mark, 64 * 1024);
        assert_eq!(cfg.low_water_mark, 16 * 1024);
    }

    #[test]
    fn write_config_custom() {
        let cfg = WriteConfig {
            high_water_mark: 1024,
            low_water_mark: 256,
        };
        assert_eq!(cfg.high_water_mark, 1024);
        assert_eq!(cfg.low_water_mark, 256);
    }

    // -- BackpressureController tests --

    #[test]
    fn controller_starts_ready() {
        let ctrl = BackpressureController::new(WriteConfig::default());
        assert!(ctrl.is_write_ready());
        assert_eq!(ctrl.buffered_bytes(), 0);
    }

    #[test]
    fn controller_engages_at_high_water() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        // Below threshold — still ready
        assert!(!ctrl.record_buffered(99));
        assert!(ctrl.is_write_ready());

        // At threshold — backpressure engages
        assert!(ctrl.record_buffered(1));
        assert!(!ctrl.is_write_ready());
    }

    #[test]
    fn controller_releases_at_low_water() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        // Engage backpressure
        ctrl.record_buffered(120);
        assert!(!ctrl.is_write_ready());

        // Flush some — still above low water
        assert!(!ctrl.record_flushed(60));
        assert!(!ctrl.is_write_ready());

        // Flush below low water — released
        assert!(ctrl.record_flushed(20));
        assert!(ctrl.is_write_ready());
    }

    #[test]
    fn controller_hysteresis() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        // Engage
        ctrl.record_buffered(100);
        assert!(!ctrl.is_write_ready());

        // Drop to 60 — still backpressured (above low water)
        ctrl.record_flushed(40);
        assert!(!ctrl.is_write_ready());

        // Drop to 50 — released (at low water)
        ctrl.record_flushed(10);
        assert!(ctrl.is_write_ready());

        // Go back up to 80 — still ready (below high water)
        ctrl.record_buffered(30);
        assert!(ctrl.is_write_ready());
    }

    #[test]
    fn controller_does_not_double_engage() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        assert!(ctrl.record_buffered(100)); // first engage returns true
        assert!(!ctrl.record_buffered(50)); // second returns false (already engaged)
    }

    #[test]
    fn controller_does_not_double_release() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        ctrl.record_buffered(100);
        assert!(ctrl.record_flushed(80)); // first release returns true
        assert!(!ctrl.record_flushed(5)); // already released
    }

    #[test]
    fn controller_saturating_arithmetic() {
        let cfg = WriteConfig {
            high_water_mark: 100,
            low_water_mark: 50,
        };
        let mut ctrl = BackpressureController::new(cfg);

        // Flushing more than buffered should not underflow
        ctrl.record_buffered(10);
        ctrl.record_flushed(100);
        assert_eq!(ctrl.buffered_bytes(), 0);
    }

    // -- ReadLimiter tests --

    #[test]
    fn read_limiter_starts_unpaused() {
        let lim = ReadLimiter::new(10);
        assert!(!lim.is_read_paused());
        assert_eq!(lim.pending(), 0);
    }

    #[test]
    fn read_limiter_pauses_at_limit() {
        let mut lim = ReadLimiter::new(3);

        assert!(!lim.record_received());
        assert!(!lim.record_received());
        assert!(lim.record_received()); // third message hits limit
        assert!(lim.is_read_paused());
    }

    #[test]
    fn read_limiter_resumes_after_consume() {
        let mut lim = ReadLimiter::new(3);

        // Fill to limit
        lim.record_received();
        lim.record_received();
        lim.record_received();
        assert!(lim.is_read_paused());

        // Consume one — drops below limit, should resume
        assert!(lim.record_consumed());
        assert!(!lim.is_read_paused());
    }

    #[test]
    fn read_limiter_does_not_double_pause() {
        let mut lim = ReadLimiter::new(2);
        lim.record_received();
        assert!(lim.record_received()); // pauses
        assert!(!lim.record_received()); // already paused
    }

    #[test]
    fn read_limiter_saturating_consume() {
        let mut lim = ReadLimiter::new(10);
        // Consuming without receiving should not underflow
        lim.record_consumed();
        assert_eq!(lim.pending(), 0);
    }

    // -- BoundedChannel tests --

    #[tokio::test]
    async fn bounded_channel_send_recv() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(8);
        tx.send(42).await.unwrap();
        assert_eq!(rx.recv().await, Some(42));
    }

    #[tokio::test]
    async fn bounded_channel_try_send_succeeds_when_not_full() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(2);
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();

        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, Some(2));
    }

    #[tokio::test]
    async fn bounded_channel_try_send_fails_when_full() {
        let (tx, _rx) = BoundedChannel::channel::<i32>(2);
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();

        let result = tx.try_send(3);
        assert!(matches!(result, Err(NetworkError::ChannelFull)));
    }

    #[tokio::test]
    async fn bounded_channel_send_errors_when_receiver_dropped() {
        let (tx, rx) = BoundedChannel::channel::<i32>(8);
        drop(rx);

        let result = tx.send(42).await;
        assert!(matches!(result, Err(NetworkError::ChannelClosed)));
    }

    #[tokio::test]
    async fn bounded_channel_try_send_errors_when_receiver_dropped() {
        let (tx, rx) = BoundedChannel::channel::<i32>(8);
        drop(rx);

        let result = tx.try_send(42);
        assert!(matches!(result, Err(NetworkError::ChannelClosed)));
    }

    #[tokio::test]
    async fn bounded_channel_recv_returns_none_when_sender_dropped() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(8);
        drop(tx);

        assert_eq!(rx.recv().await, None);
    }

    #[tokio::test]
    async fn bounded_channel_capacity() {
        let (tx, rx) = BoundedChannel::channel::<i32>(16);
        assert_eq!(tx.capacity(), 16);
        assert_eq!(rx.capacity(), 16);
    }

    #[tokio::test]
    async fn bounded_channel_fifo_order() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(8);

        for i in 0..5 {
            tx.send(i).await.unwrap();
        }

        for i in 0..5 {
            assert_eq!(rx.recv().await, Some(i));
        }
    }

    #[tokio::test]
    async fn bounded_channel_try_recv() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(8);

        // Empty channel
        assert!(rx.try_recv().is_none());

        tx.send(42).await.unwrap();
        assert_eq!(rx.try_recv(), Some(42));
        assert!(rx.try_recv().is_none());
    }

    #[tokio::test]
    async fn bounded_channel_sender_clone() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(8);
        let tx2 = tx.clone();

        tx.send(1).await.unwrap();
        tx2.send(2).await.unwrap();

        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, Some(2));
    }

    #[tokio::test]
    async fn bounded_channel_send_waits_when_full() {
        let (tx, mut rx) = BoundedChannel::channel::<i32>(1);

        tx.send(1).await.unwrap();

        // Spawn a sender that will block because channel is full
        let tx_clone = tx.clone();
        let handle = tokio::spawn(async move {
            tx_clone.send(2).await.unwrap();
        });

        // Give the spawned task a moment to block
        tokio::task::yield_now().await;

        // Drain one to unblock the sender
        assert_eq!(rx.recv().await, Some(1));

        // The spawned send should now complete
        handle.await.unwrap();
        assert_eq!(rx.recv().await, Some(2));
    }
}
