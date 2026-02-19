//! Command and event registries for the plugin system.
//!
//! [`CommandRegistry`] tracks which plugin owns which slash-command, enforcing
//! uniqueness (first registrant wins) and case-insensitive lookup.
//!
//! [`EventRegistry`] tracks which plugins are subscribed to which event types,
//! supporting fan-out dispatch where multiple plugins receive the same event.

use std::collections::{HashMap, HashSet};

use crate::ffi::PluginEventType;

// ---------------------------------------------------------------------------
// CommandEntry
// ---------------------------------------------------------------------------

/// A registered command with its owning plugin and description.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    /// The plugin that registered this command.
    pub plugin_name: String,
    /// Human-readable description of the command.
    pub description: String,
}

// ---------------------------------------------------------------------------
// CommandRegistry
// ---------------------------------------------------------------------------

/// Registry for plugin commands.
///
/// Command names are stored lowercase for case-insensitive lookup.
/// Each command can only be owned by one plugin (first registrant wins).
#[derive(Debug, Default)]
pub struct CommandRegistry {
    /// Map from lowercase command name to the entry (plugin + description).
    commands: HashMap<String, CommandEntry>,
}

/// Errors that can occur during command registration.
#[derive(Debug)]
pub enum CommandError {
    /// A command with that name is already registered by another plugin.
    AlreadyRegistered {
        command: String,
        owner: String,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered { command, owner } => {
                write!(
                    f,
                    "command `{command}` is already registered by plugin `{owner}`"
                )
            }
        }
    }
}

impl std::error::Error for CommandError {}

impl CommandRegistry {
    /// Creates a new, empty command registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a command for the given plugin.
    ///
    /// Command names are normalised to lowercase. If another plugin already
    /// owns this command, an error is returned (first registrant wins).
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::AlreadyRegistered`] if the command name is
    /// already taken by a different plugin.
    pub fn register(
        &mut self,
        plugin_name: &str,
        command: &str,
        description: &str,
    ) -> Result<(), CommandError> {
        let key = command.to_lowercase();

        if let Some(existing) = self.commands.get(&key) {
            // Allow re-registration by the same plugin (idempotent).
            if existing.plugin_name == plugin_name {
                return Ok(());
            }
            return Err(CommandError::AlreadyRegistered {
                command: key,
                owner: existing.plugin_name.clone(),
            });
        }

        self.commands.insert(
            key,
            CommandEntry {
                plugin_name: plugin_name.to_owned(),
                description: description.to_owned(),
            },
        );
        Ok(())
    }

    /// Unregisters a single command owned by the given plugin.
    ///
    /// Returns `true` if the command was found and removed.
    pub fn unregister(&mut self, plugin_name: &str, command: &str) -> bool {
        let key = command.to_lowercase();
        if let Some(entry) = self.commands.get(&key) {
            if entry.plugin_name == plugin_name {
                self.commands.remove(&key);
                return true;
            }
        }
        false
    }

    /// Unregisters all commands owned by the given plugin.
    ///
    /// Returns the number of commands removed.
    pub fn unregister_all(&mut self, plugin_name: &str) -> usize {
        let before = self.commands.len();
        self.commands
            .retain(|_, entry| entry.plugin_name != plugin_name);
        before - self.commands.len()
    }

    /// Looks up which plugin handles the given command.
    ///
    /// Command names are matched case-insensitively.
    #[must_use]
    pub fn lookup(&self, command: &str) -> Option<&CommandEntry> {
        self.commands.get(&command.to_lowercase())
    }

    /// Returns a list of all registered commands with their owner and description.
    ///
    /// The list is sorted by command name for deterministic output.
    #[must_use]
    pub fn list(&self) -> Vec<(String, String, String)> {
        let mut entries: Vec<_> = self
            .commands
            .iter()
            .map(|(cmd, entry)| {
                (
                    cmd.clone(),
                    entry.plugin_name.clone(),
                    entry.description.clone(),
                )
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Returns the number of registered commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` if no commands are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

// ---------------------------------------------------------------------------
// EventRegistry
// ---------------------------------------------------------------------------

/// Registry for plugin event subscriptions.
///
/// Multiple plugins can subscribe to the same event type. The registry tracks
/// which plugins are interested in which events for fan-out dispatch.
#[derive(Debug, Default)]
pub struct EventRegistry {
    /// Map from event type to the set of subscribed plugin names.
    subscriptions: HashMap<PluginEventType, HashSet<String>>,
}

impl EventRegistry {
    /// Creates a new, empty event registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes a plugin to an event type.
    ///
    /// Subscribing to the same event multiple times is idempotent.
    pub fn subscribe(&mut self, plugin_name: &str, event_type: PluginEventType) {
        self.subscriptions
            .entry(event_type)
            .or_default()
            .insert(plugin_name.to_owned());
    }

