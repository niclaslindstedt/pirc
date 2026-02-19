//! Top-level coordinator for plugin lifecycle management.
//!
//! [`PluginManager`] is responsible for loading, initialising, enabling,
//! disabling, and unloading plugins. It tracks all managed plugins in a
//! name-keyed map and enforces the plugin state machine:
//!
//! ```text
//! Loaded -> Enabled -> Disabled -> (Unloaded / re-Enabled)
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use tracing::{debug, error, info, warn};

use crate::ffi::{PluginEventType, PluginHostApi, PluginStatus};
use crate::loader::{LoadError, LoadedPlugin, PluginLoader};

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
        }
    }
}

impl std::error::Error for ManagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LoadFailed(err) => Some(err),
            _ => None,
        }
    }
}

impl From<LoadError> for ManagerError {
    fn from(err: LoadError) -> Self {
        Self::LoadFailed(err)
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

/// A plugin that is tracked by the [`PluginManager`].
pub struct ManagedPlugin {
    /// The underlying loaded dynamic library and API vtable.
    plugin: LoadedPlugin,
    /// Current lifecycle state.
    state: PluginState,
    /// Plugin name (cached from the plugin's `info` call).
    name: String,
    /// Plugin version (cached from the plugin's `info` call).
    version: String,
    /// Commands registered by this plugin.
    commands: HashSet<String>,
    /// Event types this plugin has subscribed to.
    hooked_events: HashSet<PluginEventType>,
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
            .finish_non_exhaustive()
    }
}

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
    plugins: HashMap<String, ManagedPlugin>,
    loader: PluginLoader,
}

