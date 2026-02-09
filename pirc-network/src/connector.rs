//! Outbound TCP connector with reconnection support.
//!
//! [`Connector`] establishes TCP connections and returns [`Connection`] objects.
//! [`ReconnectingConnector`] wraps a `Connector` with a [`ReconnectPolicy`]
//! that retries with exponential backoff on failure.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time;
use tracing::{info, instrument, warn};

use crate::connection::Connection;
use crate::error::NetworkError;

// ---------------------------------------------------------------------------
// Connector
// ---------------------------------------------------------------------------

/// Default connection timeout (10 seconds).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// A TCP connector that establishes outbound connections.
///
/// Each successful connection is wrapped as a [`Connection`] with typed IRC
/// message I/O. A configurable timeout is applied to each connect attempt.
#[derive(Debug, Clone)]
pub struct Connector {
    timeout: Duration,
}

impl Connector {
    /// Create a new `Connector` with the default timeout (10s).
    pub fn new() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a new `Connector` with the given connection timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Connect to the given address, returning a [`Connection`] on success.
    ///
    /// The connection attempt is subject to the configured timeout.
    #[instrument(skip(self), fields(%addr, timeout_ms = self.timeout.as_millis()))]
    pub async fn connect(&self, addr: SocketAddr) -> Result<Connection, NetworkError> {
        let stream = time::timeout(self.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(NetworkError::Io)?;

        let conn = Connection::new(stream)?;
        info!(id = conn.info().id, %addr, "connected");
        Ok(conn)
    }
}

impl Default for Connector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ReconnectPolicy
// ---------------------------------------------------------------------------

/// Configuration for exponential backoff reconnection.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Maximum number of retry attempts. `None` means unlimited.
    pub max_retries: Option<u32>,
    /// Initial delay before the first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries (caps exponential growth).
    pub max_delay: Duration,
    /// Multiplicative factor applied to the delay after each retry.
    pub backoff_factor: f64,
}

impl ReconnectPolicy {
    /// Compute the delay for the given attempt number (0-indexed).
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_delay.as_secs_f64();
        let delay_secs = base * self.backoff_factor.powi(i32::try_from(attempt).unwrap_or(i32::MAX));
        let delay = Duration::from_secs_f64(delay_secs);
        delay.min(self.max_delay)
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_retries: Some(5),
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

// ---------------------------------------------------------------------------
// ReconnectingConnector
// ---------------------------------------------------------------------------

/// A connector that retries connection attempts with exponential backoff.
///
/// Wraps a [`Connector`] and applies a [`ReconnectPolicy`] to retry failed
/// connection attempts. Each retry emits a tracing event.
#[derive(Debug, Clone)]
pub struct ReconnectingConnector {
    connector: Connector,
    policy: ReconnectPolicy,
}

impl ReconnectingConnector {
    /// Create a new `ReconnectingConnector` with the given connector and policy.
    pub fn new(connector: Connector, policy: ReconnectPolicy) -> Self {
        Self { connector, policy }
    }

    /// Attempt to connect with retries according to the configured policy.
    ///
    /// On each failure the connector waits for an exponentially increasing
    /// delay (up to `max_delay`) before retrying. The last error is returned
    /// when all retries are exhausted.
    #[instrument(skip(self), fields(%addr))]
    pub async fn connect_with_retry(
        &self,
        addr: SocketAddr,
    ) -> Result<Connection, NetworkError> {
        let mut attempt: u32 = 0;

        loop {
            match self.connector.connect(addr).await {
                Ok(conn) => {
                    if attempt > 0 {
                        info!(attempt, "reconnected successfully");
                    }
                    return Ok(conn);
                }
                Err(err) => {
                    let retries_exhausted = self
                        .policy
                        .max_retries
                        .is_some_and(|max| attempt >= max);

                    if retries_exhausted {
                        warn!(attempt, %err, "all retries exhausted");
                        return Err(err);
                    }

                    let delay = self.policy.delay_for_attempt(attempt);
                    warn!(
                        attempt,
                        next_attempt = attempt + 1,
                        delay_ms = delay.as_millis(),
                        %err,
                        "connection failed, retrying"
                    );

                    time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listener::Listener;

    /// Helper: bind a Listener on a random loopback port.
    async fn loopback_listener() -> Listener {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        Listener::bind(addr).await.unwrap()
    }

    // -- Connector tests --

    #[tokio::test]
    async fn connect_to_listener_succeeds() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        let connector = Connector::new();
        let conn = connector.connect(addr).await.unwrap();
        assert!(conn.info().peer_addr.ip().is_loopback());
        assert_eq!(conn.info().peer_addr.port(), addr.port());
    }

    #[tokio::test]
    async fn connect_to_closed_port_fails() {
        // Bind and immediately drop so the port is closed.
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let connector = Connector::new();
        let result = connector.connect(addr).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn connect_timeout_enforced() {
        // Use a non-routable address to trigger timeout. 192.0.2.1 is
        // TEST-NET-1 (RFC 5737), traffic should black-hole.
        let addr: SocketAddr = "192.0.2.1:6667".parse().unwrap();
        let connector = Connector::with_timeout(Duration::from_millis(100));

        let start = tokio::time::Instant::now();
        let result = connector.connect(addr).await;
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(NetworkError::Timeout)));
        // Should complete near the timeout, not hang for the default 10s.
        assert!(elapsed < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn connector_default_timeout_is_10s() {
        let connector = Connector::new();
        assert_eq!(connector.timeout, DEFAULT_TIMEOUT);
    }

    #[tokio::test]
    async fn connector_default_trait() {
        let connector = Connector::default();
        assert_eq!(connector.timeout, DEFAULT_TIMEOUT);
    }

    // -- ReconnectPolicy tests --

    #[test]
    fn policy_default_values() {
        let policy = ReconnectPolicy::default();
        assert_eq!(policy.max_retries, Some(5));
        assert_eq!(policy.initial_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
        assert_eq!(policy.backoff_factor, 2.0);
    }

    #[test]
    fn policy_delay_for_attempt_exponential() {
        let policy = ReconnectPolicy {
            max_retries: Some(10),
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        };

        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(4));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(8));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(16));
        // Attempt 5 would be 32s, but capped at 30s
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(30));
    }

    #[test]
    fn policy_delay_capped_at_max() {
        let policy = ReconnectPolicy {
            max_retries: None,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            backoff_factor: 10.0,
        };

        // 1 * 10^2 = 100, capped to 5
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(5));
    }

