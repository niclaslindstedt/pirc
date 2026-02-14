//! Common types and error handling for the pirc IRC system.
//!
//! This crate provides the shared foundation used across all pirc crates:
//!
//! - **Validated types** — [`Nickname`], [`ChannelName`], [`ServerId`], [`UserId`]
//! - **IRC modes** — [`ChannelMode`], [`UserMode`]
//! - **Error hierarchy** — [`PircError`], [`ChannelError`], [`UserError`]
//! - **Configuration** — [`config`] module with XDG-compatible path resolution
//! - **Convenience alias** — [`Result<T>`] using [`PircError`]
//!
//! All types enforce IRC protocol invariants at construction time and support
//! serde serialization.

pub mod config;
pub mod error;
pub mod types;

pub use error::{ChannelError, PircError, RaftError, Result, UserError};
pub use types::{ChannelMode, ChannelName, Nickname, ServerId, UserId, UserMode};
