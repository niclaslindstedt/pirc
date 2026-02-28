//! Connection pooling for server-to-server links.
//!
//! Provides [`ConnectionPool`], which manages a set of named connections keyed
//! by [`ServerId`]. The pool enforces a maximum capacity and supports
//! broadcasting a message to all connected servers.

use std::collections::HashMap;
use std::ops::Deref;

use pirc_common::ServerId;
use pirc_protocol::Message;
use tokio::sync::{RwLock, RwLockReadGuard};
use tracing::{debug, warn};

use crate::connection::{AsyncTransport, Connection};
use crate::error::NetworkError;
use crate::shutdown::ShutdownSignal;

/// An RAII read guard wrapping a borrowed [`Connection`] from the pool.
///
/// This type is returned by [`ConnectionPool::get`] and dereferences to
/// `Connection`, keeping the pool's read lock held for the lifetime of the
/// guard.
pub struct ConnectionRef<'a>(RwLockReadGuard<'a, Connection>);

impl Deref for ConnectionRef<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.0
    }
}

/// A pool of server-to-server connections keyed by [`ServerId`].
///
/// The pool enforces a configurable maximum number of connections and provides
/// broadcast and shutdown operations across all pooled connections.
#[derive(Debug)]
pub struct ConnectionPool {
    connections: RwLock<HashMap<ServerId, Connection>>,
    max_connections: usize,
}

impl ConnectionPool {
    /// Creates a new empty connection pool with the given capacity limit.
    pub fn new(max_connections: usize) -> Self {
        debug!(max_connections, "connection pool created");
        Self {
            connections: RwLock::new(HashMap::new()),
            max_connections,
        }
    }

    /// Adds a connection to the pool for the given server.
    ///
    /// Returns [`NetworkError::PoolExhausted`] if the pool is at capacity.
    /// Replaces any existing connection for the same `ServerId`.
    pub async fn add(&self, id: ServerId, conn: Connection) -> Result<(), NetworkError> {
        let mut conns = self.connections.write().await;
        // Allow replacement of existing entry without counting against capacity
        if !conns.contains_key(&id) && conns.len() >= self.max_connections {
            warn!(%id, max = self.max_connections, "connection pool exhausted");
            return Err(NetworkError::PoolExhausted);
        }
        debug!(%id, conn_id = conn.info().id, "added connection to pool");
        conns.insert(id, conn);
        Ok(())
    }

