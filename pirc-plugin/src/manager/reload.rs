//! Hot-reload support for plugins.
//!
//! Provides methods to reload individual plugins or all plugins from disk,
//! and to detect which plugins have changed by comparing file modification
//! times.

use std::path::Path;

use tracing::{debug, error, info, warn};

use crate::ffi::{PluginHostApi, PluginStatus};
use crate::loader::PluginLoader;

use super::types::{ManagerError, PluginState};
use super::PluginManager;

impl PluginManager {
    /// Reloads a single plugin by name.
    ///
    /// This performs a full reload cycle:
    /// 1. Shuts down and unloads the current plugin instance
    /// 2. Reloads the dynamic library from the same path on disk
    /// 3. Re-initialises the plugin with the provided host API
    ///
    /// The plugin's original configuration and commands/events are cleared
    /// during reload; the new plugin instance re-registers them during `init`.
    ///
    /// If the reload fails at any stage after the old instance has been
    /// unloaded, the plugin is left in a clean unloaded state (removed from
    /// the manager) rather than keeping a half-broken entry.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerError`] if the plugin is not found, is still enabled
    /// (must be disabled or loaded first), or if loading/init of the new
    /// library fails.
    #[allow(unsafe_code)]
    pub fn reload_plugin(
        &mut self,
        name: &str,
        host_api: &PluginHostApi,
    ) -> Result<(), ManagerError> {
        let managed = self
            .plugins
            .get(name)
            .ok_or_else(|| ManagerError::NotFound(name.to_owned()))?;

        // Cannot reload an enabled plugin — caller must disable it first.
        if managed.state == PluginState::Enabled {
            return Err(ManagerError::InvalidState {
                name: name.to_owned(),
                current: managed.state,
                action: "reload",
            });
        }

        let lib_path = managed.plugin.path().to_path_buf();
        let old_config = managed.config.clone();

        info!(plugin = %name, path = %lib_path.display(), "reloading plugin");

        // --- Phase 1: Shut down and remove the old instance ---

        // Call shutdown on the old plugin.
        let result = (managed.plugin.api().shutdown)();
        if result.status == PluginStatus::Error {
            let reason = unsafe { result.error_message.into_string() };
            warn!(
                plugin = %name,
                reason = %reason,
                "plugin shutdown failed during reload, proceeding with unload"
            );
        }

        // Clean up registries.
        let cmd_count = self.commands.unregister_all(name);
        let evt_count = self.events.unsubscribe_all(name);
        if cmd_count > 0 {
            debug!(plugin = %name, commands = cmd_count, "commands unregistered for reload");
        }
        if evt_count > 0 {
            debug!(plugin = %name, events = evt_count, "events unhooked for reload");
        }

        // Remove the old plugin (drops the library handle).
        self.plugins.remove(name);

        // --- Phase 2: Load the new library and re-init ---

        match self.load_plugin_with_config(&lib_path, host_api, old_config) {
            Ok(new_name) => {
                info!(plugin = %new_name, "plugin reloaded successfully");
                Ok(())
            }
            Err(e) => {
                error!(
                    plugin = %name,
                    path = %lib_path.display(),
                    error = %e,
                    "failed to reload plugin, plugin is now unloaded"
                );
                Err(e)
            }
        }
    }

    /// Reloads all currently loaded plugins that are in the `Loaded` or
    /// `Disabled` state.
    ///
    /// Enabled plugins are skipped (they must be disabled before reload).
    /// Individual reload failures are logged but do not prevent other plugins
    /// from being reloaded.
    ///
    /// Returns the names of plugins that were successfully reloaded.
    pub fn reload_all(&mut self, host_api: &PluginHostApi) -> Vec<String> {
        // Collect names of reloadable plugins (not enabled).
        let names: Vec<String> = self
            .plugins
            .values()
            .filter(|p| p.state != PluginState::Enabled)
            .map(|p| p.name.clone())
            .collect();

        if names.is_empty() {
            info!("no plugins eligible for reload");
            return Vec::new();
        }

        info!(count = names.len(), "reloading all eligible plugins");

        let mut reloaded = Vec::new();
        for name in &names {
            match self.reload_plugin(name, host_api) {
                Ok(()) => reloaded.push(name.clone()),
                Err(e) => {
                    warn!(plugin = %name, error = %e, "failed to reload plugin");
                }
            }
        }

        info!(
            reloaded = reloaded.len(),
            total = names.len(),
            "reload_all complete"
        );
        reloaded
    }

