//! Async TCP networking layer for the pirc IRC system.
//!
//! This crate provides the networking infrastructure on top of the wire protocol
//! defined in [`pirc_protocol`]. It handles connection management, framed message
//! I/O, connection pooling, backpressure, and graceful shutdown.
//!
//! # Modules
//!
//! - [`codec`] — Framed message codec for reading/writing IRC messages over TCP
//! - [`connection`] — Connection traits and base connection type
//! - [`listener`] — TCP listener and connection acceptor
//! - [`connector`] — Outbound TCP connector with reconnection
//! - [`pool`] — Connection pooling for server-to-server links
//! - [`shutdown`] — Graceful shutdown coordination
//! - [`error`] — Error types for the networking layer

pub mod backpressure;
pub mod codec;
pub mod connection;
pub mod connector;
pub mod error;
pub mod listener;
pub mod pool;
pub mod shutdown;

pub use backpressure::{
    BackpressureController, BoundedChannel, BoundedReceiver, BoundedSender, ReadLimiter,
    WriteConfig, DEFAULT_READ_LIMIT,
};
pub use connection::{AsyncTransport, Connection, ConnectionInfo};
pub use connector::{Connector, ReconnectPolicy, ReconnectingConnector};
pub use error::NetworkError;
pub use listener::Listener;
pub use pool::{ConnectionPool, ConnectionRef};
