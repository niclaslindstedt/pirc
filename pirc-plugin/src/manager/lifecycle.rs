//! Plugin lifecycle operations: loading, enabling, disabling, unloading,
//! and directory scanning.

use std::collections::HashSet;
use std::path::Path;

use tracing::{debug, error, info, trace, warn};

use crate::config::{self, PluginConfig};
use crate::ffi::{PluginHostApi, PluginStatus};
use crate::loader::PluginLoader;
use crate::sandbox::CapabilityChecker;

use super::types::{ManagedPlugin, ManagerError, PluginState};
use super::PluginManager;

impl PluginManager {
    /// Loads a plugin from the given dynamic library path.
    ///
    /// The library is loaded, the entry point called, and the plugin's `init`
    /// method invoked with the provided `host_api`. The plugin enters the
    /// `Loaded` state upon success.
    ///
    /// This is a convenience wrapper around [`load_plugin_with_config`](Self::load_plugin_with_config)
    /// that uses a default [`PluginConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the library cannot be loaded, the plugin
    /// name is already taken, or the plugin's `init` callback fails.
    pub fn load_plugin(
        &mut self,
        path: &Path,
        host_api: &PluginHostApi,
    ) -> Result<String, ManagerError> {
        self.load_plugin_with_config(path, host_api, PluginConfig::default())
    }

    /// Loads a plugin from the given dynamic library path with a
    /// pre-loaded configuration.
    ///
    /// If `config.enabled` is `false`, the plugin is loaded and initialised
    /// but remains in the `Loaded` state (not auto-enabled).
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the library cannot be loaded, the plugin
    /// name is already taken, or the plugin's `init` callback fails.
    #[allow(unsafe_code)]
    pub fn load_plugin_with_config(
        &mut self,
        path: &Path,
        host_api: &PluginHostApi,
        plugin_config: PluginConfig,
    ) -> Result<String, ManagerError> {
        let loaded = self.loader.load(path)?;

        // Record the file modification time for hot-reload change detection.
        let last_modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok();
        if last_modified.is_none() {
            trace!(path = %path.display(), "could not read modification time");
        }

        let (name, version, capabilities) = unsafe {
            let info = (loaded.api().info)();
            let n = info.name.as_str().to_owned();
            let v = info.version.as_str().to_owned();
            let caps = if info.capabilities.is_null() || info.capabilities_len == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(info.capabilities, info.capabilities_len)
                    .to_vec()
            };
            (loaded.api().free_info)(info);
            (n, v, caps)
        };

        if self.plugins.contains_key(&name) {
            return Err(ManagerError::DuplicateName(name));
        }

        let result = (loaded.api().init)(std::ptr::addr_of!(*host_api));

        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            return Err(ManagerError::PluginCallFailed {
                name,
                action: "init",
                reason,
            });
        }

        let checker = CapabilityChecker::new(&name, &capabilities);

        info!(
            plugin = %name,
            version = %version,
            enabled = plugin_config.enabled,
            capabilities = capabilities.len(),
            "plugin loaded with config"
        );

        let managed = ManagedPlugin {
            plugin: loaded,
            state: PluginState::Loaded,
            name: name.clone(),
            version,
            commands: HashSet::new(),
            hooked_events: HashSet::new(),
            config: plugin_config,
            capabilities: checker,
            last_modified,
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
    /// For each library, looks for a matching `<name>.toml` configuration
    /// file in the same directory. If a config file sets `enabled = false`,
    /// the plugin is loaded but not auto-enabled. Libraries are sorted by
    /// filename for deterministic load order. Individual load failures are
    /// logged but do not prevent other plugins from loading.
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

            // Derive a config file name from the library file stem.
            let plugin_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // Strip common library prefixes (e.g. "lib" on Unix).
            let config_name = plugin_stem.strip_prefix("lib").unwrap_or(plugin_stem);
            let plugin_config = config::load_plugin_config(dir, config_name);

            match self.load_plugin_with_config(path, host_api, plugin_config) {
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
}