    /// Unsubscribes a plugin from an event type.
    ///
    /// Returns `true` if the plugin was previously subscribed.
    pub fn unsubscribe(&mut self, plugin_name: &str, event_type: PluginEventType) -> bool {
        if let Some(subs) = self.subscriptions.get_mut(&event_type) {
            let removed = subs.remove(plugin_name);
            if subs.is_empty() {
                self.subscriptions.remove(&event_type);
            }
            return removed;
        }
        false
    }

    /// Unsubscribes a plugin from all event types.
    ///
    /// Returns the number of event types the plugin was unsubscribed from.
    pub fn unsubscribe_all(&mut self, plugin_name: &str) -> usize {
        let mut count = 0;
        self.subscriptions.retain(|_, subs| {
            if subs.remove(plugin_name) {
                count += 1;
            }
            !subs.is_empty()
        });
        count
    }

    /// Returns the names of all plugins subscribed to the given event type.
    ///
    /// The list is sorted for deterministic dispatch order.
    #[must_use]
    pub fn subscribers(&self, event_type: PluginEventType) -> Vec<String> {
        match self.subscriptions.get(&event_type) {
            Some(subs) => {
                let mut names: Vec<_> = subs.iter().cloned().collect();
                names.sort();
                names
            }
            None => Vec::new(),
        }
    }

