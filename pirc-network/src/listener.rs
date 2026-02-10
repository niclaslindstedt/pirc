//! TCP listener and connection acceptor.
//!
//! [`Listener`] wraps a [`tokio::net::TcpListener`] and produces
//! [`Connection`] objects for each accepted TCP stream.

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing::{info, instrument, trace};

use crate::connection::Connection;
use crate::error::NetworkError;
use crate::shutdown::ShutdownSignal;

/// A TCP listener that accepts incoming connections and wraps them as
/// [`Connection`] objects with typed IRC message I/O.
///
/// Each accepted connection is automatically assigned a monotonically
/// increasing connection ID (managed by [`Connection::new`]).
pub struct Listener {
    inner: TcpListener,
}

impl Listener {
    /// Bind to the given socket address and start listening.
    #[instrument(skip_all, fields(%addr))]
    pub async fn bind(addr: SocketAddr) -> Result<Self, NetworkError> {
        let inner = TcpListener::bind(addr).await?;
        let local = inner.local_addr()?;
        info!(%local, "listener bound");
        Ok(Self { inner })
    }

    /// Accept the next incoming connection.
    ///
    /// Returns a [`Connection`] wrapping the accepted TCP stream and the
    /// remote peer's [`SocketAddr`].
    #[instrument(skip(self), fields(local = %self.inner.local_addr().unwrap()))]
    pub async fn accept(&self) -> Result<(Connection, SocketAddr), NetworkError> {
        trace!("waiting for connection");
        let (stream, peer_addr) = self.inner.accept().await?;
        let conn = Connection::new(stream)?;
        let id = conn.info().id;
        info!(id, %peer_addr, "accepted connection");
        Ok((conn, peer_addr))
    }

    /// Accept the next incoming connection, or return `None` if shutdown is
    /// signaled before a connection arrives.
    ///
    /// This is the shutdown-aware version of [`Listener::accept`].
    #[instrument(skip(self, shutdown), fields(local = %self.inner.local_addr().unwrap()))]
    pub async fn accept_with_shutdown(
        &self,
        shutdown: &mut ShutdownSignal,
    ) -> Result<Option<(Connection, SocketAddr)>, NetworkError> {
        trace!("waiting for connection (shutdown-aware)");
        tokio::select! {
            result = self.inner.accept() => {
                let (stream, peer_addr) = result?;
                let conn = Connection::new(stream)?;
                let id = conn.info().id;
                info!(id, %peer_addr, "accepted connection");
                Ok(Some((conn, peer_addr)))
            }
            () = shutdown.recv() => {
                info!("listener shutting down, stopped accepting");
                Ok(None)
            }
        }
    }

    /// Returns the local address this listener is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, NetworkError> {
        Ok(self.inner.local_addr()?)
    }
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("local_addr", &self.inner.local_addr().ok())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::AsyncTransport;
    use crate::shutdown::ShutdownSignal;
    use pirc_protocol::{Command, Message};
    use std::time::Duration;
    use tokio::net::TcpStream;

    /// Helper: bind a Listener on a random loopback port.
    async fn loopback_listener() -> Listener {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        Listener::bind(addr).await.unwrap()
    }

    #[tokio::test]
    async fn bind_and_local_addr() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn accept_returns_connection_and_peer_addr() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let client_local = client_stream.local_addr().unwrap();

        let (conn, peer_addr) = listener.accept().await.unwrap();
        assert_eq!(peer_addr, client_local);
        assert!(conn.info().peer_addr.ip().is_loopback());
    }

    #[tokio::test]
    async fn accept_produces_connection_with_unique_id() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        // Connect two clients
        let _c1 = TcpStream::connect(addr).await.unwrap();
        let (conn1, _) = listener.accept().await.unwrap();

        let _c2 = TcpStream::connect(addr).await.unwrap();
        let (conn2, _) = listener.accept().await.unwrap();

        assert_ne!(conn1.info().id, conn2.info().id);
    }

    #[tokio::test]
    async fn accepted_connection_receives_messages() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        // Client connects and sends a message via raw framed stream
        let client_stream = TcpStream::connect(addr).await.unwrap();
        let mut client = Connection::new(client_stream).unwrap();

        let (mut server_conn, _) = listener.accept().await.unwrap();

        let msg = Message::new(Command::Ping, vec!["hello".to_owned()]);
        client.send(msg.clone()).await.unwrap();

        let received = server_conn.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn multiple_concurrent_connections() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();

        let n = 5;
        let mut client_streams = Vec::new();

        // Open N connections concurrently
        for _ in 0..n {
            client_streams.push(TcpStream::connect(addr).await.unwrap());
        }

        // Accept all N
        let mut server_conns = Vec::new();
        for _ in 0..n {
            let (conn, _) = listener.accept().await.unwrap();
            server_conns.push(conn);
        }

        // Verify all connection IDs are unique
        let ids: Vec<u64> = server_conns.iter().map(|c| c.info().id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "all connection IDs must be unique");

        // Verify we can communicate on each connection
        for (i, client_stream) in client_streams.into_iter().enumerate() {
            let mut client = Connection::new(client_stream).unwrap();
            let msg = Message::new(Command::Ping, vec![format!("conn-{i}")]);
            client.send(msg.clone()).await.unwrap();

            let received = server_conns[i].recv().await.unwrap().unwrap();
            assert_eq!(received, msg);
        }
    }

    #[tokio::test]
    async fn listener_debug_impl() {
        let listener = loopback_listener().await;
        let debug_str = format!("{listener:?}");
        assert!(debug_str.contains("Listener"));
        assert!(debug_str.contains("local_addr"));
    }

    #[tokio::test]
    async fn accept_with_shutdown_returns_none_on_signal() {
        let listener = loopback_listener().await;
        let (controller, mut signal) = ShutdownSignal::new();

        // Signal shutdown immediately
        controller.shutdown();

        let result = listener.accept_with_shutdown(&mut signal).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn accept_with_shutdown_returns_connection_before_signal() {
        let listener = loopback_listener().await;
        let addr = listener.local_addr().unwrap();
        let (_controller, mut signal) = ShutdownSignal::new();

        // Connect a client before triggering shutdown
        let _client = TcpStream::connect(addr).await.unwrap();

        let result = listener.accept_with_shutdown(&mut signal).await.unwrap();
        assert!(result.is_some());
        let (conn, peer_addr) = result.unwrap();
        assert!(peer_addr.ip().is_loopback());
        assert!(conn.info().peer_addr.ip().is_loopback());
    }

    #[tokio::test]
    async fn accept_with_shutdown_stops_accept_loop() {
        let listener = std::sync::Arc::new(loopback_listener().await);
        let (controller, mut signal) = ShutdownSignal::new();

        // Start an accept loop in a task
        let listener_clone = listener.clone();
        let handle = tokio::spawn(async move {
            let mut count = 0u32;
            loop {
                match listener_clone.accept_with_shutdown(&mut signal).await {
                    Ok(Some(_)) => count += 1,
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            count
        });

        // Give the task time to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Signal shutdown
        controller.shutdown();

        let count = handle.await.unwrap();
        assert_eq!(count, 0);
    }
}