    // -- ReconnectingConnector tests --

    #[tokio::test]
    async fn reconnecting_connect_succeeds_first_try() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        let rc = ReconnectingConnector::new(
            Connector::new(),
            ReconnectPolicy::default(),
        );

        let conn = rc.connect_with_retry(addr).await.unwrap();
        assert_eq!(conn.info().peer_addr.port(), addr.port());
    }

    #[tokio::test]
    async fn reconnecting_connect_retries_then_succeeds() {
        // Start a listener, get its address, drop it, then restart it
        // after a short delay. The reconnecting connector should retry
        // and eventually succeed.
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        drop(listener);

        // Restart the listener on the same address after a delay.
        let restart_handle = tokio::spawn(async move {
            time::sleep(Duration::from_millis(150)).await;
            Listener::bind(addr).await.unwrap()
        });

        let policy = ReconnectPolicy {
            max_retries: Some(10),
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(200),
            backoff_factor: 1.5,
        };

        let rc = ReconnectingConnector::new(
            Connector::with_timeout(Duration::from_millis(200)),
            policy,
        );

        let conn = rc.connect_with_retry(addr).await.unwrap();
        assert_eq!(conn.info().peer_addr.port(), addr.port());

        // Keep the restarted listener alive until after the test assertion
        let _listener = restart_handle.await.unwrap();
    }

    #[tokio::test]
    async fn reconnecting_connect_exhausts_retries() {
        // Connect to a closed port with limited retries.
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let policy = ReconnectPolicy {
            max_retries: Some(2),
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(50),
            backoff_factor: 2.0,
        };

        let rc = ReconnectingConnector::new(
            Connector::with_timeout(Duration::from_millis(100)),
            policy,
        );

        let result = rc.connect_with_retry(addr).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reconnecting_connect_unlimited_retries_succeeds() {
        // Use unlimited retries (max_retries = None) and verify it
        // eventually connects when the listener restarts.
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let restart_handle = tokio::spawn(async move {
            time::sleep(Duration::from_millis(100)).await;
            Listener::bind(addr).await.unwrap()
        });

        let policy = ReconnectPolicy {
            max_retries: None,
            initial_delay: Duration::from_millis(30),
            max_delay: Duration::from_millis(100),
            backoff_factor: 1.5,
        };

        let rc = ReconnectingConnector::new(
            Connector::with_timeout(Duration::from_millis(100)),
            policy,
        );

        let conn = rc.connect_with_retry(addr).await.unwrap();
        assert_eq!(conn.info().peer_addr.port(), addr.port());

        let _listener = restart_handle.await.unwrap();
    }

    #[tokio::test]
    async fn reconnecting_connector_debug_impl() {
        let rc = ReconnectingConnector::new(
            Connector::new(),
            ReconnectPolicy::default(),
        );
        let debug_str = format!("{rc:?}");
        assert!(debug_str.contains("ReconnectingConnector"));
        assert!(debug_str.contains("Connector"));
        assert!(debug_str.contains("ReconnectPolicy"));
    }
}
