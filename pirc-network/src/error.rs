//! Error types for the networking layer.

use std::io;

use pirc_protocol::ProtocolError;

/// Errors that can occur in the networking layer.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// An I/O error occurred during a network operation.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// A protocol-level error occurred while encoding or decoding a message.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// The connection was closed unexpectedly.
    #[error("connection closed")]
    ConnectionClosed,

    /// A network operation timed out.
    #[error("operation timed out")]
    Timeout,

    /// The connection pool has no available connections.
    #[error("connection pool exhausted")]
    PoolExhausted,
}

impl From<NetworkError> for pirc_common::PircError {
    fn from(err: NetworkError) -> Self {
        Self::ConnectionError {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts() {
        let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
        let net_err = NetworkError::from(io_err);
        assert!(matches!(net_err, NetworkError::Io(_)));
        assert_eq!(net_err.to_string(), "refused");
    }

    #[test]
    fn protocol_error_converts() {
        let proto_err = ProtocolError::EmptyMessage;
        let net_err = NetworkError::from(proto_err);
        assert!(matches!(net_err, NetworkError::Protocol(_)));
        assert_eq!(net_err.to_string(), "protocol error: empty message");
    }

    #[test]
    fn connection_closed_display() {
        let err = NetworkError::ConnectionClosed;
        assert_eq!(err.to_string(), "connection closed");
    }

    #[test]
    fn timeout_display() {
        let err = NetworkError::Timeout;
        assert_eq!(err.to_string(), "operation timed out");
    }

    #[test]
    fn pool_exhausted_display() {
        let err = NetworkError::PoolExhausted;
        assert_eq!(err.to_string(), "connection pool exhausted");
    }

    #[test]
    fn converts_to_pirc_error() {
        let net_err = NetworkError::ConnectionClosed;
        let pirc_err: pirc_common::PircError = net_err.into();
        assert!(matches!(
            pirc_err,
            pirc_common::PircError::ConnectionError { .. }
        ));
        assert_eq!(pirc_err.to_string(), "connection error: connection closed");
    }

    #[test]
    fn io_converts_to_pirc_error() {
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe");
        let net_err = NetworkError::Io(io_err);
        let pirc_err: pirc_common::PircError = net_err.into();
        assert_eq!(pirc_err.to_string(), "connection error: broken pipe");
    }

    #[test]
    fn network_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(NetworkError::Timeout);
        assert_eq!(err.to_string(), "operation timed out");
    }
}
