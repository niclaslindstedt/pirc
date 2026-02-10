//! Connection traits and base connection type.
//!
//! Provides [`Connection`], which wraps a framed TCP stream and delivers typed
//! [`Message`] I/O, along with the [`AsyncTransport`] trait that abstracts over
//! the underlying transport (TCP today, TLS in the future).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::{SinkExt, StreamExt};
use pirc_protocol::Message;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::{debug, trace};

use crate::codec::PircCodec;
use crate::error::NetworkError;
use crate::shutdown::ShutdownSignal;

// ---------------------------------------------------------------------------
// Connection ID generator
// ---------------------------------------------------------------------------

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

fn next_connection_id() -> u64 {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// ConnectionInfo
// ---------------------------------------------------------------------------

/// Metadata about a connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Unique connection identifier.
    pub id: u64,
    /// Remote peer address.
    pub peer_addr: SocketAddr,
    /// Time the connection was established.
    pub connected_at: Instant,
    /// Total bytes sent over this connection.
    pub bytes_sent: u64,
    /// Total bytes received over this connection.
    pub bytes_received: u64,
}

impl ConnectionInfo {
    /// Creates a new `ConnectionInfo` for the given peer address.
    pub fn new(peer_addr: SocketAddr) -> Self {
        Self {
            id: next_connection_id(),
            peer_addr,
            connected_at: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// AsyncTransport trait
// ---------------------------------------------------------------------------

/// Trait abstracting an async transport capable of sending and receiving
/// IRC [`Message`] values.
///
/// This trait allows the networking layer to work with different underlying
/// transports (plain TCP, TLS, etc.) through a common interface.
pub trait AsyncTransport {
    /// Send a message over the transport.
    fn send(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), NetworkError>> + Send;

    /// Receive the next message from the transport.
    ///
    /// Returns `Ok(None)` when the remote end has closed the connection.
    fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<Message>, NetworkError>> + Send;

    /// Gracefully shut down the transport (flush pending writes, then close).
    fn shutdown(&mut self) -> impl std::future::Future<Output = Result<(), NetworkError>> + Send;

    /// Returns the remote peer address.
    fn peer_addr(&self) -> Result<SocketAddr, NetworkError>;
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

/// A connection wrapping a framed TCP stream that provides typed IRC message I/O.
///
/// `Connection` owns a [`Framed<TcpStream, PircCodec>`] and exposes ergonomic
/// async methods for sending and receiving [`Message`] values. It also carries
/// [`ConnectionInfo`] metadata about the connection.
pub struct Connection {
    framed: Framed<TcpStream, PircCodec>,
    info: ConnectionInfo,
}

impl Connection {
    /// Wrap an already-connected [`TcpStream`] into a `Connection`.
    pub fn new(stream: TcpStream) -> Result<Self, NetworkError> {
        let peer_addr = stream.peer_addr()?;
        let info = ConnectionInfo::new(peer_addr);
        let framed = Framed::new(stream, PircCodec::new());
        debug!(id = info.id, %peer_addr, "connection created");
        Ok(Self { framed, info })
    }

    /// Returns a reference to the connection metadata.
    pub fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    /// Receive the next message, or flush and close if shutdown is signaled.
    ///
    /// Returns `Ok(Some(msg))` for a normal message, `Ok(None)` if shutdown
    /// was signaled (after flushing pending writes and closing the connection),
    /// or `Err` on I/O errors.
    pub async fn recv_with_shutdown(
        &mut self,
        shutdown: &mut ShutdownSignal,
    ) -> Result<Option<Message>, NetworkError> {
        tokio::select! {
            result = self.framed.next() => {
                match result {
                    Some(Ok(msg)) => {
                        trace!(id = self.info.id, ?msg, "received message");
                        let wire_len = (msg.to_string().len() + 2) as u64;
                        self.info.bytes_received += wire_len;
                        Ok(Some(msg))
                    }
                    Some(Err(e)) => Err(e),
                    None => {
                        debug!(id = self.info.id, "connection EOF");
                        Ok(None)
                    }
                }
            }
            () = shutdown.recv() => {
                debug!(id = self.info.id, "shutdown signaled, flushing and closing");
                self.framed.close().await?;
                Ok(None)
            }
        }
    }
}

impl AsyncTransport for Connection {
    async fn send(&mut self, msg: Message) -> Result<(), NetworkError> {
        trace!(id = self.info.id, ?msg, "sending message");
        // Calculate wire size before sending: message text + \r\n
        let wire_len = (msg.to_string().len() + 2) as u64;
        self.framed.send(msg).await?;
        self.info.bytes_sent += wire_len;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Message>, NetworkError> {
        match self.framed.next().await {
            Some(Ok(msg)) => {
                trace!(id = self.info.id, ?msg, "received message");
                // Calculate wire size: message text + \r\n
                let wire_len = (msg.to_string().len() + 2) as u64;
                self.info.bytes_received += wire_len;
                Ok(Some(msg))
            }
            Some(Err(e)) => Err(e),
            None => {
                debug!(id = self.info.id, "connection EOF");
                Ok(None)
            }
        }
    }

    async fn shutdown(&mut self) -> Result<(), NetworkError> {
        debug!(id = self.info.id, "shutting down connection");
        self.framed.close().await?;
        Ok(())
    }

    fn peer_addr(&self) -> Result<SocketAddr, NetworkError> {
        Ok(self.info.peer_addr)
    }
}

// ---------------------------------------------------------------------------
// Debug impl (avoid dumping the framed internals)
// ---------------------------------------------------------------------------

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownSignal;
    use pirc_protocol::{Command, Prefix};
    use std::time::Duration;
    use tokio::net::TcpListener;

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
    async fn send_recv_simple_message() {
        let (mut client, mut server) = loopback_pair().await;

        let msg = Message::new(Command::Ping, vec!["hello".to_owned()]);
        client.send(msg.clone()).await.unwrap();

        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn send_recv_message_with_prefix() {
        let (mut client, mut server) = loopback_pair().await;

        let msg = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Privmsg,
            vec!["#channel".to_owned(), "Hello, world!".to_owned()],
        );

        client.send(msg.clone()).await.unwrap();
        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn send_recv_multiple_messages() {
        let (mut client, mut server) = loopback_pair().await;

        let messages = vec![
            Message::new(Command::Ping, vec!["server1".to_owned()]),
            Message::new(Command::Pong, vec!["server2".to_owned()]),
            Message::new(Command::Nick, vec!["testuser".to_owned()]),
        ];

        for msg in &messages {
            client.send(msg.clone()).await.unwrap();
        }

        for expected in &messages {
            let received = server.recv().await.unwrap().unwrap();
            assert_eq!(&received, expected);
        }
    }

    #[tokio::test]
    async fn recv_returns_none_on_eof() {
        let (client, mut server) = loopback_pair().await;

        // Drop the client side to cause EOF on the server side
        drop(client);

        let result = server.recv().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn peer_addr_returns_remote_address() {
        let (client, server) = loopback_pair().await;

        let client_peer = client.peer_addr().unwrap();
        let server_peer = server.peer_addr().unwrap();

        // Client's peer is the server's local addr and vice-versa
        assert_eq!(client_peer.ip(), server_peer.ip());
        assert_ne!(client_peer.port(), server_peer.port());
    }

    #[tokio::test]
    async fn connection_info_has_unique_ids() {
        let (c1, s1) = loopback_pair().await;
        let (c2, s2) = loopback_pair().await;

        let ids: Vec<u64> = vec![c1.info().id, s1.info().id, c2.info().id, s2.info().id];
        // All IDs should be unique
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "connection IDs must be unique");
    }

    #[tokio::test]
    async fn connection_info_stores_peer_addr() {
        let (client, _server) = loopback_pair().await;

        let info = client.info();
        assert_eq!(info.peer_addr, client.peer_addr().unwrap());
        assert!(info.peer_addr.ip().is_loopback());
    }

    #[tokio::test]
    async fn shutdown_flushes_and_closes() {
        let (mut client, mut server) = loopback_pair().await;

        // Send a message, then shut down
        let msg = Message::new(Command::Quit, vec!["goodbye".to_owned()]);
        client.send(msg.clone()).await.unwrap();
        client.shutdown().await.unwrap();

        // Server should still receive the message
        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);

        // After the flushed message, server should see EOF
        let eof = server.recv().await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn bidirectional_communication() {
        let (mut client, mut server) = loopback_pair().await;

        // Client sends to server
        let ping = Message::new(Command::Ping, vec!["check".to_owned()]);
        client.send(ping.clone()).await.unwrap();
        let received = server.recv().await.unwrap().unwrap();
        assert_eq!(received, ping);

        // Server responds to client
        let pong = Message::new(Command::Pong, vec!["check".to_owned()]);
        server.send(pong.clone()).await.unwrap();
        let received = client.recv().await.unwrap().unwrap();
        assert_eq!(received, pong);
    }

    #[tokio::test]
    async fn connection_debug_impl() {
        let (client, _server) = loopback_pair().await;
        let debug_str = format!("{client:?}");
        assert!(debug_str.contains("Connection"));
        assert!(debug_str.contains("info"));
    }

    #[tokio::test]
    async fn bytes_counters_initialized_to_zero() {
        let (client, server) = loopback_pair().await;
        assert_eq!(client.info().bytes_sent, 0);
        assert_eq!(client.info().bytes_received, 0);
        assert_eq!(server.info().bytes_sent, 0);
        assert_eq!(server.info().bytes_received, 0);
    }

    #[tokio::test]
    async fn bytes_sent_incremented_on_send() {
        let (mut client, mut server) = loopback_pair().await;

        let msg = Message::new(Command::Ping, vec!["hello".to_owned()]);
        // Wire format: "PING hello\r\n" = 12 bytes
        let expected_bytes = (msg.to_string().len() + 2) as u64;

        client.send(msg).await.unwrap();
        assert_eq!(client.info().bytes_sent, expected_bytes);
        assert_eq!(client.info().bytes_received, 0);

        // Consume on server side to verify bytes_received
        let _ = server.recv().await.unwrap().unwrap();
        assert_eq!(server.info().bytes_received, expected_bytes);
        assert_eq!(server.info().bytes_sent, 0);
    }

    #[tokio::test]
    async fn bytes_counters_accumulate_over_multiple_messages() {
        let (mut client, mut server) = loopback_pair().await;

        let messages = vec![
            Message::new(Command::Ping, vec!["server1".to_owned()]),
            Message::new(Command::Pong, vec!["server2".to_owned()]),
            Message::new(Command::Nick, vec!["testuser".to_owned()]),
        ];

        let mut total_bytes: u64 = 0;
        for msg in &messages {
            total_bytes += (msg.to_string().len() + 2) as u64;
            client.send(msg.clone()).await.unwrap();
        }

        assert_eq!(client.info().bytes_sent, total_bytes);

        for _ in &messages {
            let _ = server.recv().await.unwrap().unwrap();
        }

        assert_eq!(server.info().bytes_received, total_bytes);
    }

    #[tokio::test]
    async fn bytes_counters_track_bidirectional() {
        let (mut client, mut server) = loopback_pair().await;

        let ping = Message::new(Command::Ping, vec!["check".to_owned()]);
        let pong = Message::new(Command::Pong, vec!["check".to_owned()]);

        let ping_bytes = (ping.to_string().len() + 2) as u64;
        let pong_bytes = (pong.to_string().len() + 2) as u64;

        // Client sends ping
        client.send(ping.clone()).await.unwrap();
        let _ = server.recv().await.unwrap().unwrap();

        // Server sends pong
        server.send(pong.clone()).await.unwrap();
        let _ = client.recv().await.unwrap().unwrap();

        assert_eq!(client.info().bytes_sent, ping_bytes);
        assert_eq!(client.info().bytes_received, pong_bytes);
        assert_eq!(server.info().bytes_received, ping_bytes);
        assert_eq!(server.info().bytes_sent, pong_bytes);
    }

    #[tokio::test]
    async fn recv_with_shutdown_returns_none_on_signal() {
        let (mut client, mut server) = loopback_pair().await;
        let (controller, mut signal) = ShutdownSignal::new();

        // Signal shutdown immediately
        controller.shutdown();

        let result = server.recv_with_shutdown(&mut signal).await.unwrap();
        assert!(result.is_none());

        // The server connection should be closed — client sees EOF
        let eof = client.recv().await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn recv_with_shutdown_receives_messages_before_signal() {
        let (mut client, mut server) = loopback_pair().await;
        let (_controller, mut signal) = ShutdownSignal::new();

        let msg = Message::new(Command::Ping, vec!["hello".to_owned()]);
        client.send(msg.clone()).await.unwrap();

        let result = server.recv_with_shutdown(&mut signal).await.unwrap();
        assert_eq!(result, Some(msg));
    }

    #[tokio::test]
    async fn recv_with_shutdown_flushes_before_close() {
        let (mut client, mut server) = loopback_pair().await;
        let (controller, mut signal) = ShutdownSignal::new();

        // Server sends a message, then gets shutdown signaled
        let msg = Message::new(Command::Quit, vec!["bye".to_owned()]);
        server.send(msg.clone()).await.unwrap();

        controller.shutdown();
        let result = server.recv_with_shutdown(&mut signal).await.unwrap();
        assert!(result.is_none());

        // Client should still receive the message that was sent before shutdown
        let received = client.recv().await.unwrap().unwrap();
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn recv_with_shutdown_during_active_exchange() {
        let (mut client, mut server) = loopback_pair().await;
        let (controller, mut signal) = ShutdownSignal::new();

        // Exchange a few messages first
        let msg1 = Message::new(Command::Ping, vec!["1".to_owned()]);
        let msg2 = Message::new(Command::Pong, vec!["2".to_owned()]);

        client.send(msg1.clone()).await.unwrap();
        let r1 = server.recv_with_shutdown(&mut signal).await.unwrap();
        assert_eq!(r1, Some(msg1));

        server.send(msg2.clone()).await.unwrap();
        let r2 = client.recv().await.unwrap().unwrap();
        assert_eq!(r2, msg2);

        // Now signal shutdown
        controller.shutdown();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            server.recv_with_shutdown(&mut signal),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.is_none());
    }
}
