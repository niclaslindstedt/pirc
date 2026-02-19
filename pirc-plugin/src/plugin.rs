//! Safe Rust Plugin trait and host interface for pirc plugins.
//!
//! This module provides the high-level Rust API that plugin authors implement.
//! The [`Plugin`] trait wraps the raw C FFI ABI defined in [`crate::ffi`],
//! and the [`declare_plugin!`](crate::declare_plugin) macro bridges the two.

use std::fmt;

use crate::ffi::{PluginCapability, PluginEventType};

// ---------------------------------------------------------------------------
// LogLevel
// ---------------------------------------------------------------------------

/// Log severity levels for plugin log messages.
///
/// Matches the numeric levels used in [`crate::ffi::PluginHostApi::log`]:
/// 0=error, 1=warn, 2=info, 3=debug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Unrecoverable or critical errors.
    Error = 0,
    /// Potentially harmful situations.
    Warn = 1,
    /// Informational messages.
    Info = 2,
    /// Detailed debug information.
    Debug = 3,
}

impl LogLevel {
    /// Converts a raw FFI `u32` level into a [`LogLevel`].
    ///
    /// Unknown values map to [`LogLevel::Debug`].
    #[must_use]
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Error,
            1 => Self::Warn,
            2 => Self::Info,
            _ => Self::Debug,
        }
    }
}

// ---------------------------------------------------------------------------
// PluginError
// ---------------------------------------------------------------------------

