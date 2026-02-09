/// Errors that can occur when parsing a protocol message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// The input was empty or contained only whitespace.
    #[error("empty message")]
    EmptyMessage,

    /// The input exceeded the maximum allowed length.
    #[error("message too long ({length} bytes, max {max})")]
    MessageTooLong { length: usize, max: usize },

    /// No command was found in the message.
    #[error("missing command")]
    MissingCommand,

    /// The command string was not recognized.
    #[error("unknown command: {0}")]
    UnknownCommand(String),

    /// The prefix was malformed (e.g., missing `@` in user prefix).
    #[error("invalid prefix: {0}")]
    InvalidPrefix(String),

    /// A nickname in the prefix was invalid.
    #[error("invalid nickname in prefix: {0}")]
    InvalidNickname(String),

    /// Too many parameters (exceeds the 15-parameter limit).
    #[error("too many parameters ({count}, max {max})")]
    TooManyParams { count: usize, max: usize },

    /// An invalid protocol version string.
    #[error("invalid version: {0}")]
    InvalidVersion(String),
}

impl ProtocolError {
    /// Returns a short identifier for the error kind, useful for logging.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyMessage => "empty_message",
            Self::MessageTooLong { .. } => "message_too_long",
            Self::MissingCommand => "missing_command",
            Self::UnknownCommand(_) => "unknown_command",
            Self::InvalidPrefix(_) => "invalid_prefix",
            Self::InvalidNickname(_) => "invalid_nickname",
            Self::TooManyParams { .. } => "too_many_params",
            Self::InvalidVersion(_) => "invalid_version",
        }
    }
}

// Allow conversion into the common PircError
impl From<ProtocolError> for pirc_common::PircError {
    fn from(err: ProtocolError) -> Self {
        Self::ProtocolError {
            message: err.to_string(),
        }
    }
}
