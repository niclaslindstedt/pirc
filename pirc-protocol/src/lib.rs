//! Wire protocol types for the pirc IRC system.
//!
//! This crate defines the core data structures for the pirc text-based protocol,
//! which is inspired by IRC with extensions for encryption, clustering, and P2P.
//!
//! # Wire format
//!
//! Each message is a single line terminated by `\r\n`:
//!
//! ```text
//! :<prefix> <command> <params...> :<trailing>\r\n
//! ```
//!
//! - **Prefix** — optional, identifies the source ([`Prefix`])
//! - **Command** — uppercase keyword or 3-digit numeric ([`Command`])
//! - **Params** — space-separated, max 15 ([`message::MAX_PARAMS`])
//! - **Trailing** — final parameter prefixed with `:`, may contain spaces
//!
//! # Numeric replies
//!
//! Standard IRC numeric reply codes are defined as constants in the
//! [`numeric`] module (e.g., [`numeric::RPL_WELCOME`], [`numeric::ERR_NICKNAMEINUSE`]).

pub mod command;
pub mod error;
pub mod message;
pub mod numeric;
pub mod parser;
pub mod prefix;
pub mod version;

pub use command::{Command, PircSubcommand};
pub use error::ProtocolError;
pub use message::{Message, MessageBuilder};
pub use parser::parse;
pub use prefix::Prefix;
pub use version::{ProtocolVersion, PROTOCOL_VERSION_CURRENT};
