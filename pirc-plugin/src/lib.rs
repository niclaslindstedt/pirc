//! # pirc-plugin
//!
//! Native plugin system for the pirc IRC client. This crate defines the
//! stable C FFI ABI used for loading plugins as dynamic libraries at runtime.
//!
//! Plugins are compiled as `.dylib` (macOS) or `.so` (Linux) files and placed
//! in `~/.pirc/plugins/`. The host loads them via `libloading`, looks up the
//! `pirc_plugin_init` symbol, and interacts with the plugin through the
//! [`ffi::PluginApi`] vtable.

pub mod ffi;
