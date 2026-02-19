//! Plugin system integration for the pirc client.
//!
//! Provides:
//! - [`ClientPluginHost`] — concrete `PluginHostApi` vtable implementation
//!   that routes `echo()` to the client status buffer and `log()` to `tracing`.
//! - `/plugin` command handlers on [`App`].
//! - Plugin initialization during client startup.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use pirc_plugin::ffi::{FfiString, PluginEventType, PluginHostApi, PluginResult};
use pirc_plugin::manager::PluginState;
use tracing::{debug, error, info, warn};

use crate::client_command::PluginSubcommand;

use super::App;

// ---------------------------------------------------------------------------
// Thread-local echo sink for plugin host callbacks
// ---------------------------------------------------------------------------

// Messages produced by plugin echo() calls are buffered here and drained
// by the client after each plugin operation.
thread_local! {
    static ECHO_SINK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Drains all pending echo messages produced by plugins.
fn drain_echo_messages() -> Vec<String> {
    ECHO_SINK.with(|sink| std::mem::take(&mut *sink.borrow_mut()))
}

// ---------------------------------------------------------------------------
// PluginHostApi extern "C" callbacks
// ---------------------------------------------------------------------------

extern "C" fn host_register_command(
    _name: FfiString,
    _cb: extern "C" fn(FfiString) -> PluginResult,
) -> PluginResult {
    // Command registration is handled by the PluginManager itself
    // during init; the host callback is a no-op at this layer.
    PluginResult::ok()
}

extern "C" fn host_unregister_command(_name: FfiString) -> PluginResult {
    PluginResult::ok()
}

extern "C" fn host_hook_event(_et: PluginEventType) -> PluginResult {
    PluginResult::ok()
}

extern "C" fn host_unhook_event(_et: PluginEventType) -> PluginResult {
    PluginResult::ok()
}

extern "C" fn host_echo(msg: FfiString) {
    #[allow(unsafe_code)]
    let text = unsafe { msg.as_str().to_owned() };
    #[allow(unsafe_code)]
    unsafe {
        msg.free();
    }
    ECHO_SINK.with(|sink| sink.borrow_mut().push(text));
}

extern "C" fn host_log(level: u32, msg: FfiString) {
    #[allow(unsafe_code)]
    let text = unsafe { msg.as_str().to_owned() };
    #[allow(unsafe_code)]
    unsafe {
        msg.free();
    }
    match level {
        0 => error!(target: "plugin", "{text}"),
        1 => warn!(target: "plugin", "{text}"),
        2 => info!(target: "plugin", "{text}"),
        _ => debug!(target: "plugin", "{text}"),
    }
}

extern "C" fn host_get_config_value(_key: FfiString) -> FfiString {
    // Config values are resolved by the PluginManager; the host
    // callback returns empty for now.
    FfiString::empty()
}

/// Creates the concrete [`PluginHostApi`] vtable used by the client.
#[must_use]
pub fn create_host_api() -> PluginHostApi {
    PluginHostApi {
        register_command: host_register_command,
        unregister_command: host_unregister_command,
        hook_event: host_hook_event,
        unhook_event: host_unhook_event,
        echo: host_echo,
        log: host_log,
        get_config_value: host_get_config_value,
    }
}

// ---------------------------------------------------------------------------
// Plugin initialization
// ---------------------------------------------------------------------------

impl App {
    /// Initialise the plugin system on client startup.
    ///
    /// Checks the `plugins.enabled` config flag, resolves the plugins
    /// directory, creates it if missing, and loads all plugins found there.
    pub(super) fn init_plugins(&mut self) {
        if !self.config.plugins.enabled {
            info!("plugin system disabled by configuration");
            return;
        }

        let plugins_dir = resolve_plugins_dir(self.config.plugins.plugins_dir.as_ref());
        let Some(dir) = plugins_dir else {
            warn!("could not determine plugins directory");
            return;
        };

        // Create directory if it doesn't exist.
        if !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                warn!(path = %dir.display(), error = %e, "failed to create plugins directory");
                return;
            }
            info!(path = %dir.display(), "created plugins directory");
        }

        let host_api = create_host_api();
        let loaded = self.plugin_manager.load_plugins_dir(&dir, &host_api);

        // Auto-enable plugins whose config says enabled=true.
        for name in &loaded {
            if let Some(plugin) = self.plugin_manager.get_plugin(name) {
                if plugin.config().enabled && plugin.state() == PluginState::Loaded {
                    if let Err(e) = self.plugin_manager.enable_plugin(name) {
                        warn!(plugin = %name, error = %e, "failed to auto-enable plugin");
                    }
                }
            }
        }

        // Drain any echo messages produced during init.
        for msg in drain_echo_messages() {
            self.push_status(&msg);
        }

        if loaded.is_empty() {
            debug!(path = %dir.display(), "no plugins found");
        } else {
            self.push_status(&format!(
                "Loaded {} plugin(s)",
                loaded.len()
            ));
        }
    }

    // -----------------------------------------------------------------------
    // /plugin command handlers
    // -----------------------------------------------------------------------

    /// Handle `/plugin` subcommands.
    pub(super) fn handle_plugin_command(&mut self, sub: &PluginSubcommand) {
        match sub {
            PluginSubcommand::List => self.plugin_list(),
            PluginSubcommand::Load(path) => self.plugin_load(path),
            PluginSubcommand::Unload(name) => self.plugin_unload(name),
            PluginSubcommand::Reload(name) => self.plugin_reload(name),
            PluginSubcommand::Enable(name) => self.plugin_enable(name),
            PluginSubcommand::Disable(name) => self.plugin_disable(name),
            PluginSubcommand::Info(name) => self.plugin_info(name),
        }
    }

    fn plugin_list(&mut self) {
        let plugins = self.plugin_manager.list_plugins();
        if plugins.is_empty() {
            self.push_status("No plugins loaded");
            return;
        }
        self.push_status(&format!("Loaded plugins ({}):", plugins.len()));
        for p in &plugins {
            self.push_status(&format!("  {} v{} [{}]", p.name, p.version, p.state));
        }
    }

    fn plugin_load(&mut self, path: &str) {
        let path = Path::new(path);
        let host_api = create_host_api();
        match self.plugin_manager.load_plugin(path, &host_api) {
            Ok(name) => {
                // Auto-enable if config says so.
                if let Some(plugin) = self.plugin_manager.get_plugin(&name) {
                    if plugin.config().enabled && plugin.state() == PluginState::Loaded {
                        if let Err(e) = self.plugin_manager.enable_plugin(&name) {
                            self.push_status(&format!("Plugin {name} loaded but enable failed: {e}"));
                            for msg in drain_echo_messages() {
                                self.push_status(&msg);
                            }
                            return;
                        }
                    }
                }
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Plugin {name} loaded"));
            }
            Err(e) => {
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Failed to load plugin: {e}"));
            }
        }
    }

    fn plugin_unload(&mut self, name: &str) {
        // Must disable first if enabled.
        if let Some(plugin) = self.plugin_manager.get_plugin(name) {
            if plugin.state() == PluginState::Enabled {
                if let Err(e) = self.plugin_manager.disable_plugin(name) {
                    self.push_status(&format!("Failed to disable plugin before unload: {e}"));
                    return;
                }
            }
        }
        match self.plugin_manager.unload_plugin(name) {
            Ok(()) => self.push_status(&format!("Plugin {name} unloaded")),
            Err(e) => self.push_status(&format!("Failed to unload plugin: {e}")),
        }
    }

    fn plugin_reload(&mut self, name: &str) {
        let host_api = create_host_api();
        match self.plugin_manager.reload_plugin(name, &host_api) {
            Ok(()) => {
                // Auto-enable after reload.
                if let Some(plugin) = self.plugin_manager.get_plugin(name) {
                    if plugin.config().enabled && plugin.state() == PluginState::Loaded {
                        if let Err(e) = self.plugin_manager.enable_plugin(name) {
                            self.push_status(&format!("Plugin {name} reloaded but enable failed: {e}"));
                            for msg in drain_echo_messages() {
                                self.push_status(&msg);
                            }
                            return;
                        }
                    }
                }
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Plugin {name} reloaded"));
            }
            Err(e) => {
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Failed to reload plugin: {e}"));
            }
        }
    }

    fn plugin_enable(&mut self, name: &str) {
        match self.plugin_manager.enable_plugin(name) {
            Ok(()) => {
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Plugin {name} enabled"));
            }
            Err(e) => {
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Failed to enable plugin: {e}"));
            }
        }
    }

    fn plugin_disable(&mut self, name: &str) {
        match self.plugin_manager.disable_plugin(name) {
            Ok(()) => self.push_status(&format!("Plugin {name} disabled")),
            Err(e) => self.push_status(&format!("Failed to disable plugin: {e}")),
        }
    }

    fn plugin_info(&mut self, name: &str) {
        let Some(plugin) = self.plugin_manager.get_plugin(name) else {
            self.push_status(&format!("Plugin '{name}' not found"));
            return;
        };

        // Collect all info while borrowing plugin_manager, then display.
        let info = plugin.info();
        let cap_count = plugin.capabilities().capability_count();

        let mut lines = vec![
            format!("Plugin: {}", info.name),
            format!("  Version: {}", info.version),
            format!("  State: {}", info.state),
        ];

        if info.commands.is_empty() {
            lines.push("  Commands: (none)".into());
        } else {
            lines.push(format!("  Commands: {}", info.commands.join(", ")));
        }

        if info.hooked_events.is_empty() {
            lines.push("  Events: (none)".into());
        } else {
            let events: Vec<String> = info.hooked_events.iter().map(|e| format!("{e:?}")).collect();
            lines.push(format!("  Events: {}", events.join(", ")));
        }

        lines.push(format!("  Capabilities: {cap_count}"));

        for line in &lines {
            self.push_status(line);
        }
    }

    // -----------------------------------------------------------------------
    // Event dispatch wiring
    // -----------------------------------------------------------------------

    /// Map an IRC message to plugin event(s) and dispatch.
    pub(super) fn dispatch_irc_event_to_plugins(&self, msg: &pirc_protocol::Message) {
        use pirc_protocol::Command;

        let prefix_str = msg
            .prefix
            .as_ref()
            .map_or(String::new(), ToString::to_string);

        match msg.command {
            Command::Privmsg | Command::Notice => {
                let data = msg.params.get(1).map_or("", String::as_str);
                self.dispatch_plugin_event(PluginEventType::MessageReceived, data, &prefix_str);
            }
            Command::Join => {
                let channel = msg.params.first().map_or("", String::as_str);
                self.dispatch_plugin_event(PluginEventType::UserJoined, channel, &prefix_str);
            }
            Command::Part => {
                let channel = msg.params.first().map_or("", String::as_str);
                self.dispatch_plugin_event(PluginEventType::UserParted, channel, &prefix_str);
            }
            Command::Quit => {
                let reason = msg.params.first().map_or("", String::as_str);
                self.dispatch_plugin_event(PluginEventType::UserQuit, reason, &prefix_str);
            }
            Command::Nick => {
                let new_nick = msg.params.first().map_or("", String::as_str);
                self.dispatch_plugin_event(PluginEventType::NickChanged, new_nick, &prefix_str);
            }
            _ => {}
        }
    }

    /// Dispatch an IRC event to all subscribed plugins.
    pub(super) fn dispatch_plugin_event(
        &self,
        event_type: PluginEventType,
        data: &str,
        source: &str,
    ) {
        self.plugin_manager.dispatch_event(event_type, data, source);
    }

    /// Try to dispatch a command to a plugin.
    ///
    /// Returns `true` if a plugin handled the command.
    pub(super) fn dispatch_plugin_command(&mut self, command: &str, args: &str) -> bool {
        match self.plugin_manager.dispatch_command(command, args) {
            Ok(handled) => {
                // Drain echo messages from plugin execution.
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                handled
            }
            Err(e) => {
                for msg in drain_echo_messages() {
                    self.push_status(&msg);
                }
                self.push_status(&format!("Plugin command error: {e}"));
                false
            }
        }
    }
}

