//! Convenience re-exports for plugin authors.
//!
//! Import everything a plugin needs with a single glob:
//!
//! ```rust,ignore
//! use pirc_plugin::prelude::*;
//! ```

pub use crate::ffi::{PluginCapability, PluginEventType};
pub use crate::plugin::{LogLevel, Plugin, PluginError, PluginEvent, PluginHost};
