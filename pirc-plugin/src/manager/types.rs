//! Core types used throughout the plugin manager.
//!
//! Contains [`PluginState`], [`ManagerError`], [`PluginInfo`], and
//! [`ManagedPlugin`].

use std::collections::HashSet;
use std::fmt;
use std::time::SystemTime;

use crate::config::PluginConfig;
use crate::ffi::PluginEventType;
use crate::loader::{LoadError, LoadedPlugin};
use crate::registry::CommandError;
use crate::sandbox::CapabilityChecker;

// ---------------------------------------------------------------------------
// PluginState
// ---------------------------------------------------------------------------

/// The lifecycle state of a managed plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// Plugin library has been loaded and `init` called, but `on_enable`
    /// has not yet been called.
    Loaded,
    /// Plugin is active and receiving events/commands.
    Enabled,
    /// Plugin has been disabled via `on_disable` but is still loaded in
    /// memory and can be re-enabled.
    Disabled,
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loaded => write!(f, "loaded"),
            Self::Enabled => write!(f, "enabled"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

// ---------------------------------------------------------------------------
// ManagerError
// ---------------------------------------------------------------------------

/// Errors that can occur during plugin management operations.
#[derive(Debug)]
pub enum ManagerError {
    /// A plugin with the given name is already loaded.
    DuplicateName(String),
    /// No plugin with the given name exists.
    NotFound(String),
    /// The requested operation is invalid for the plugin's current state.
    InvalidState {
        name: String,
        current: PluginState,
        action: &'static str,
    },
    /// Failed to load the plugin dynamic library.
    LoadFailed(LoadError),
    /// A plugin lifecycle callback (init, enable, disable, shutdown) failed.
    PluginCallFailed {
        name: String,
        action: &'static str,
        reason: String,
    },
    /// A command registration conflict (another plugin already owns the command).
    CommandConflict(CommandError),
    /// The plugin lacks the required capability for the attempted action.
    PermissionDenied {
        name: String,
        action: String,
    },
}

impl fmt::Display for ManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName(name) => {
                write!(f, "plugin `{name}` is already loaded")
            }
            Self::NotFound(name) => {
                write!(f, "plugin `{name}` not found")
            }
            Self::InvalidState {
                name,
                current,
                action,
            } => {
                write!(
                    f,
                    "cannot {action} plugin `{name}` in state {current}"
                )
            }
            Self::LoadFailed(err) => write!(f, "{err}"),
            Self::PluginCallFailed {
                name,
                action,
                reason,
            } => {
                write!(f, "plugin `{name}` {action} failed: {reason}")
            }
            Self::CommandConflict(err) => write!(f, "{err}"),
            Self::PermissionDenied { name, action } => {
                write!(f, "plugin `{name}` denied: {action}")
            }
        }
    }
}

impl std::error::Error for ManagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LoadFailed(err) => Some(err),
            Self::CommandConflict(err) => Some(err),
            _ => None,
        }
    }
}

impl From<LoadError> for ManagerError {
    fn from(err: LoadError) -> Self {
        Self::LoadFailed(err)
    }
}

impl From<CommandError> for ManagerError {
    fn from(err: CommandError) -> Self {
        Self::CommandConflict(err)
    }
}

// ---------------------------------------------------------------------------
// PluginInfo (summary)
// ---------------------------------------------------------------------------

/// A read-only summary of a managed plugin's current state.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Current lifecycle state.
    pub state: PluginState,
    /// Commands registered by this plugin.
    pub commands: Vec<String>,
    /// Event types this plugin is subscribed to.
    pub hooked_events: Vec<PluginEventType>,
}

// ---------------------------------------------------------------------------
// ManagedPlugin
// ---------------------------------------------------------------------------

/// A plugin that is tracked by the [`PluginManager`](super::PluginManager).
pub struct ManagedPlugin {
    /// The underlying loaded dynamic library and API vtable.
    pub(super) plugin: LoadedPlugin,
    /// Current lifecycle state.
    pub(super) state: PluginState,
    /// Plugin name (cached from the plugin's `info` call).
    pub(super) name: String,
    /// Plugin version (cached from the plugin's `info` call).
    pub(super) version: String,
    /// Commands registered by this plugin.
    pub(super) commands: HashSet<String>,
    /// Event types this plugin has subscribed to.
    pub(super) hooked_events: HashSet<PluginEventType>,
    /// Per-plugin configuration loaded from TOML.
    pub(super) config: PluginConfig,
    /// Capability checker based on the plugin's declared capabilities.
    pub(super) capabilities: CapabilityChecker,
    /// File modification time at the point the library was loaded.
    /// Used by the hot-reload system to detect changes.
    pub(super) last_modified: Option<SystemTime>,
}

impl ManagedPlugin {
    /// Returns the plugin's current lifecycle state.
    #[must_use]
    pub fn state(&self) -> PluginState {
        self.state
    }

    /// Returns the plugin name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the plugin version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the set of registered command names.
    #[must_use]
    pub fn commands(&self) -> &HashSet<String> {
        &self.commands
    }

    /// Returns the set of hooked event types.
    #[must_use]
    pub fn hooked_events(&self) -> &HashSet<PluginEventType> {
        &self.hooked_events
    }

    /// Returns a reference to the underlying [`LoadedPlugin`].
    #[must_use]
    pub fn loaded_plugin(&self) -> &LoadedPlugin {
        &self.plugin
    }

    /// Returns a reference to the plugin's configuration.
    #[must_use]
    pub fn config(&self) -> &PluginConfig {
        &self.config
    }

    /// Returns a reference to the plugin's capability checker.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilityChecker {
        &self.capabilities
    }

    /// Returns the file modification time recorded when the library was loaded.
    #[must_use]
    pub fn last_modified(&self) -> Option<SystemTime> {
        self.last_modified
    }

    /// Looks up a plugin-specific config setting by key.
    #[must_use]
    pub fn get_config_value(&self, key: &str) -> Option<String> {
        self.config.get_setting(key)
    }

    /// Adds a command name to this plugin's tracked command set.
    pub(crate) fn add_command(&mut self, command: String) {
        self.commands.insert(command);
    }

    /// Removes a command name from this plugin's tracked command set.
    pub(crate) fn remove_command(&mut self, command: &str) {
        self.commands.remove(command);
    }

    /// Adds an event type to this plugin's tracked event set.
    pub(crate) fn add_hooked_event(&mut self, event_type: PluginEventType) {
        self.hooked_events.insert(event_type);
    }

    /// Removes an event type from this plugin's tracked event set.
    pub(crate) fn remove_hooked_event(&mut self, event_type: PluginEventType) {
        self.hooked_events.remove(&event_type);
    }

    /// Produces a read-only summary snapshot.
    #[must_use]
    pub fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.name.clone(),
            version: self.version.clone(),
            state: self.state,
            commands: self.commands.iter().cloned().collect(),
            hooked_events: self.hooked_events.iter().copied().collect(),
        }
    }
}

impl fmt::Debug for ManagedPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagedPlugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("state", &self.state)
            .field("commands", &self.commands)
            .field("hooked_events", &self.hooked_events)
            .field("config", &self.config)
            .field("capabilities", &self.capabilities)
            .field("last_modified", &self.last_modified)
            .finish_non_exhaustive()
    }
}

