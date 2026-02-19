//! Command dispatch and event fan-out logic.
//!
//! This module contains the core dispatch routines used by
//! [`PluginManager`](crate::manager::PluginManager). They are extracted into a
//! separate module to keep the manager focused on lifecycle management.

use std::collections::HashMap;

use tracing::{debug, warn};

use crate::ffi::{PluginEventType, PluginStatus};
use crate::manager::{ManagedPlugin, ManagerError, PluginState};
use crate::registry::{CommandRegistry, EventRegistry};

/// Registers a command for the given plugin.
///
/// Command names are case-insensitive; the first plugin to register a name
/// wins.
///
/// # Errors
///
/// Returns [`ManagerError::NotFound`] if the plugin doesn't exist, or
/// [`ManagerError::CommandConflict`] if another plugin already owns
/// the command.
pub(crate) fn register_command(
    commands: &mut CommandRegistry,
    plugins: &mut HashMap<String, ManagedPlugin>,
    plugin_name: &str,
    command: &str,
    description: &str,
) -> Result<(), ManagerError> {
    if !plugins.contains_key(plugin_name) {
        return Err(ManagerError::NotFound(plugin_name.to_owned()));
    }
    commands.register(plugin_name, command, description)?;
    let managed = plugins.get_mut(plugin_name).expect("checked above");
    managed.add_command(command.to_lowercase());
    debug!(plugin = %plugin_name, command = %command, "command registered");
    Ok(())
}

/// Hooks an event type for the given plugin.
///
/// # Errors
///
/// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
pub(crate) fn hook_event(
    events: &mut EventRegistry,
    plugins: &mut HashMap<String, ManagedPlugin>,
    plugin_name: &str,
    event_type: PluginEventType,
) -> Result<(), ManagerError> {
    let managed = plugins
        .get_mut(plugin_name)
        .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
    managed.add_hooked_event(event_type);
    events.subscribe(plugin_name, event_type);
    debug!(plugin = %plugin_name, event = ?event_type, "event hooked");
    Ok(())
}

/// Unregisters a command from the given plugin.
///
/// # Errors
///
/// Returns [`ManagerError::NotFound`] if the plugin doesn't exist.
pub(crate) fn unregister_command(
    commands: &mut CommandRegistry,
    plugins: &mut HashMap<String, ManagedPlugin>,
    plugin_name: &str,
    command: &str,
) -> Result<bool, ManagerError> {
    let managed = plugins
        .get_mut(plugin_name)
        .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
    managed.remove_command(&command.to_lowercase());
    let removed = commands.unregister(plugin_name, command);
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
pub(crate) fn unhook_event(
    events: &mut EventRegistry,
    plugins: &mut HashMap<String, ManagedPlugin>,
    plugin_name: &str,
    event_type: PluginEventType,
) -> Result<bool, ManagerError> {
    let managed = plugins
        .get_mut(plugin_name)
        .ok_or_else(|| ManagerError::NotFound(plugin_name.to_owned()))?;
    managed.remove_hooked_event(event_type);
    let removed = events.unsubscribe(plugin_name, event_type);
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
pub(crate) fn dispatch_command(
    commands: &CommandRegistry,
    plugins: &HashMap<String, ManagedPlugin>,
    command: &str,
    args: &str,
) -> Result<bool, ManagerError> {
    let Some(entry) = commands.lookup(command) else {
        return Ok(false);
    };

    let Some(managed) = plugins.get(&entry.plugin_name) else {
        return Ok(false);
    };

    if managed.state() != PluginState::Enabled {
        debug!(
            plugin = %entry.plugin_name,
            command = %command,
            state = %managed.state(),
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
        (managed.loaded_plugin().api().on_event)(std::ptr::addr_of!(ffi_event));

    // Free FfiString allocations now that the plugin callback has returned.
    let crate::ffi::PluginEvent { data, source, .. } = ffi_event;
    unsafe {
        data.free();
        source.free();
    }

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
pub(crate) fn dispatch_event(
    events: &EventRegistry,
    plugins: &HashMap<String, ManagedPlugin>,
    event_type: PluginEventType,
    data: &str,
    source: &str,
) -> usize {
    let subscribers = events.subscribers(event_type);
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
        let Some(managed) = plugins.get(plugin_name) else {
            continue;
        };

        if managed.state() != PluginState::Enabled {
            debug!(
                plugin = %plugin_name,
                event = ?event_type,
                "event dispatch skipped: plugin not enabled"
            );
            continue;
        }

        let result = (managed.loaded_plugin().api().on_event)(
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

    // Free FfiString allocations now that all plugin callbacks have returned.
    let crate::ffi::PluginEvent {
        data: ffi_data,
        source: ffi_source,
        ..
    } = ffi_event;
    unsafe {
        ffi_data.free();
        ffi_source.free();
    }

    debug!(
        event = ?event_type,
        subscribers = subscribers.len(),
        delivered,
        "event dispatched"
    );
    delivered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_command_nonexistent_returns_not_found() {
        let mut commands = CommandRegistry::new();
        let mut plugins = HashMap::new();
        let err = register_command(&mut commands, &mut plugins, "ghost", "test", "desc")
            .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    #[test]
    fn hook_event_nonexistent_returns_not_found() {
        let mut events = EventRegistry::new();
        let mut plugins = HashMap::new();
        let err = hook_event(
            &mut events,
            &mut plugins,
            "ghost",
            PluginEventType::Connected,
        )
        .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    #[test]
    fn dispatch_command_no_match_returns_false() {
        let commands = CommandRegistry::new();
        let plugins = HashMap::new();
        let result = dispatch_command(&commands, &plugins, "nonexistent", "").unwrap();
        assert!(!result);
    }

    #[test]
    fn dispatch_event_no_subscribers_returns_zero() {
        let events = EventRegistry::new();
        let plugins = HashMap::new();
        let count = dispatch_event(&events, &plugins, PluginEventType::Connected, "", "");
        assert_eq!(count, 0);
    }

    #[test]
    fn unregister_command_nonexistent_returns_not_found() {
        let mut commands = CommandRegistry::new();
        let mut plugins = HashMap::new();
        let err = unregister_command(&mut commands, &mut plugins, "ghost", "test")
            .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    #[test]
    fn unhook_event_nonexistent_returns_not_found() {
        let mut events = EventRegistry::new();
        let mut plugins = HashMap::new();
        let err = unhook_event(
            &mut events,
            &mut plugins,
            "ghost",
            PluginEventType::Connected,
        )
        .unwrap_err();
        assert!(matches!(err, ManagerError::NotFound(_)));
    }

    #[test]
    fn command_registry_starts_empty() {
        let commands = CommandRegistry::new();
        assert!(commands.is_empty());
    }

    #[test]
    fn event_registry_starts_without_subscribers() {
        let events = EventRegistry::new();
        assert!(!events.has_subscribers(PluginEventType::Connected));
    }

    #[test]
    fn manager_error_display_command_conflict() {
        use crate::registry::CommandError;
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
        use crate::registry::CommandError;
        use std::error::Error;
        let err = ManagerError::CommandConflict(CommandError::AlreadyRegistered {
            command: "test".into(),
            owner: "owner".into(),
        });
        assert!(err.source().is_some());
    }

    #[test]
    fn manager_error_from_command_error() {
        use crate::registry::CommandError;
        let cmd_err = CommandError::AlreadyRegistered {
            command: "test".into(),
            owner: "owner".into(),
        };
        let manager_err: ManagerError = cmd_err.into();
        assert!(matches!(manager_err, ManagerError::CommandConflict(_)));
    }
}
