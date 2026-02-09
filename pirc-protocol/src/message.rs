use std::fmt;

use crate::command::Command;
use crate::prefix::Prefix;

/// Maximum number of parameters in an IRC message (per RFC 2812).
pub const MAX_PARAMS: usize = 15;

/// A parsed IRC protocol message.
///
/// Represents the wire format: `:<prefix> <command> <params...> :<trailing>\r\n`
///
/// - `prefix` is optional and identifies the message source.
/// - `command` is the IRC command or numeric reply.
/// - `params` holds up to [`MAX_PARAMS`] parameters. The last parameter may
///   contain spaces if it was a "trailing" parameter (prefixed with `:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Optional message source (server or user).
    pub prefix: Option<Prefix>,
    /// The command or numeric reply.
    pub command: Command,
    /// Command parameters (max 15).
    pub params: Vec<String>,
}

impl Message {
    /// Create a new message with no prefix.
    pub fn new(command: Command, params: Vec<String>) -> Self {
        Self {
            prefix: None,
            command,
            params,
        }
    }

    /// Create a new message with a prefix.
    pub fn with_prefix(prefix: Prefix, command: Command, params: Vec<String>) -> Self {
        Self {
            prefix: Some(prefix),
            command,
            params,
        }
    }

    /// Returns `true` if this message is a numeric reply.
    pub fn is_numeric(&self) -> bool {
        matches!(self.command, Command::Numeric(_))
    }

    /// Returns the numeric code if this is a numeric reply, or `None`.
    pub fn numeric_code(&self) -> Option<u16> {
        match self.command {
            Command::Numeric(code) => Some(code),
            _ => None,
        }
    }

    /// Returns the trailing (last) parameter, if any.
    pub fn trailing(&self) -> Option<&str> {
        self.params.last().map(String::as_str)
    }

    /// Create a builder for constructing a message.
    ///
    /// # Examples
    ///
    /// ```
    /// use pirc_protocol::{Command, Message, Prefix};
    ///
    /// let msg = Message::builder(Command::Privmsg)
    ///     .prefix(Prefix::Server("irc.example.com".to_owned()))
    ///     .param("#general")
    ///     .trailing("Hello, world!")
    ///     .build();
    ///
    /// assert_eq!(msg.to_string(), ":irc.example.com PRIVMSG #general :Hello, world!");
    /// ```
    pub fn builder(command: Command) -> MessageBuilder {
        MessageBuilder {
            prefix: None,
            command,
            params: Vec::new(),
        }
    }
}

/// Builder for constructing [`Message`] values ergonomically.
///
/// Obtained via [`Message::builder`].
#[derive(Debug, Clone)]
#[must_use]
pub struct MessageBuilder {
    prefix: Option<Prefix>,
    command: Command,
    params: Vec<String>,
}

impl MessageBuilder {
    /// Set the message prefix (source).
    pub fn prefix(mut self, prefix: Prefix) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Append a parameter.
    pub fn param(mut self, value: &str) -> Self {
        self.params.push(value.to_owned());
        self
    }

    /// Append a trailing parameter (the last parameter, which may contain spaces).
    ///
    /// This is semantically identical to [`param`](Self::param) — the
    /// `Display` implementation decides whether to format the last parameter
    /// with a `:` prefix based on its content. This method exists for clarity
    /// when building messages.
    pub fn trailing(self, value: &str) -> Self {
        self.param(value)
    }

    /// Consume the builder and produce the [`Message`].
    pub fn build(self) -> Message {
        Message {
            prefix: self.prefix,
            command: self.command,
            params: self.params,
        }
    }
}

impl fmt::Display for Message {
    /// Formats the message in IRC wire format.
    ///
    /// The trailing `\r\n` is **not** included — callers should append it
    /// when writing to a transport.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(prefix) = &self.prefix {
            write!(f, ":{prefix} ")?;
        }
        write!(f, "{}", self.command)?;

        // For PIRC commands, inject the subcommand keyword before params.
        if let Command::Pirc(sub) = &self.command {
            write!(f, " {sub}")?;
        }

        if !self.params.is_empty() {
            // All params except possibly the last are simple (no spaces).
            let (last, rest) = self.params.split_last().expect("non-empty params");
            for param in rest {
                write!(f, " {param}")?;
            }
            // The last parameter is written as trailing if it contains
            // spaces or is empty, or if it starts with ':'.
            if last.is_empty() || last.contains(' ') || last.starts_with(':') {
                write!(f, " :{last}")?;
            } else {
                write!(f, " {last}")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
