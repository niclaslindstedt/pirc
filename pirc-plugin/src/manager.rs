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
use crate::registry::{CommandError, CommandRegistry, EventRegistry};

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
    commands: CommandRegistry,
    events: EventRegistry,
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

        // Clean up registries before removing the plugin.
        let cmd_count = self.commands.unregister_all(name);
        let evt_count = self.events.unsubscribe_all(name);
        if cmd_count > 0 {
            debug!(plugin = %name, commands = cmd_count, "commands unregistered on unload");
        }
        if evt_count > 0 {
            debug!(plugin = %name, events = evt_count, "events unhooked on unload");
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
        if !self.plugins.contains_key(plugin_name) {
            return Err(ManagerError::NotFound(plugin_name.to_owned()));
        }
        self.commands.register(plugin_name, command, description)?;
        let managed = self.plugins.get_mut(plugin_name).expect("checked above");
        managed.commands.insert(command.to_lowercase());
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
        self.events.subscribe(plugin_name, event_type);
        debug!(plugin = %plugin_name, event = ?event_type, "event hooked");
        Ok(())
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
        let managed = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
        managed.commands.remove(&command.to_lowercase());
        let removed = self.commands.unregister(plugin_name, command);
        if removed {
            debug!(plugin = %plugin_name, command = %command, "command unregistered");
        }
        Ok(removed)
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
        let managed = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
        managed.hooked_events.remove(&event_type);
        let removed = self.events.unsubscribe(plugin_name, event_type);
        if removed {
            debug!(plugin = %plugin_name, event = ?event_type, "event unhooked");
        }
        Ok(removed)
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
    #[allow(unsafe_code)]
    pub fn dispatch_command(
        &self,
        command: &str,
        args: &str,
    ) -> Result<bool, ManagerError> {
        let Some(entry) = self.commands.lookup(command) else {
            return Ok(false);
        };

        let Some(managed) = self.plugins.get(&entry.plugin_name) else {
            return Ok(false);
        };

        if managed.state != PluginState::Enabled {
            debug!(
                plugin = %entry.plugin_name,
                command = %command,
                state = %managed.state,
                "command dispatch skipped: plugin not enabled"
            );
            return Ok(false);
        }

        let ffi_event = crate::ffi::PluginEvent {
            event_type: PluginEventType::CommandExecuted,
            data: crate::ffi::FfiString::new(command),
            source: crate::ffi::FfiString::new(args),
        };

        let result =
            (managed.plugin.api().on_event)(std::ptr::addr_of!(ffi_event));

        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name: entry.plugin_name.clone(),
                action: "dispatch_command",
                reason,
            });
        }

        debug!(
            plugin = %entry.plugin_name,
            command = %command,
            "command dispatched"
        );
        Ok(true)
    }

    /// Dispatches an event to all subscribed plugins.
    ///
    /// Fans out the event to every plugin subscribed to this event type
    /// that is currently in the `Enabled` state. Individual plugin failures
    /// are logged but do not prevent delivery to other plugins.
    ///
    /// Returns the number of plugins that successfully received the event.
    #[allow(unsafe_code)]
    pub fn dispatch_event(
        &self,
        event_type: PluginEventType,
        data: &str,
        source: &str,
    ) -> usize {
        let subscribers = self.events.subscribers(event_type);
        if subscribers.is_empty() {
            return 0;
        }

        let ffi_event = crate::ffi::PluginEvent {
            event_type,
            data: crate::ffi::FfiString::new(data),
            source: crate::ffi::FfiString::new(source),
        };

        let mut delivered = 0;

        for plugin_name in &subscribers {
            let Some(managed) = self.plugins.get(plugin_name) else {
                continue;
            };

            if managed.state != PluginState::Enabled {
                debug!(
                    plugin = %plugin_name,
                    event = ?event_type,
                    "event dispatch skipped: plugin not enabled"
                );
                continue;
            }

            let result = (managed.plugin.api().on_event)(
                std::ptr::addr_of!(ffi_event),
            );

            if result.status == PluginStatus::Error {
                let reason = unsafe { result.error_message.into_string() };
                warn!(
                    plugin = %plugin_name,
                    event = ?event_type,
                    error = %reason,
                    "plugin event handler failed"
                );
            } else {
                delivered += 1;
            }
        }

        debug!(
            event = ?event_type,
            subscribers = subscribers.len(),
            delivered,
            "event dispatched"
        );
        delivered
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
    use crate::ffi::{FfiString, PluginResult};

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

    // -- register_command / hook_event on nonexistent plugin -----------------

    #[test]
    fn register_command_nonexistent_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.register_command("ghost", "test", "desc").unwrap_err();
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

    // -- ManagerError CommandConflict -----------------------------------------

    #[test]
    fn manager_error_display_command_conflict() {
        let err = ManagerError::CommandConflict(CommandError::AlreadyRegistered {
            command: "hello".into(),
            owner: "plugin-a".into(),
        });
        let msg = err.to_string();
        assert!(msg.contains("hello"));
        assert!(msg.contains("plugin-a"));
    }

    #[test]
    fn manager_error_source_command_conflict() {
        use std::error::Error;
        let err = ManagerError::CommandConflict(CommandError::AlreadyRegistered {
            command: "test".into(),
            owner: "owner".into(),
        });
        assert!(err.source().is_some());
    }

    #[test]
    fn manager_error_from_command_error() {
        let cmd_err = CommandError::AlreadyRegistered {
            command: "test".into(),
            owner: "owner".into(),
        };
        let manager_err: ManagerError = cmd_err.into();
        assert!(matches!(manager_err, ManagerError::CommandConflict(_)));
    }

    // -- Registry integration tests ------------------------------------------

    #[test]
    fn command_registry_exposed() {
        let manager = PluginManager::new();
        assert!(manager.command_registry().is_empty());
    }

    #[test]
    fn event_registry_exposed() {
        let manager = PluginManager::new();
        assert!(!manager.event_registry().has_subscribers(PluginEventType::Connected));
    }

    // -- dispatch_command with no matching command ----------------------------

    #[test]
    fn dispatch_command_no_match_returns_false() {
        let manager = PluginManager::new();
        let result = manager.dispatch_command("nonexistent", "").unwrap();
        assert!(!result);
    }

    // -- dispatch_event with no subscribers ----------------------------------

    #[test]
    fn dispatch_event_no_subscribers_returns_zero() {
        let manager = PluginManager::new();
        let count = manager.dispatch_event(PluginEventType::Connected, "", "");
        assert_eq!(count, 0);
    }

    // -- unregister_command on nonexistent plugin ----------------------------

    #[test]
    fn unregister_command_nonexistent_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager.unregister_command("ghost", "test").unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    // -- unhook_event on nonexistent plugin ----------------------------------

    #[test]
    fn unhook_event_nonexistent_returns_not_found() {
        let mut manager = PluginManager::new();
        let err = manager
            .unhook_event("ghost", PluginEventType::Connected)
            .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }
}
