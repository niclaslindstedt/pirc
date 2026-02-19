//! # pirc-plugin
//!
//! Native plugin system for the pirc IRC client. This crate defines the
//! stable C FFI ABI used for loading plugins as dynamic libraries at runtime,
//! plus the safe Rust [`Plugin`](plugin::Plugin) trait that plugin authors
//! implement.
//!
//! Plugins are compiled as `.dylib` (macOS) or `.so` (Linux) files and placed
//! in `~/.pirc/plugins/`. The host loads them via `libloading`, looks up the
//! `pirc_plugin_init` symbol, and interacts with the plugin through the
//! [`ffi::PluginApi`] vtable.
//!
//! Plugin authors implement the [`Plugin`](plugin::Plugin) trait and use the
//! [`declare_plugin!`] macro to generate the C FFI bridge automatically.

pub mod config;
pub mod dispatch;
pub mod ffi;
pub mod loader;
#[macro_use]
pub mod macros;
pub mod manager;
pub mod plugin;
pub mod registry;
