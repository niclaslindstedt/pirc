//! Capability-based sandboxing for plugins.
//!
//! Each plugin declares its required capabilities via
//! [`Plugin::capabilities()`](crate::plugin::Plugin::capabilities).
//! The [`CapabilityChecker`] enforces these declarations at runtime by
//! gating host API callbacks — a plugin that did not declare
//! [`RegisterCommands`](crate::ffi::PluginCapability::RegisterCommands),
//! for example, will receive a [`PluginError::PermissionDenied`] when it
//! attempts to register a command.

use std::collections::HashSet;

use crate::ffi::PluginCapability;
use crate::plugin::PluginError;

/// Enforces declared plugin capabilities at runtime.
///
/// Constructed from a plugin's declared capability list, the checker
/// provides `check` (boolean) and `require` (fallible) methods that
/// the host calls before executing any privileged operation.
#[derive(Debug, Clone)]
pub struct CapabilityChecker {
    capabilities: HashSet<PluginCapability>,
    plugin_name: String,
}

impl CapabilityChecker {
    /// Creates a new checker from the plugin's declared capabilities.
    #[must_use]
    pub fn new(plugin_name: &str, capabilities: &[PluginCapability]) -> Self {
        Self {
            capabilities: capabilities.iter().copied().collect(),
            plugin_name: plugin_name.to_owned(),
        }
    }

    /// Returns `true` if this plugin declared the given capability.
    #[must_use]
    pub fn check(&self, capability: PluginCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// Checks that the plugin declared the given capability, returning
    /// [`PluginError::PermissionDenied`] if it did not.
    ///
    /// # Errors
    ///
    /// Returns `PermissionDenied` with the plugin name and a description
    /// of the denied action.
    pub fn require(&self, capability: PluginCapability) -> Result<(), PluginError> {
        if self.check(capability) {
            Ok(())
        } else {
            Err(PluginError::PermissionDenied {
                plugin: self.plugin_name.clone(),
                action: capability_action(capability).to_owned(),
            })
        }
    }

    /// Returns the plugin name associated with this checker.
    #[must_use]
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Returns the number of declared capabilities.
    #[must_use]
    pub fn capability_count(&self) -> usize {
        self.capabilities.len()
    }
}

/// Maps a capability to a human-readable action description used in
/// error messages and log output.
fn capability_action(cap: PluginCapability) -> &'static str {
    match cap {
        PluginCapability::ReadConfig => "read configuration",
        PluginCapability::RegisterCommands => "register commands",
        PluginCapability::HookEvents => "hook events",
        PluginCapability::SendMessages => "send messages",
        PluginCapability::AccessNetwork => "access network",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checker_with_all_capabilities() {
        let caps = [
            PluginCapability::ReadConfig,
            PluginCapability::RegisterCommands,
            PluginCapability::HookEvents,
            PluginCapability::SendMessages,
            PluginCapability::AccessNetwork,
        ];
        let checker = CapabilityChecker::new("test-plugin", &caps);
        assert_eq!(checker.capability_count(), 5);
        assert_eq!(checker.plugin_name(), "test-plugin");
        for cap in &caps {
            assert!(checker.check(*cap));
            assert!(checker.require(*cap).is_ok());
        }
    }

    #[test]
    fn checker_with_no_capabilities() {
        let checker = CapabilityChecker::new("empty-plugin", &[]);
        assert_eq!(checker.capability_count(), 0);
        assert!(!checker.check(PluginCapability::ReadConfig));
        assert!(!checker.check(PluginCapability::RegisterCommands));
        assert!(!checker.check(PluginCapability::HookEvents));
        assert!(!checker.check(PluginCapability::SendMessages));
        assert!(!checker.check(PluginCapability::AccessNetwork));
    }

    #[test]
    fn require_denied_returns_permission_denied() {
        let checker = CapabilityChecker::new("restricted", &[]);
        let err = checker
            .require(PluginCapability::RegisterCommands)
            .unwrap_err();
        match &err {
            PluginError::PermissionDenied { plugin, action } => {
                assert_eq!(plugin, "restricted");
                assert_eq!(action, "register commands");
            }
            other => panic!("expected PermissionDenied, got: {other}"),
        }
    }

    #[test]
    fn require_allowed_returns_ok() {
        let checker =
            CapabilityChecker::new("capable", &[PluginCapability::SendMessages]);
        assert!(checker.require(PluginCapability::SendMessages).is_ok());
    }

    #[test]
    fn partial_capabilities_are_enforced() {
        let checker = CapabilityChecker::new(
            "partial",
            &[PluginCapability::RegisterCommands, PluginCapability::HookEvents],
        );
        assert!(checker.check(PluginCapability::RegisterCommands));
        assert!(checker.check(PluginCapability::HookEvents));
        assert!(!checker.check(PluginCapability::SendMessages));
        assert!(!checker.check(PluginCapability::ReadConfig));
        assert!(!checker.check(PluginCapability::AccessNetwork));
    }

    #[test]
    fn duplicate_capabilities_are_deduplicated() {
        let checker = CapabilityChecker::new(
            "dup",
            &[
                PluginCapability::ReadConfig,
                PluginCapability::ReadConfig,
                PluginCapability::ReadConfig,
            ],
        );
        assert_eq!(checker.capability_count(), 1);
        assert!(checker.check(PluginCapability::ReadConfig));
    }

    #[test]
    fn permission_denied_error_display() {
        let checker = CapabilityChecker::new("my-plugin", &[]);
        let err = checker
            .require(PluginCapability::SendMessages)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("my-plugin"));
        assert!(msg.contains("send messages"));
    }

    #[test]
    fn each_capability_has_distinct_action_description() {
        let actions: Vec<&str> = [
            PluginCapability::ReadConfig,
            PluginCapability::RegisterCommands,
            PluginCapability::HookEvents,
            PluginCapability::SendMessages,
            PluginCapability::AccessNetwork,
        ]
        .iter()
        .map(|c| capability_action(*c))
        .collect();

        // All descriptions should be unique.
        let unique: HashSet<&&str> = actions.iter().collect();
        assert_eq!(unique.len(), actions.len());
    }

    #[test]
    fn checker_clone() {
        let checker = CapabilityChecker::new(
            "cloneable",
            &[PluginCapability::ReadConfig],
        );
        let cloned = checker.clone();
        assert_eq!(cloned.plugin_name(), "cloneable");
        assert!(cloned.check(PluginCapability::ReadConfig));
    }

    #[test]
    fn checker_debug_format() {
        let checker = CapabilityChecker::new("dbg", &[PluginCapability::HookEvents]);
        let debug = format!("{checker:?}");
        assert!(debug.contains("CapabilityChecker"));
        assert!(debug.contains("dbg"));
    }
}
