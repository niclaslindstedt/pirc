//! Top-level coordinator for plugin lifecycle management.
//!
//! [`PluginManager`] is responsible for loading, initialising, enabling,
//! disabling, and unloading plugins. It tracks all managed plugins in a
//! name-keyed map and enforces the plugin state machine:
//!
//! ```text
//! Loaded -> Enabled -> Disabled -> (Unloaded / re-Enabled)
//! ```

mod lifecycle;
mod types;

pub use types::{ManagedPlugin, ManagerError, PluginInfo, PluginState};

use std::collections::HashMap;
use std::fmt;

use crate::config::PluginConfig;
use crate::ffi::PluginEventType;
use crate::loader::PluginLoader;
use crate::registry::{CommandRegistry, EventRegistry};

// ---------------------------------------------------------------------------
// PluginManager
// ---------------------------------------------------------------------------

/// Top-level coordinator for plugin lifecycle management.
///
/// Tracks all loaded plugins by name and provides methods to load, enable,
/// disable, and unload them. Plugin directory scanning loads libraries in
/// deterministic sorted order, and individual failures do not prevent other
/// plugins from loading.
pub struct PluginManager {
    pub(crate) plugins: HashMap<String, ManagedPlugin>,
    pub(crate) loader: PluginLoader,
    pub(crate) commands: CommandRegistry,
    pub(crate) events: EventRegistry,
}

