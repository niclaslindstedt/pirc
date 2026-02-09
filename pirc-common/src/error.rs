//! Error types for the pirc system.
//!
//! Provides a three-level hierarchy: domain-specific errors ([`ChannelError`],
//! [`UserError`]) that convert into the top-level [`PircError`] via `From` impls,
//! plus a [`Result<T>`] alias for ergonomic `?` usage.

use std::io;

/// Errors related to IRC channel operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelError {
    #[error("channel not found: {channel}")]
    NotFound { channel: String },
    #[error("already joined channel: {channel}")]
    AlreadyJoined { channel: String },
    #[error("banned from channel: {channel}")]
    Banned { channel: String },
    #[error("invalid channel name '{name}': {reason}")]
    InvalidName { name: String, reason: String },
    #[error("not in channel: {channel}")]
    NotInChannel { channel: String },
}

/// Errors related to IRC user operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    #[error("nickname already in use: {nick}")]
    NickInUse { nick: String },
    #[error("invalid nickname '{nick}': {reason}")]
    InvalidNick { nick: String, reason: String },
    #[error("not an operator")]
    NotOperator,
    #[error("user not found: {nick}")]
    NotFound { nick: String },
}

/// Top-level error type for the pirc system.
#[derive(Debug, thiserror::Error)]
pub enum PircError {
    #[error("protocol error: {message}")]
    ProtocolError { message: String },
    #[error("connection error: {message}")]
    ConnectionError { message: String },
    #[error(transparent)]
    ChannelError(#[from] ChannelError),
    #[error(transparent)]
    UserError(#[from] UserError),
    #[error("crypto error: {message}")]
    CryptoError { message: String },
    #[error("config error: {message}")]
    ConfigError { message: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A convenience result type that uses [`PircError`] as the error variant.
pub type Result<T> = std::result::Result<T, PircError>;

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ChannelError construction ----

    #[test]
    fn channel_not_found() {
        let err = ChannelError::NotFound {
            channel: "#test".into(),
        };
        assert_eq!(err.to_string(), "channel not found: #test");
    }

    #[test]
    fn channel_already_joined() {
        let err = ChannelError::AlreadyJoined {
            channel: "#test".into(),
        };
        assert_eq!(err.to_string(), "already joined channel: #test");
    }

    #[test]
    fn channel_banned() {
        let err = ChannelError::Banned {
            channel: "#test".into(),
        };
        assert_eq!(err.to_string(), "banned from channel: #test");
    }

    #[test]
    fn channel_invalid_name() {
        let err = ChannelError::InvalidName {
            name: "bad".into(),
            reason: "missing # prefix".into(),
        };
        assert_eq!(
            err.to_string(),
            "invalid channel name 'bad': missing # prefix"
        );
    }

    #[test]
    fn channel_not_in_channel() {
        let err = ChannelError::NotInChannel {
            channel: "#test".into(),
        };
        assert_eq!(err.to_string(), "not in channel: #test");
    }

    // ---- UserError construction ----

    #[test]
    fn user_nick_in_use() {
        let err = UserError::NickInUse {
            nick: "alice".into(),
        };
        assert_eq!(err.to_string(), "nickname already in use: alice");
    }

    #[test]
    fn user_invalid_nick() {
        let err = UserError::InvalidNick {
            nick: "123bad".into(),
            reason: "must start with a letter".into(),
        };
        assert_eq!(
            err.to_string(),
            "invalid nickname '123bad': must start with a letter"
        );
    }

    #[test]
    fn user_not_operator() {
        let err = UserError::NotOperator;
        assert_eq!(err.to_string(), "not an operator");
    }

    #[test]
    fn user_not_found() {
        let err = UserError::NotFound { nick: "bob".into() };
        assert_eq!(err.to_string(), "user not found: bob");
    }

    // ---- PircError construction ----

    #[test]
    fn pirc_protocol_error() {
        let err = PircError::ProtocolError {
            message: "malformed message".into(),
        };
        assert_eq!(err.to_string(), "protocol error: malformed message");
    }

    #[test]
    fn pirc_connection_error() {
        let err = PircError::ConnectionError {
            message: "connection refused".into(),
        };
        assert_eq!(err.to_string(), "connection error: connection refused");
    }

    #[test]
    fn pirc_crypto_error() {
        let err = PircError::CryptoError {
            message: "key exchange failed".into(),
        };
        assert_eq!(err.to_string(), "crypto error: key exchange failed");
    }

    #[test]
    fn pirc_config_error() {
        let err = PircError::ConfigError {
            message: "invalid toml".into(),
        };
        assert_eq!(err.to_string(), "config error: invalid toml");
    }

    #[test]
    fn pirc_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = PircError::Io(io_err);
        assert_eq!(err.to_string(), "file not found");
    }

    // ---- From conversions ----

    #[test]
    fn channel_error_into_pirc_error() {
        let channel_err = ChannelError::NotFound {
            channel: "#test".into(),
        };
        let pirc_err: PircError = channel_err.into();
        assert!(matches!(pirc_err, PircError::ChannelError(_)));
        assert_eq!(pirc_err.to_string(), "channel not found: #test");
    }

    #[test]
    fn user_error_into_pirc_error() {
        let user_err = UserError::NickInUse {
            nick: "alice".into(),
        };
        let pirc_err: PircError = user_err.into();
        assert!(matches!(pirc_err, PircError::UserError(_)));
        assert_eq!(pirc_err.to_string(), "nickname already in use: alice");
    }

    #[test]
    fn io_error_into_pirc_error() {
        let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
        let pirc_err: PircError = io_err.into();
        assert!(matches!(pirc_err, PircError::Io(_)));
    }

    // ---- From in function context (? operator) ----

    fn returns_pirc_result_from_channel() -> Result<()> {
        Err(ChannelError::NotFound {
            channel: "#test".into(),
        })?
    }

    fn returns_pirc_result_from_user() -> Result<()> {
        Err(UserError::NotOperator)?
    }

    fn returns_pirc_result_from_io() -> Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "oops"))?
    }

    #[test]
    fn question_mark_channel_error() {
        let result = returns_pirc_result_from_channel();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PircError::ChannelError(ChannelError::NotFound { .. })
        ));
    }

    #[test]
    fn question_mark_user_error() {
        let result = returns_pirc_result_from_user();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PircError::UserError(UserError::NotOperator)
        ));
    }

    #[test]
    fn question_mark_io_error() {
        let result = returns_pirc_result_from_io();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PircError::Io(_)));
    }

    // ---- Result alias ----

    #[test]
    fn result_alias_ok() {
        let result: Result<i32> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn result_alias_err() {
        let result: Result<i32> = Err(PircError::ProtocolError {
            message: "test".into(),
        });
        assert!(result.is_err());
    }

    // ---- Error trait ----

    #[test]
    fn pirc_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(PircError::ProtocolError {
            message: "test".into(),
        });
        assert_eq!(err.to_string(), "protocol error: test");
    }

    #[test]
    fn channel_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(ChannelError::NotFound {
            channel: "#test".into(),
        });
        assert_eq!(err.to_string(), "channel not found: #test");
    }

    #[test]
    fn user_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(UserError::NotOperator);
        assert_eq!(err.to_string(), "not an operator");
    }

    // ---- Source chain ----
    //
    // With #[error(transparent)], source() delegates to the inner error's source().
    // ChannelError and UserError are leaf errors (no source), so source() is None.
    // Io wraps std::io::Error, which may or may not have a source depending on construction.

    #[test]
    fn io_error_display_is_preserved() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let pirc_err = PircError::Io(io_err);
        assert_eq!(pirc_err.to_string(), "file missing");
    }

    #[test]
    fn channel_error_transparent_display() {
        let channel_err = ChannelError::NotFound {
            channel: "#test".into(),
        };
        let pirc_err = PircError::ChannelError(channel_err);
        assert_eq!(pirc_err.to_string(), "channel not found: #test");
    }

    #[test]
    fn user_error_transparent_display() {
        let user_err = UserError::NotOperator;
        let pirc_err = PircError::UserError(user_err);
        assert_eq!(pirc_err.to_string(), "not an operator");
    }
}
