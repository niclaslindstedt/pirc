//! Configuration module for the pirc system.
//!
//! Provides XDG-compatible path resolution for configuration files and
//! directories used by both the client (`pirc`) and server (`pircd`).

pub mod paths;

pub use paths::{
    config_dir, default_client_config_path, default_server_config_path, plugins_dir, scripts_dir,
};