impl PluginManager {
    /// Creates a new, empty `PluginManager`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            loader: PluginLoader::new(),
        }
    }

    /// Loads a plugin from the given dynamic library path.
    ///
    /// The library is loaded, the entry point called, and the plugin's `init`
    /// method invoked with the provided `host_api`. The plugin enters the
    /// `Loaded` state upon success.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the library cannot be loaded, the plugin
    /// name is already taken, or the plugin's `init` callback fails.
    #[allow(unsafe_code)]
    pub fn load_plugin(
        &mut self,
        path: &Path,
        host_api: &PluginHostApi,
    ) -> Result<String, ManagerError> {
        let loaded = self.loader.load(path)?;

        // Get plugin name and version via the info/free_info cycle.
        // SAFETY: The plugin was loaded via PluginLoader::load which validates
        // the entry point and vtable.
        let (name, version) = unsafe {
            let info = (loaded.api().info)();
            let n = info.name.as_str().to_owned();
            let v = info.version.as_str().to_owned();
            (loaded.api().free_info)(info);
            (n, v)
        };

        if self.plugins.contains_key(&name) {
            return Err(ManagerError::DuplicateName(name));
        }

        // Call init with the host API.
        // SAFETY: host_api is a valid pointer to the host's callback vtable.
        let result = (loaded.api().init)(std::ptr::addr_of!(*host_api));

        if result.status == PluginStatus::Error {
            // SAFETY: On error, the caller owns the error_message FfiString.
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name,
                action: "init",
                reason,
            });
        }

        info!(plugin = %name, version = %version, "plugin loaded and initialised");

        let managed = ManagedPlugin {
            plugin: loaded,
            state: PluginState::Loaded,
            name: name.clone(),
            version,
            commands: HashSet::new(),
            hooked_events: HashSet::new(),
        };

        self.plugins.insert(name.clone(), managed);
        Ok(name)
    }

    /// Enables a loaded or disabled plugin by calling its `on_enable` callback.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the plugin is not found, is already enabled,
    /// or its `on_enable` callback fails.
    #[allow(unsafe_code)]
    pub fn enable_plugin(&mut self, name: &str) -> Result<(), ManagerError> {
        let managed = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| ManagerError::NotFound(name.to_owned()))?;

        if managed.state == PluginState::Enabled {
            return Err(ManagerError::InvalidState {
                name: name.to_owned(),
                current: managed.state,
                action: "enable",
            });
        }

        let result = (managed.plugin.api().on_enable)();

        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name: name.to_owned(),
                action: "enable",
                reason,
            });
        }

        managed.state = PluginState::Enabled;
        info!(plugin = %name, "plugin enabled");
        Ok(())
    }

    /// Disables an enabled plugin by calling its `on_disable` callback.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the plugin is not found, is not enabled,
    /// or its `on_disable` callback fails.
    #[allow(unsafe_code)]
    pub fn disable_plugin(&mut self, name: &str) -> Result<(), ManagerError> {
        let managed = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| ManagerError::NotFound(name.to_owned()))?;

        if managed.state != PluginState::Enabled {
            return Err(ManagerError::InvalidState {
                name: name.to_owned(),
                current: managed.state,
                action: "disable",
            });
        }

        let result = (managed.plugin.api().on_disable)();

        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name: name.to_owned(),
                action: "disable",
                reason,
            });
        }

        managed.state = PluginState::Disabled;
        info!(plugin = %name, "plugin disabled");
        Ok(())
    }

    /// Unloads a plugin: calls `shutdown`, removes it from the manager, and
    /// drops the library handle.
    ///
    /// The plugin must be in the `Loaded` or `Disabled` state. If the plugin
    /// is currently `Enabled`, disable it first.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the plugin is not found, is in the wrong
    /// state, or the `shutdown` callback fails.
    #[allow(unsafe_code)]
    pub fn unload_plugin(&mut self, name: &str) -> Result<(), ManagerError> {
        let managed = self
            .plugins
            .get(name)
            .ok_or_else(|| ManagerError::NotFound(name.to_owned()))?;

        if managed.state == PluginState::Enabled {
            return Err(ManagerError::InvalidState {
                name: name.to_owned(),
                current: managed.state,
                action: "unload",
            });
        }

        // Call shutdown before removing.
        let result = (managed.plugin.api().shutdown)();

        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name: name.to_owned(),
                action: "shutdown",
                reason,
            });
        }

        // Remove from map (dropping LoadedPlugin unloads the library).
        self.plugins.remove(name);
        info!(plugin = %name, "plugin unloaded");
        Ok(())
    }

    /// Scans a directory for plugin dynamic libraries and loads each one.
    ///
    /// Libraries are sorted by filename for deterministic load order.
    /// Individual load failures are logged but do not prevent other plugins
    /// from loading.
    ///
    /// Returns the names of successfully loaded plugins.
    pub fn load_plugins_dir(
        &mut self,
        dir: &Path,
        host_api: &PluginHostApi,
    ) -> Vec<String> {
        let ext = PluginLoader::library_extension();

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!(path = %dir.display(), error = %e, "failed to read plugins directory");
                return Vec::new();
            }
        };

        let mut paths: Vec<_> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        // Sort by filename for deterministic ordering.
        paths.sort();

        let mut loaded_names = Vec::new();

        for path in &paths {
            debug!(path = %path.display(), "attempting to load plugin");
            match self.load_plugin(path, host_api) {
                Ok(name) => loaded_names.push(name),
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to load plugin, skipping"
                    );
                }
            }
        }

        info!(
            count = loaded_names.len(),
            total = paths.len(),
            "plugin directory scan complete"
        );

        loaded_names
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
    /// through the host API.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
    pub fn register_command(
        &mut self,
        plugin_name: &str,
        command: &str,
    ) -> Result<(), ManagerError> {
        let managed = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
        managed.commands.insert(command.to_owned());
        debug!(plugin = %plugin_name, command = %command, "command registered");
        Ok(())
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
        let managed = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
        managed.hooked_events.insert(event_type);
        debug!(plugin = %plugin_name, event = ?event_type, "event hooked");
        Ok(())
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
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    // -- register_command / hook_event on nonexistent plugin -----------------

    #[test]
    fn register_command_nonexistent_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.register_command("ghost", "test").unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    #[test]
    fn hook_event_nonexistent_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager
            .hook_event("ghost", PluginEventType::Connected)
            .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    // -- load_plugin with invalid path ---------------------------------------

    #[test]
    fn load_plugin_invalid_path_returns_error() {
        use crate::ffi::{FfiString, PluginResult};

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

        let host_api = PluginHostApi {
            register_command: noop_register,
            unregister_command: noop_unregister,
            hook_event: noop_hook,
            unhook_event: noop_unhook,
            echo: noop_echo,
            log: noop_log,
            get_config_value: noop_get_config,
        };

        let mut manager = PluginManager::new();
        let result = manager.load_plugin(Path::new("/nonexistent/plugin.dylib"), &host_api);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ManagerError::LoadFailed(_)));
    }

    // -- load_plugins_dir with empty/nonexistent directory --------------------

    #[test]
    fn load_plugins_dir_nonexistent_returns_empty() {
        use crate::ffi::{FfiString, PluginResult};

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

        let host_api = PluginHostApi {
            register_command: noop_register,
            unregister_command: noop_unregister,
            hook_event: noop_hook,
            unhook_event: noop_unhook,
            echo: noop_echo,
            log: noop_log,
            get_config_value: noop_get_config,
        };

        let mut manager = PluginManager::new();
        let loaded = manager.load_plugins_dir(Path::new("/nonexistent/dir"), &host_api);
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_plugins_dir_empty_directory() {
        use crate::ffi::{FfiString, PluginResult};

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

        let host_api = PluginHostApi {
            register_command: noop_register,
            unregister_command: noop_unregister,
            hook_event: noop_hook,
            unhook_event: noop_unhook,
            echo: noop_echo,
            log: noop_log,
            get_config_value: noop_get_config,
        };

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
}
