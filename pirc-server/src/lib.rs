//! IRC server implementation for pirc.
//!
//! This crate contains the core server logic including:
//!
//! - **Channels** — [`channel`] and [`channel_registry`] for IRC channel management
//! - **Users** — [`user`] and [`registry`] for connected user tracking
//! - **Groups** — [`group_registry`] for encrypted group management
//! - **Command handlers** — [`handler`], [`handler_channel`], [`handler_cluster`],
//!   [`handler_group`], [`handler_oper`], [`handler_p2p`], [`handler_relay`]
//! - **Clustering** — [`cluster`] server linking with [`raft`] consensus
//! - **Reliability** — [`degraded_mode`], [`failover_queue`], [`graceful_shutdown`]
//! - **Storage** — [`offline_store`], [`prekey_store`] for offline message and key storage
//! - **Migration** — [`migration`] and [`commit_consumer`] for Raft log application
//! - **Configuration** — [`config`] server configuration

pub mod channel;
pub mod channel_registry;
pub mod cluster;
pub mod commit_consumer;
pub mod config;
pub mod degraded_mode;
pub mod failover_queue;
pub mod graceful_shutdown;
pub mod group_registry;
pub mod handler;
pub mod handler_channel;
pub mod handler_cluster;
pub mod handler_group;
pub mod handler_oper;
pub mod handler_p2p;
pub mod handler_relay;
pub mod migration;
pub mod offline_store;
pub mod prekey_store;
pub mod raft;
pub mod registry;
pub mod user;
