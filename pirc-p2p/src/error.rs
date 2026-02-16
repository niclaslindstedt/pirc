//! Error types for the `pirc-p2p` crate.

use std::net::SocketAddr;

/// Errors produced by P2P operations.
#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    /// STUN protocol error (malformed message, unexpected response, etc.).
    #[error("STUN error: {0}")]
    Stun(String),

    /// STUN request timed out waiting for a response.
    #[error("STUN request to {0} timed out")]
    StunTimeout(SocketAddr),

    /// TURN protocol error.
    #[error("TURN error: {0}")]
    Turn(String),

    /// TURN request timed out waiting for a response.
    #[error("TURN request to {0} timed out")]
    TurnTimeout(SocketAddr),

    /// Network I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A convenience result type that uses [`P2pError`] as the error variant.
pub type Result<T> = std::result::Result<T, P2pError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_error_display() {
        let err = P2pError::Stun("bad magic cookie".into());
        assert_eq!(err.to_string(), "STUN error: bad magic cookie");
    }

    #[test]
    fn stun_timeout_display() {
        let addr: SocketAddr = "1.2.3.4:3478".parse().unwrap();
        let err = P2pError::StunTimeout(addr);
        assert_eq!(err.to_string(), "STUN request to 1.2.3.4:3478 timed out");
    }

    #[test]
    fn io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = P2pError::from(io_err);
        assert!(err.to_string().contains("refused"));
    }

    #[test]
    fn p2p_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(P2pError::Stun("test".into()));
        assert_eq!(err.to_string(), "STUN error: test");
    }
}