/// Errors returned by plugin lifecycle and event methods.
#[derive(Debug)]
pub enum PluginError {
    /// Plugin initialisation failed.
    InitFailed(String),
    /// Plugin shutdown failed.
    ShutdownFailed(String),
    /// A command handler failed.
    CommandFailed(String),
    /// An event handler failed.
    EventFailed(String),
    /// The plugin attempted an action it lacks the capability for.
    PermissionDenied {
        /// Name of the plugin that was denied.
        plugin: String,
        /// Human-readable description of the denied action.
        action: String,
    },
    /// Catch-all for other failures.
    Other(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitFailed(msg) => write!(f, "init failed: {msg}"),
            Self::ShutdownFailed(msg) => write!(f, "shutdown failed: {msg}"),
            Self::CommandFailed(msg) => write!(f, "command failed: {msg}"),
            Self::EventFailed(msg) => write!(f, "event failed: {msg}"),
            Self::PermissionDenied { plugin, action } => {
                write!(f, "plugin `{plugin}` denied: {action}")
            }
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for PluginError {}

// ---------------------------------------------------------------------------
// PluginEvent (safe wrapper)
// ---------------------------------------------------------------------------

/// A safe Rust representation of an event delivered from the host.
#[derive(Debug, Clone)]
pub struct PluginEvent {
    /// What kind of event this is.
    pub event_type: PluginEventType,
    /// Primary payload (e.g. the message text, the nick, the channel name).
    pub data: String,
    /// Secondary payload (e.g. the channel for a join, the new nick).
    pub source: String,
}

// ---------------------------------------------------------------------------
// PluginHost trait — host callbacks available to plugins
// ---------------------------------------------------------------------------

/// The host-side interface that plugins use to interact with the IRC client.
///
/// An implementation of this trait is passed to [`Plugin::init`] so the plugin
/// can register commands, hook events, send messages, and access configuration.
pub trait PluginHost {
    /// Register a new slash-command (e.g. `myplugin` for `/myplugin`).
    fn register_command(&self, name: &str, description: &str) -> Result<(), PluginError>;

    /// Unregister a previously registered command.
    fn unregister_command(&self, name: &str);

    /// Subscribe to an event type.
    fn hook_event(&self, event_type: PluginEventType) -> Result<(), PluginError>;

    /// Unsubscribe from an event type.
    fn unhook_event(&self, event_type: PluginEventType);

    /// Print a message to the user's active window.
    fn echo(&self, text: &str);

    /// Write a log message at the given severity level.
    fn log(&self, level: LogLevel, msg: &str);

    /// Look up a configuration value by key. Returns `None` if not found.
    fn get_config_value(&self, key: &str) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Plugin trait — implemented by plugin authors
// ---------------------------------------------------------------------------

/// The main trait that plugin authors implement to create a pirc plugin.
///
/// All lifecycle methods have default no-op implementations so plugins only
/// need to override the methods they care about. The metadata methods
/// (`name`, `version`, `description`, `author`) must be implemented.
pub trait Plugin: Send {
    /// Returns the human-readable name of this plugin.
    fn name(&self) -> &str;

    /// Returns the semantic version string (e.g. "1.0.0").
    fn version(&self) -> &str;

    /// Returns a short description of what this plugin does.
    #[allow(clippy::unnecessary_literal_bound)]
    fn description(&self) -> &str {
        ""
    }

    /// Returns the author name or identifier.
    #[allow(clippy::unnecessary_literal_bound)]
    fn author(&self) -> &str {
        ""
    }

    /// Returns the capabilities this plugin requires.
    fn capabilities(&self) -> &[PluginCapability] {
        &[]
    }

    /// Called once after loading to initialise the plugin.
    ///
    /// The `host` reference provides access to the IRC client's command
    /// registration, event hooks, logging, and configuration APIs.
    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError>;

    /// Called when the plugin is about to be unloaded. Clean up resources here.
    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// Called when the plugin is enabled (after init or after a disable/enable cycle).
    fn on_enable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// Called when the plugin is disabled but not yet unloaded.
    fn on_disable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// Called when an event the plugin subscribed to fires.
    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> {
        let _ = event;
        Ok(())
    }

    /// Called when the user executes a command registered by this plugin.
    ///
    /// Returns `true` if the command was handled, `false` otherwise.
    fn on_command(&mut self, cmd: &str, args: &[&str]) -> Result<bool, PluginError> {
        let _ = (cmd, args);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- LogLevel tests -------------------------------------------------------

    #[test]
    fn log_level_from_u32_known_values() {
        assert_eq!(LogLevel::from_u32(0), LogLevel::Error);
        assert_eq!(LogLevel::from_u32(1), LogLevel::Warn);
        assert_eq!(LogLevel::from_u32(2), LogLevel::Info);
        assert_eq!(LogLevel::from_u32(3), LogLevel::Debug);
    }

    #[test]
    fn log_level_from_u32_unknown_defaults_to_debug() {
        assert_eq!(LogLevel::from_u32(42), LogLevel::Debug);
        assert_eq!(LogLevel::from_u32(u32::MAX), LogLevel::Debug);
    }

    // -- PluginError tests ----------------------------------------------------

    #[test]
    fn plugin_error_display() {
        let cases = [
            (
                PluginError::InitFailed("boom".into()),
                "init failed: boom",
            ),
            (
                PluginError::ShutdownFailed("oops".into()),
                "shutdown failed: oops",
            ),
            (
                PluginError::CommandFailed("bad cmd".into()),
                "command failed: bad cmd",
            ),
            (
                PluginError::EventFailed("bad event".into()),
                "event failed: bad event",
            ),
            (
                PluginError::PermissionDenied {
                    plugin: "my-plugin".into(),
                    action: "register commands".into(),
                },
                "plugin `my-plugin` denied: register commands",
            ),
            (PluginError::Other("misc".into()), "misc"),
        ];
        for (err, expected) in &cases {
            assert_eq!(err.to_string(), *expected);
        }
    }

    #[test]
    fn plugin_error_is_std_error() {
        let err: Box<dyn std::error::Error> =
            Box::new(PluginError::Other("test".into()));
        assert_eq!(err.to_string(), "test");
    }

    // -- Plugin trait defaults ------------------------------------------------

    /// Minimal plugin that only implements the required methods.
    struct MinimalPlugin;

    #[allow(clippy::unnecessary_literal_bound)]
    impl Plugin for MinimalPlugin {
        fn name(&self) -> &str {
            "minimal"
        }
        fn version(&self) -> &str {
            "0.1.0"
        }
        fn init(&mut self, _host: &dyn PluginHost) -> Result<(), PluginError> {
            Ok(())
        }
    }

    #[test]
    fn plugin_trait_defaults() {
        let mut plugin = MinimalPlugin;
        assert_eq!(plugin.name(), "minimal");
        assert_eq!(plugin.version(), "0.1.0");
        assert_eq!(plugin.description(), "");
        assert_eq!(plugin.author(), "");
        assert!(plugin.capabilities().is_empty());
        assert!(plugin.shutdown().is_ok());
        assert!(plugin.on_enable().is_ok());
        assert!(plugin.on_disable().is_ok());

        let event = PluginEvent {
            event_type: PluginEventType::Connected,
            data: String::new(),
            source: String::new(),
        };
        assert!(plugin.on_event(&event).is_ok());
        assert!(!plugin.on_command("test", &[]).unwrap());
    }

    // -- PluginEvent ----------------------------------------------------------

    #[test]
    fn plugin_event_clone() {
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "hello".into(),
            source: "#channel".into(),
        };
        let cloned = event.clone();
        assert_eq!(cloned.event_type, event.event_type);
        assert_eq!(cloned.data, event.data);
        assert_eq!(cloned.source, event.source);
    }
}