impl PluginManager {
    /// Creates a new, empty `PluginManager`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            loader: PluginLoader::new(),
            commands: CommandRegistry::new(),
            events: EventRegistry::new(),
        }
    }

    /// Returns a list of info summaries for all managed plugins.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        let mut infos: Vec<_> = self.plugins.values().map(ManagedPlugin::info).collect();
        // Sort by name for deterministic ordering.
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }

    /// Returns a reference to a managed plugin by name.
    #[must_use]
    pub fn get_plugin(&self, name: &str) -> Option<&ManagedPlugin> {
        self.plugins.get(name)
    }

    /// Returns a mutable reference to a managed plugin by name.
    #[must_use]
    pub fn get_plugin_mut(&mut self, name: &str) -> Option<&mut ManagedPlugin> {
        self.plugins.get_mut(name)
    }

    /// Returns the number of currently managed plugins.
    #[must_use]
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Returns `true` if a plugin with the given name is loaded.
    #[must_use]
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Registers a command for the given plugin.
    ///
    /// This is called by the host when a plugin calls `register_command`
    /// through the host API. Command names are case-insensitive; the first
    /// plugin to register a name wins.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError::NotFound`] if the plugin doesn't exist, or
    /// [`ManagerError::CommandConflict`] if another plugin already owns
    /// the command.
    pub fn register_command(
        &mut self,
        plugin_name: &str,
        command: &str,
        description: &str,
    ) -> Result<(), ManagerError> {
        crate::dispatch::register_command(
            &mut self.commands,
            &mut self.plugins,
            plugin_name,
            command,
            description,
        )
    }

    /// Hooks an event type for the given plugin.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
    pub fn hook_event(
        &mut self,
        plugin_name: &str,
        event_type: PluginEventType,
    ) -> Result<(), ManagerError> {
        crate::dispatch::hook_event(
            &mut self.events,
            &mut self.plugins,
            plugin_name,
            event_type,
        )
    }

    /// Unregisters a command from the given plugin.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
    pub fn unregister_command(
        &mut self,
        plugin_name: &str,
        command: &str,
    ) -> Result<bool, ManagerError> {
        crate::dispatch::unregister_command(
            &mut self.commands,
            &mut self.plugins,
            plugin_name,
            command,
        )
    }

    /// Unhooks an event type for the given plugin.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
    pub fn unhook_event(
        &mut self,
        plugin_name: &str,
        event_type: PluginEventType,
    ) -> Result<bool, ManagerError> {
        crate::dispatch::unhook_event(
            &mut self.events,
            &mut self.plugins,
            plugin_name,
            event_type,
        )
    }

    /// Dispatches a command to the plugin that owns it.
    ///
    /// Looks up the command in the [`CommandRegistry`], verifies the owning
    /// plugin is enabled, and calls the plugin's `on_event` with a
    /// `CommandExecuted` event containing the command name and arguments.
    ///
    /// Returns `true` if the command was dispatched (a plugin handled it),
    /// `false` if no plugin owns the command.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the owning plugin exists but its callback
    /// fails.
    pub fn dispatch_command(
        &self,
        command: &str,
        args: &str,
    ) -> Result<bool, ManagerError> {
        crate::dispatch::dispatch_command(
            &self.commands,
            &self.plugins,
            command,
            args,
        )
    }

    /// Dispatches an event to all subscribed plugins.
    ///
    /// Fans out the event to every plugin subscribed to this event type
    /// that is currently in the `Enabled` state. Individual plugin failures
    /// are logged but do not prevent delivery to other plugins.
    ///
    /// Returns the number of plugins that successfully received the event.
    pub fn dispatch_event(
        &self,
        event_type: PluginEventType,
        data: &str,
        source: &str,
    ) -> usize {
        crate::dispatch::dispatch_event(
            &self.events,
            &self.plugins,
            event_type,
            data,
            source,
        )
    }

    /// Looks up a config setting for a specific plugin.
    ///
    /// Returns `None` if the plugin doesn't exist or the key is not set.
    #[must_use]
    pub fn get_plugin_config_value(&self, plugin_name: &str, key: &str) -> Option<String> {
        self.plugins
            .get(plugin_name)
            .and_then(|p| p.get_config_value(key))
    }

    /// Returns the [`PluginConfig`] for a given plugin, or `None` if not found.
    #[must_use]
    pub fn get_plugin_config(&self, name: &str) -> Option<&PluginConfig> {
        self.plugins.get(name).map(ManagedPlugin::config)
    }

    /// Returns a reference to the command registry.
    #[must_use]
    pub fn command_registry(&self) -> &CommandRegistry {
        &self.commands
    }

    /// Returns a reference to the event registry.
    #[must_use]
    pub fn event_registry(&self) -> &EventRegistry {
        &self.events
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugin_count", &self.plugins.len())
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .field("commands", &self.commands)
            .field("events", &self.events)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{FfiString, PluginHostApi, PluginResult};
    use crate::loader::LoadError;

    use std::path::Path;

    /// Creates a noop [`PluginHostApi`] for tests that need one but don't
    /// exercise any host callbacks.
    fn noop_host_api() -> PluginHostApi {
        extern "C" fn noop_register(
            _name: FfiString,
            _cb: extern "C" fn(FfiString) -> PluginResult,
        ) -> PluginResult {
            PluginResult::ok()
        }
        extern "C" fn noop_unregister(_name: FfiString) -> PluginResult {
            PluginResult::ok()
        }
        extern "C" fn noop_hook(_et: PluginEventType) -> PluginResult {
            PluginResult::ok()
        }
        extern "C" fn noop_unhook(_et: PluginEventType) -> PluginResult {
            PluginResult::ok()
        }
        extern "C" fn noop_echo(_msg: FfiString) {}
        extern "C" fn noop_log(_level: u32, _msg: FfiString) {}
        extern "C" fn noop_get_config(_key: FfiString) -> FfiString {
            FfiString::empty()
        }

        PluginHostApi {
            register_command: noop_register,
            unregister_command: noop_unregister,
            hook_event: noop_hook,
            unhook_event: noop_unhook,
            echo: noop_echo,
            log: noop_log,
            get_config_value: noop_get_config,
        }
    }

    // -- PluginState tests ---------------------------------------------------

    #[test]
    fn plugin_state_display() {
        assert_eq!(PluginState::Loaded.to_string(), "loaded");
        assert_eq!(PluginState::Enabled.to_string(), "enabled");
        assert_eq!(PluginState::Disabled.to_string(), "disabled");
    }

    #[test]
    fn plugin_state_equality() {
        assert_eq!(PluginState::Loaded, PluginState::Loaded);
        assert_ne!(PluginState::Loaded, PluginState::Enabled);
        assert_ne!(PluginState::Enabled, PluginState::Disabled);
    }

    // -- ManagerError tests --------------------------------------------------

    #[test]
    fn manager_error_display_duplicate_name() {
        let err = ManagerError::DuplicateName("test-plugin".into());
        assert_eq!(err.to_string(), "plugin `test-plugin` is already loaded");
    }

    #[test]
    fn manager_error_display_not_found() {
        let err = ManagerError::NotFound("missing".into());
        assert_eq!(err.to_string(), "plugin `missing` not found");
    }

    #[test]
    fn manager_error_display_invalid_state() {
        let err = ManagerError::InvalidState {
            name: "myplugin".into(),
            current: PluginState::Enabled,
            action: "unload",
        };
        assert_eq!(
            err.to_string(),
            "cannot unload plugin `myplugin` in state enabled"
        );
    }

    #[test]
    fn manager_error_display_plugin_call_failed() {
        let err = ManagerError::PluginCallFailed {
            name: "broken".into(),
            action: "init",
            reason: "segfault".into(),
        };
        assert_eq!(
            err.to_string(),
            "plugin `broken` init failed: segfault"
        );
    }

    #[test]
    fn manager_error_source_chain() {
        use std::error::Error;

        let load_err = ManagerError::LoadFailed(LoadError::InvalidPlugin {
            path: "test.dylib".into(),
            reason: "bad".into(),
        });
        assert!(load_err.source().is_some());

        let not_found = ManagerError::NotFound("x".into());
        assert!(not_found.source().is_none());
    }

    #[test]
    fn manager_error_from_load_error() {
        let load_err = LoadError::InvalidPlugin {
            path: "test.dylib".into(),
            reason: "null".into(),
        };
        let manager_err: ManagerError = load_err.into();
        assert!(matches!(manager_err, ManagerError::LoadFailed(_)));
    }

    // -- PluginManager construction ------------------------------------------

    #[test]
    fn new_manager_is_empty() {
        let manager = PluginManager::new();
        assert_eq!(manager.plugin_count(), 0);
        assert!(manager.list_plugins().is_empty());
    }

    #[test]
    fn default_manager_is_empty() {
        let manager = PluginManager::default();
        assert_eq!(manager.plugin_count(), 0);
    }

    #[test]
    fn manager_debug_format() {
        let manager = PluginManager::new();
        let debug = format!("{manager:?}");
        assert!(debug.contains("PluginManager"));
        assert!(debug.contains("plugin_count"));
    }

    // -- get_plugin / has_plugin on empty manager ----------------------------

    #[test]
    fn get_plugin_not_found() {
        let manager = PluginManager::new();
        assert!(manager.get_plugin("nonexistent").is_none());
    }

    #[test]
    fn has_plugin_empty_manager() {
        let manager = PluginManager::new();
        assert!(!manager.has_plugin("anything"));
    }

    // -- enable/disable/unload error paths without FFI -----------------------

    #[test]
    fn enable_nonexistent_plugin_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.enable_plugin("ghost").unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(ref n) if n == "ghost"));
    }

    #[test]
    fn disable_nonexistent_plugin_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.disable_plugin("ghost").unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(ref n) if n == "ghost"));
    }

    #[test]
    fn unload_nonexistent_plugin_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.unload_plugin("ghost").unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(ref n) if n == "ghost"));
    }

    // -- load_plugin with invalid path ---------------------------------------

    #[test]
    fn load_plugin_invalid_path_returns_error() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let result = manager.load_plugin(Path::new("/nonexistent/plugin.dylib"), &host_api);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ManagerError::LoadFailed(_)));
    }

    // -- load_plugins_dir with empty/nonexistent directory --------------------

    #[test]
    fn load_plugins_dir_nonexistent_returns_empty() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let loaded = manager.load_plugins_dir(Path::new("/nonexistent/dir"), &host_api);
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_plugins_dir_empty_directory() {
        let host_api = noop_host_api();

        let dir = std::env::temp_dir().join("pirc_test_empty_plugins_dir");
        let _ = std::fs::create_dir(&dir);

        let mut manager = PluginManager::new();
        let loaded = manager.load_plugins_dir(&dir, &host_api);
        assert!(loaded.is_empty());
        assert_eq!(manager.plugin_count(), 0);

        let _ = std::fs::remove_dir(&dir);
    }

    // -- ManagedPlugin info summary ------------------------------------------

    #[test]
    fn plugin_info_summary() {
        let info = PluginInfo {
            name: "test".into(),
            version: "1.0.0".into(),
            state: PluginState::Enabled,
            commands: vec!["hello".into(), "world".into()],
            hooked_events: vec![PluginEventType::Connected],
        };
        assert_eq!(info.name, "test");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.state, PluginState::Enabled);
        assert_eq!(info.commands.len(), 2);
        assert_eq!(info.hooked_events.len(), 1);
    }

    #[test]
    fn plugin_info_debug_format() {
        let info = PluginInfo {
            name: "dbg".into(),
            version: "0.1.0".into(),
            state: PluginState::Loaded,
            commands: vec![],
            hooked_events: vec![],
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("dbg"));
    }

    // -- PluginState copy trait -----------------------------------------------

    #[test]
    fn plugin_state_is_copy() {
        let s = PluginState::Loaded;
        let s2 = s;
        assert_eq!(s, s2); // both valid — Copy
    }

    // -- ManagerError Display for LoadFailed ----------------------------------

    #[test]
    fn manager_error_display_load_failed() {
        let err = ManagerError::LoadFailed(LoadError::InvalidPlugin {
            path: "test.dylib".into(),
            reason: "bad format".into(),
        });
        let msg = err.to_string();
        assert!(msg.contains("test.dylib"));
        assert!(msg.contains("bad format"));
    }

    // -- Config on PluginManager ----------------------------------------------

    #[test]
    fn get_plugin_config_value_nonexistent_plugin() {
        let manager = PluginManager::new();
        assert!(manager.get_plugin_config_value("nope", "key").is_none());
    }

    #[test]
    fn get_plugin_config_nonexistent_plugin() {
        let manager = PluginManager::new();
        assert!(manager.get_plugin_config("nope").is_none());
    }

    #[test]
    fn load_plugin_with_config_invalid_path() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let config = PluginConfig::default();
        let result = manager.load_plugin_with_config(
            Path::new("/nonexistent/plugin.dylib"),
            &host_api,
            config,
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ManagerError::LoadFailed(_)));
    }
}