/// Resolve the plugins directory from config or default location.
fn resolve_plugins_dir(config_dir: Option<&String>) -> Option<PathBuf> {
    if let Some(dir) = config_dir {
        Some(PathBuf::from(dir))
    } else {
        pirc_common::config::plugins_dir()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pirc_plugin::manager::PluginManager;

    #[test]
    fn resolve_plugins_dir_with_config() {
        let custom = String::from("/custom/plugins");
        let dir = resolve_plugins_dir(Some(&custom));
        assert_eq!(dir, Some(PathBuf::from("/custom/plugins")));
    }

    #[test]
    fn resolve_plugins_dir_default() {
        let dir = resolve_plugins_dir(None);
        // Should return Some on most systems (may be None if HOME is unset).
        // Just verify it doesn't panic.
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("plugins"));
        }
    }

    #[test]
    fn create_host_api_is_valid() {
        let api = create_host_api();
        // Verify we can call the callbacks without panicking.
        let result = (api.hook_event)(PluginEventType::Connected);
        assert!(result.is_ok());
        let result = (api.unhook_event)(PluginEventType::Connected);
        assert!(result.is_ok());
    }

    #[test]
    fn echo_sink_collects_messages() {
        ECHO_SINK.with(|sink| sink.borrow_mut().clear());
        host_echo(FfiString::new("hello from plugin"));
        let msgs = drain_echo_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], "hello from plugin");
    }

    #[test]
    fn drain_echo_messages_clears_sink() {
        ECHO_SINK.with(|sink| sink.borrow_mut().clear());
        host_echo(FfiString::new("msg1"));
        host_echo(FfiString::new("msg2"));
        let msgs = drain_echo_messages();
        assert_eq!(msgs.len(), 2);
        let msgs = drain_echo_messages();
        assert!(msgs.is_empty());
    }

    #[test]
    fn host_log_does_not_panic() {
        // Just verify logging callbacks don't crash.
        host_log(0, FfiString::new("error message"));
        host_log(1, FfiString::new("warn message"));
        host_log(2, FfiString::new("info message"));
        host_log(3, FfiString::new("debug message"));
        host_log(99, FfiString::new("unknown level"));
    }

    #[test]
    fn host_get_config_value_returns_empty() {
        let result = host_get_config_value(FfiString::new("some_key"));
        assert!(result.is_null());
    }

    #[test]
    fn host_register_and_unregister_are_noop() {
        extern "C" fn noop_cb(_args: FfiString) -> PluginResult {
            PluginResult::ok()
        }
        let result = host_register_command(FfiString::new("test"), noop_cb);
        assert!(result.is_ok());
        let result = host_unregister_command(FfiString::new("test"));
        assert!(result.is_ok());
    }

    #[test]
    fn plugin_manager_new_is_empty() {
        let manager = PluginManager::new();
        assert_eq!(manager.plugin_count(), 0);
        assert!(manager.list_plugins().is_empty());
    }
}