    /// Borrows the connection for the given server.
    ///
    /// Returns a [`ConnectionRef`] RAII guard that holds the pool's read lock
    /// for its lifetime, or `None` if no connection exists for `id`.
    pub async fn get(&self, id: ServerId) -> Option<ConnectionRef<'_>> {
        let guard = self.connections.read().await;
        // We must check containment first; RwLockReadGuard::try_map fails
        // (returning the original guard) when the key is absent.
        if !guard.contains_key(&id) {
            return None;
        }
        let mapped = RwLockReadGuard::try_map(guard, |conns| conns.get(&id)).ok()?;
        Some(ConnectionRef(mapped))
    }

    /// Removes and returns the connection for the given server.
    pub async fn remove(&self, id: ServerId) -> Option<Connection> {
        let mut conns = self.connections.write().await;
        let conn = conns.remove(&id);
        if conn.is_some() {
            debug!(%id, "removed connection from pool");
        }
        conn
    }

    /// Returns `true` if the pool contains a connection for the given server.
    pub async fn contains(&self, id: ServerId) -> bool {
        self.connections.read().await.contains_key(&id)
    }

    /// Returns the list of all connected server IDs.
    pub async fn connected_servers(&self) -> Vec<ServerId> {
        self.connections.read().await.keys().copied().collect()
    }

    /// Returns the number of connections in the pool.
    pub async fn len(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Returns `true` if the pool contains no connections.
    pub async fn is_empty(&self) -> bool {
        self.connections.read().await.is_empty()
    }

    /// Sends a message to all pooled connections.
    ///
    /// Returns a list of `(ServerId, Result<()>)` indicating the outcome for
    /// each connection. Connections that fail to send are not removed from the
    /// pool — the caller can decide how to handle failures.
    pub async fn broadcast(&self, msg: &Message) -> Vec<(ServerId, Result<(), NetworkError>)> {
        let mut conns = self.connections.write().await;
        let mut results = Vec::with_capacity(conns.len());
        let batch = [msg.clone()];
        for (&id, conn) in conns.iter_mut() {
            let result = conn.send_batch(&batch).await;
            if let Err(ref e) = result {
                warn!(%id, error = %e, "broadcast send failed");
            }
            results.push((id, result));
        }
        results
    }

    /// Gracefully shuts down all pooled connections and removes them.
    pub async fn shutdown_all(&self) -> Result<(), NetworkError> {
        let mut conns = self.connections.write().await;
        debug!(count = conns.len(), "shutting down all pooled connections");
        let mut last_err = None;
        for (id, conn) in conns.iter_mut() {
            if let Err(e) = conn.shutdown().await {
                warn!(%id, error = %e, "shutdown failed for pooled connection");
                last_err = Some(e);
            }
        }
        conns.clear();
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Wait for a shutdown signal, then gracefully shut down all pooled
    /// connections.
    ///
    /// This is a convenience method that combines waiting for the shutdown
    /// signal with the actual shutdown procedure. It flushes all connections
    /// before closing them.
    pub async fn shutdown_on_signal(
        &self,
        mut shutdown: ShutdownSignal,
    ) -> Result<(), NetworkError> {
        shutdown.recv().await;
        debug!("shutdown signal received, shutting down pool");
        self.shutdown_all().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownSignal;
    use pirc_protocol::Command;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    /// Helper: create a loopback TCP pair and wrap both ends as Connections.
    async fn loopback_pair() -> (Connection, Connection) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (client_stream, server_stream) = tokio::try_join!(TcpStream::connect(addr), async {
            listener.accept().await.map(|(s, _)| s)
        })
        .unwrap();

        let client = Connection::new(client_stream).unwrap();
        let server = Connection::new(server_stream).unwrap();
        (client, server)
    }

    #[tokio::test]
    async fn add_and_contains() {
        let pool = ConnectionPool::new(10);
        let (conn, _peer) = loopback_pair().await;

        let id = ServerId::new(1);
        pool.add(id, conn).await.unwrap();

        assert!(pool.contains(id).await);
        assert!(!pool.contains(ServerId::new(2)).await);
    }

    #[tokio::test]
    async fn add_and_remove() {
        let pool = ConnectionPool::new(10);
        let (conn, _peer) = loopback_pair().await;

        let id = ServerId::new(1);
        pool.add(id, conn).await.unwrap();
        assert_eq!(pool.len().await, 1);

        let removed = pool.remove(id).await;
        assert!(removed.is_some());
        assert_eq!(pool.len().await, 0);
        assert!(!pool.contains(id).await);
    }

    #[tokio::test]
    async fn remove_nonexistent_returns_none() {
        let pool = ConnectionPool::new(10);
        let removed = pool.remove(ServerId::new(99)).await;
        assert!(removed.is_none());
    }

    #[tokio::test]
    async fn len_and_is_empty() {
        let pool = ConnectionPool::new(10);
        assert!(pool.is_empty().await);
        assert_eq!(pool.len().await, 0);

        let (conn, _peer) = loopback_pair().await;
        pool.add(ServerId::new(1), conn).await.unwrap();
        assert!(!pool.is_empty().await);
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn connected_servers_returns_all_ids() {
        let pool = ConnectionPool::new(10);

        let (c1, _p1) = loopback_pair().await;
        let (c2, _p2) = loopback_pair().await;
        let (c3, _p3) = loopback_pair().await;

        pool.add(ServerId::new(10), c1).await.unwrap();
        pool.add(ServerId::new(20), c2).await.unwrap();
        pool.add(ServerId::new(30), c3).await.unwrap();

        let mut servers = pool.connected_servers().await;
        servers.sort();
        assert_eq!(
            servers,
            vec![ServerId::new(10), ServerId::new(20), ServerId::new(30)]
        );
    }

    #[tokio::test]
    async fn capacity_enforcement() {
        let pool = ConnectionPool::new(2);

        let (c1, _p1) = loopback_pair().await;
        let (c2, _p2) = loopback_pair().await;
        let (c3, _p3) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        let result = pool.add(ServerId::new(3), c3).await;
        assert!(matches!(result, Err(NetworkError::PoolExhausted)));
        assert_eq!(pool.len().await, 2);
    }

    #[tokio::test]
    async fn replace_existing_does_not_count_against_capacity() {
        let pool = ConnectionPool::new(1);

        let (c1, _p1) = loopback_pair().await;
        pool.add(ServerId::new(1), c1).await.unwrap();

        // Replacing the same server ID should succeed even at capacity
        let (c2, _p2) = loopback_pair().await;
        pool.add(ServerId::new(1), c2).await.unwrap();
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn broadcast_sends_to_all() {
        let pool = ConnectionPool::new(10);

        // Create two connections, keeping the peer side to receive
        let (c1, _p1) = loopback_pair().await;
        let (c2, _p2) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        let msg = Message::new(Command::Ping, vec!["broadcast".to_owned()]);
        let results = pool.broadcast(&msg).await;

        assert_eq!(results.len(), 2);
        for (_, result) in &results {
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn broadcast_to_empty_pool() {
        let pool = ConnectionPool::new(10);
        let msg = Message::new(Command::Ping, vec!["empty".to_owned()]);
        let results = pool.broadcast(&msg).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn broadcast_reports_per_connection_results() {
        let pool = ConnectionPool::new(10);

        // Create a connection whose peer is dropped, so sends will fail
        let (c1, peer1) = loopback_pair().await;
        drop(peer1);

        // Create a healthy connection
        let (c2, _p2) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        let msg = Message::new(Command::Ping, vec!["test".to_owned()]);
        let results = pool.broadcast(&msg).await;

        assert_eq!(results.len(), 2);

        // One should succeed, one might fail (depending on OS buffering)
        // We at least verify results are returned for each connection
        let ids: Vec<ServerId> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&ServerId::new(1)));
        assert!(ids.contains(&ServerId::new(2)));
    }

    #[tokio::test]
    async fn broadcast_messages_received_by_peers() {
        let pool = ConnectionPool::new(10);

        let (c1, mut p1) = loopback_pair().await;
        let (c2, mut p2) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        let msg = Message::new(Command::Ping, vec!["hello".to_owned()]);
        pool.broadcast(&msg).await;

        // Both peers should receive the message
        let r1 = p1.recv().await.unwrap().unwrap();
        let r2 = p2.recv().await.unwrap().unwrap();
        assert_eq!(r1, msg);
        assert_eq!(r2, msg);
    }

    #[tokio::test]
    async fn get_existing_connection() {
        let pool = ConnectionPool::new(10);
        let (conn, _peer) = loopback_pair().await;
        let id = ServerId::new(1);
        let conn_id = conn.info().id;

        pool.add(id, conn).await.unwrap();

        let got = pool.get(id).await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().info().id, conn_id);
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let pool = ConnectionPool::new(10);
        assert!(pool.get(ServerId::new(42)).await.is_none());
    }

    #[tokio::test]
    async fn get_after_remove_returns_none() {
        let pool = ConnectionPool::new(10);
        let (conn, _peer) = loopback_pair().await;
        let id = ServerId::new(1);

        pool.add(id, conn).await.unwrap();
        assert!(pool.get(id).await.is_some());

        pool.remove(id).await;
        assert!(pool.get(id).await.is_none());
    }

    #[tokio::test]
    async fn concurrent_gets_succeed() {
        let pool = std::sync::Arc::new(ConnectionPool::new(10));
        let (conn, _peer) = loopback_pair().await;
        let id = ServerId::new(1);
        let conn_id = conn.info().id;

        pool.add(id, conn).await.unwrap();

        // Spawn multiple concurrent readers
        let mut handles = Vec::new();
        for _ in 0..10 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let got = pool.get(id).await;
                assert!(got.is_some());
                assert_eq!(got.unwrap().info().id, conn_id);
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn get_returns_correct_connection_among_multiple() {
        let pool = ConnectionPool::new(10);

        let (c1, _p1) = loopback_pair().await;
        let (c2, _p2) = loopback_pair().await;
        let c1_id = c1.info().id;
        let c2_id = c2.info().id;

        let id1 = ServerId::new(1);
        let id2 = ServerId::new(2);

        pool.add(id1, c1).await.unwrap();
        pool.add(id2, c2).await.unwrap();

        let got1 = pool.get(id1).await.unwrap();
        assert_eq!(got1.info().id, c1_id);
        drop(got1);

        let got2 = pool.get(id2).await.unwrap();
        assert_eq!(got2.info().id, c2_id);
    }

    #[tokio::test]
    async fn shutdown_all_closes_connections() {
        let pool = ConnectionPool::new(10);

        let (c1, mut p1) = loopback_pair().await;
        let (c2, mut p2) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        pool.shutdown_all().await.unwrap();

        // Pool should be empty after shutdown
        assert!(pool.is_empty().await);

        // Peers should see EOF
        assert!(p1.recv().await.unwrap().is_none());
        assert!(p2.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn shutdown_all_on_empty_pool() {
        let pool = ConnectionPool::new(10);
        pool.shutdown_all().await.unwrap();
        assert!(pool.is_empty().await);
    }

    #[tokio::test]
    async fn shutdown_on_signal_closes_all() {
        let pool = std::sync::Arc::new(ConnectionPool::new(10));

        let (c1, mut p1) = loopback_pair().await;
        let (c2, mut p2) = loopback_pair().await;

        pool.add(ServerId::new(1), c1).await.unwrap();
        pool.add(ServerId::new(2), c2).await.unwrap();

        let (controller, signal) = ShutdownSignal::new();

        let pool_clone = pool.clone();
        let handle = tokio::spawn(async move { pool_clone.shutdown_on_signal(signal).await });

        // Give the task time to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Signal shutdown
        controller.shutdown();

        handle.await.unwrap().unwrap();

        // Pool should be empty
        assert!(pool.is_empty().await);

        // Peers should see EOF
        assert!(p1.recv().await.unwrap().is_none());
        assert!(p2.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn shutdown_on_signal_empty_pool() {
        let pool = ConnectionPool::new(10);
        let (controller, signal) = ShutdownSignal::new();

        controller.shutdown();
        pool.shutdown_on_signal(signal).await.unwrap();
        assert!(pool.is_empty().await);
    }
}