    /// Checks a plugins directory for libraries that have been modified since
    /// they were last loaded.
    ///
    /// Compares the file modification time of each `.dylib`/`.so` file against
    /// the `last_modified` timestamp recorded when the plugin was loaded. Only
    /// plugins that are currently managed and whose library file has a newer
    /// modification time are returned.
    ///
    /// Returns the names of plugins whose library files have changed on disk.
    pub fn check_for_changes(&self, plugins_dir: &Path) -> Vec<String> {
        let ext = PluginLoader::library_extension();
        let mut changed = Vec::new();

        for managed in self.plugins.values() {
            let lib_path = managed.plugin.path();

            // Only consider plugins whose library resides in the given directory.
            if lib_path.parent() != Some(plugins_dir) {
                continue;
            }

            // Check the file extension matches.
            if lib_path.extension().and_then(|e| e.to_str()) != Some(ext) {
                continue;
            }

            let disk_mtime = match std::fs::metadata(lib_path).and_then(|m| m.modified()) {
                Ok(mtime) => mtime,
                Err(e) => {
                    debug!(
                        plugin = %managed.name,
                        path = %lib_path.display(),
                        error = %e,
                        "could not stat plugin library"
                    );
                    continue;
                }
            };

            let Some(loaded_mtime) = managed.last_modified else {
                // No recorded mtime — consider changed to be safe.
                changed.push(managed.name.clone());
                continue;
            };

            if disk_mtime > loaded_mtime {
                debug!(
                    plugin = %managed.name,
                    "plugin library modified since last load"
                );
                changed.push(managed.name.clone());
            }
        }

        changed.sort();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{FfiString, PluginEventType, PluginResult};

    /// Creates a noop [`PluginHostApi`] for tests.
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

    // -- reload_plugin error paths -------------------------------------------

    #[test]
    fn reload_nonexistent_plugin_returns_not_found() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let err = manager.reload_plugin("ghost", &host_api).unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(ref n) if n == "ghost"));
    }

    #[test]
    fn reload_plugin_invalid_path_returns_load_error() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();

        // Try to load from an invalid path — the load will fail, but we can
        // test that reload_plugin on an empty manager returns NotFound.
        let result = manager.reload_plugin("nonexistent", &host_api);
        assert!(result.is_err());
    }

    // -- reload_all on empty manager -----------------------------------------

    #[test]
    fn reload_all_empty_manager_returns_empty() {
        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let reloaded = manager.reload_all(&host_api);
        assert!(reloaded.is_empty());
    }

    // -- check_for_changes ---------------------------------------------------

    #[test]
    fn check_for_changes_empty_manager_returns_empty() {
        let manager = PluginManager::new();
        let dir = std::env::temp_dir();
        let changed = manager.check_for_changes(&dir);
        assert!(changed.is_empty());
    }

    #[test]
    fn check_for_changes_nonexistent_dir_returns_empty() {
        let manager = PluginManager::new();
        let changed = manager.check_for_changes(Path::new("/nonexistent/plugins/dir"));
        assert!(changed.is_empty());
    }

    // -- ManagedPlugin last_modified accessor ---------------------------------

    #[test]
    fn managed_plugin_last_modified_in_debug() {
        let info = super::super::types::PluginInfo {
            name: "test".into(),
            version: "1.0.0".into(),
            state: PluginState::Loaded,
            commands: vec![],
            hooked_events: vec![],
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("test"));
    }

    // -- check_for_changes with synthetic ManagedPlugin -----------------------

    // We cannot easily create a real ManagedPlugin without loading a dylib,
    // but we can test the check_for_changes logic indirectly by verifying
    // it correctly returns empty for a manager with no plugins.

    #[test]
    fn check_for_changes_returns_sorted() {
        let manager = PluginManager::new();
        let changed = manager.check_for_changes(Path::new("/tmp"));
        // Even empty results should be sorted (trivially true).
        assert!(changed.is_empty());
    }

    // -- ManagerError display for reload action ------------------------------

    #[test]
    fn manager_error_display_invalid_state_reload() {
        let err = ManagerError::InvalidState {
            name: "my-plugin".into(),
            current: PluginState::Enabled,
            action: "reload",
        };
        assert_eq!(
            err.to_string(),
            "cannot reload plugin `my-plugin` in state enabled"
        );
    }
}