    /// Returns `true` if any plugin is subscribed to the given event type.
    #[must_use]
    pub fn has_subscribers(&self, event_type: PluginEventType) -> bool {
        self.subscriptions
            .get(&event_type)
            .is_some_and(|subs| !subs.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- CommandRegistry tests ------------------------------------------------

    #[test]
    fn register_and_lookup_command() {
        let mut reg = CommandRegistry::new();
        reg.register("my-plugin", "hello", "Say hello")
            .unwrap();

        let entry = reg.lookup("hello").unwrap();
        assert_eq!(entry.plugin_name, "my-plugin");
        assert_eq!(entry.description, "Say hello");
    }

    #[test]
    fn command_lookup_is_case_insensitive() {
        let mut reg = CommandRegistry::new();
        reg.register("my-plugin", "Hello", "Say hello")
            .unwrap();

        assert!(reg.lookup("hello").is_some());
        assert!(reg.lookup("HELLO").is_some());
        assert!(reg.lookup("HeLLo").is_some());
    }

    #[test]
    fn duplicate_command_same_plugin_is_idempotent() {
        let mut reg = CommandRegistry::new();
        reg.register("my-plugin", "hello", "Say hello")
            .unwrap();
        // Same plugin can re-register without error.
        reg.register("my-plugin", "hello", "Say hello again")
            .unwrap();
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn duplicate_command_different_plugin_returns_error() {
        let mut reg = CommandRegistry::new();
        reg.register("plugin-a", "hello", "Say hello")
            .unwrap();

        let err = reg
            .register("plugin-b", "hello", "Also hello")
            .unwrap_err();
        assert!(matches!(err, CommandError::AlreadyRegistered { .. }));
        let msg = err.to_string();
        assert!(msg.contains("hello"));
        assert!(msg.contains("plugin-a"));
    }

    #[test]
    fn unregister_command() {
        let mut reg = CommandRegistry::new();
        reg.register("my-plugin", "hello", "Say hello")
            .unwrap();
        assert!(reg.unregister("my-plugin", "hello"));
        assert!(reg.lookup("hello").is_none());
    }

    #[test]
    fn unregister_wrong_plugin_does_nothing() {
        let mut reg = CommandRegistry::new();
        reg.register("plugin-a", "hello", "Say hello")
            .unwrap();
        assert!(!reg.unregister("plugin-b", "hello"));
        assert!(reg.lookup("hello").is_some());
    }

    #[test]
    fn unregister_nonexistent_command_returns_false() {
        let mut reg = CommandRegistry::new();
        assert!(!reg.unregister("my-plugin", "missing"));
    }

    #[test]
    fn unregister_all_removes_plugin_commands() {
        let mut reg = CommandRegistry::new();
        reg.register("my-plugin", "cmd1", "First").unwrap();
        reg.register("my-plugin", "cmd2", "Second").unwrap();
        reg.register("other-plugin", "cmd3", "Third").unwrap();

        let removed = reg.unregister_all("my-plugin");
        assert_eq!(removed, 2);
        assert!(reg.lookup("cmd1").is_none());
        assert!(reg.lookup("cmd2").is_none());
        assert!(reg.lookup("cmd3").is_some());
    }

    #[test]
    fn unregister_all_nonexistent_plugin_removes_nothing() {
        let mut reg = CommandRegistry::new();
        reg.register("plugin-a", "hello", "Say hello")
            .unwrap();
        let removed = reg.unregister_all("plugin-b");
        assert_eq!(removed, 0);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn list_commands_sorted() {
        let mut reg = CommandRegistry::new();
        reg.register("plugin-b", "zebra", "Z cmd").unwrap();
        reg.register("plugin-a", "alpha", "A cmd").unwrap();

        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "alpha");
        assert_eq!(list[1].0, "zebra");
    }

    #[test]
    fn command_registry_is_empty() {
        let reg = CommandRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn lookup_nonexistent_returns_none() {
        let reg = CommandRegistry::new();
        assert!(reg.lookup("missing").is_none());
    }

    #[test]
    fn command_error_is_std_error() {
        let err: Box<dyn std::error::Error> =
            Box::new(CommandError::AlreadyRegistered {
                command: "test".into(),
                owner: "owner".into(),
            });
        assert!(err.to_string().contains("test"));
    }

    // -- EventRegistry tests --------------------------------------------------

    #[test]
    fn subscribe_and_get_subscribers() {
        let mut reg = EventRegistry::new();
        reg.subscribe("my-plugin", PluginEventType::Connected);

        let subs = reg.subscribers(PluginEventType::Connected);
        assert_eq!(subs, vec!["my-plugin"]);
    }

    #[test]
    fn multiple_plugins_same_event() {
        let mut reg = EventRegistry::new();
        reg.subscribe("plugin-a", PluginEventType::MessageReceived);
        reg.subscribe("plugin-b", PluginEventType::MessageReceived);

        let subs = reg.subscribers(PluginEventType::MessageReceived);
        assert_eq!(subs.len(), 2);
        // Sorted deterministically.
        assert_eq!(subs[0], "plugin-a");
        assert_eq!(subs[1], "plugin-b");
    }

    #[test]
    fn subscribe_is_idempotent() {
        let mut reg = EventRegistry::new();
        reg.subscribe("my-plugin", PluginEventType::Connected);
        reg.subscribe("my-plugin", PluginEventType::Connected);

        let subs = reg.subscribers(PluginEventType::Connected);
        assert_eq!(subs.len(), 1);
    }

    #[test]
    fn unsubscribe_event() {
        let mut reg = EventRegistry::new();
        reg.subscribe("my-plugin", PluginEventType::Connected);
        assert!(reg.unsubscribe("my-plugin", PluginEventType::Connected));
        assert!(reg.subscribers(PluginEventType::Connected).is_empty());
    }

    #[test]
    fn unsubscribe_not_subscribed_returns_false() {
        let mut reg = EventRegistry::new();
        assert!(!reg.unsubscribe("my-plugin", PluginEventType::Connected));
    }

    #[test]
    fn unsubscribe_all_events() {
        let mut reg = EventRegistry::new();
        reg.subscribe("my-plugin", PluginEventType::Connected);
        reg.subscribe("my-plugin", PluginEventType::Disconnected);
        reg.subscribe("other-plugin", PluginEventType::Connected);

        let removed = reg.unsubscribe_all("my-plugin");
        assert_eq!(removed, 2);
        assert!(!reg.has_subscribers(PluginEventType::Disconnected));
        // other-plugin is still subscribed.
        assert!(reg.has_subscribers(PluginEventType::Connected));
        let subs = reg.subscribers(PluginEventType::Connected);
        assert_eq!(subs, vec!["other-plugin"]);
    }

    #[test]
    fn unsubscribe_all_nonexistent_plugin() {
        let mut reg = EventRegistry::new();
        reg.subscribe("plugin-a", PluginEventType::Connected);
        let removed = reg.unsubscribe_all("plugin-b");
        assert_eq!(removed, 0);
    }

    #[test]
    fn subscribers_empty_event_type() {
        let reg = EventRegistry::new();
        assert!(reg.subscribers(PluginEventType::UserJoined).is_empty());
    }

    #[test]
    fn has_subscribers_true_and_false() {
        let mut reg = EventRegistry::new();
        assert!(!reg.has_subscribers(PluginEventType::Connected));

        reg.subscribe("my-plugin", PluginEventType::Connected);
        assert!(reg.has_subscribers(PluginEventType::Connected));
        assert!(!reg.has_subscribers(PluginEventType::Disconnected));
    }
}
