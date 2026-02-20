//! Capability-based sandboxing integration tests.
//!
//! Tests verify that the manager enforces plugin capabilities: plugins without
//! the required capability are denied access to protected operations.

use pirc_plugin::ffi::{PluginCapability, PluginEventType};
use pirc_plugin::manager::{ManagerError, PluginManager};
use pirc_plugin::sandbox::CapabilityChecker;

use super::{example_plugin_exists, example_plugin_path, noop_host_api};

// ── CapabilityChecker standalone ────────────────────────────────────────────

#[test]
fn checker_grants_declared_capabilities() {
    let caps = [
        PluginCapability::RegisterCommands,
        PluginCapability::HookEvents,
    ];
    let checker = CapabilityChecker::new("test-plugin", &caps);

    assert!(checker.check(PluginCapability::RegisterCommands));
    assert!(checker.check(PluginCapability::HookEvents));
    assert!(!checker.check(PluginCapability::ReadConfig));
    assert!(!checker.check(PluginCapability::SendMessages));
    assert!(!checker.check(PluginCapability::AccessNetwork));
}

#[test]
fn checker_require_denied_returns_permission_denied() {
    let checker = CapabilityChecker::new("restricted-plugin", &[]);

    let err = checker
        .require(PluginCapability::SendMessages)
        .expect_err("require without capability should fail");

    let msg = err.to_string();
    assert!(msg.contains("restricted-plugin"));
    assert!(msg.contains("send messages"));
}

#[test]
fn checker_with_all_capabilities_passes_all() {
    let all_caps = [
        PluginCapability::ReadConfig,
        PluginCapability::RegisterCommands,
        PluginCapability::HookEvents,
        PluginCapability::SendMessages,
        PluginCapability::AccessNetwork,
    ];
    let checker = CapabilityChecker::new("full-access", &all_caps);
    assert_eq!(checker.capability_count(), 5);
    for cap in &all_caps {
        assert!(checker.require(*cap).is_ok());
    }
}

#[test]
fn checker_deduplicates_capabilities() {
    let caps = [
        PluginCapability::ReadConfig,
        PluginCapability::ReadConfig,
        PluginCapability::ReadConfig,
    ];
    let checker = CapabilityChecker::new("dup", &caps);
    assert_eq!(checker.capability_count(), 1);
}

// ── Manager capability enforcement ──────────────────────────────────────────

#[test]
fn manager_denies_register_command_without_capability() {
    // The auto-respond plugin only declares HookEvents and ReadConfig,
    // NOT RegisterCommands. The manager should deny register_command.
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("auto_respond_plugin"), &host_api)
        .unwrap();

    let err = manager
        .register_command(&name, "test", "should be denied")
        .expect_err("register_command without capability should fail");
    assert!(matches!(err, ManagerError::PermissionDenied { .. }));
}

#[test]
fn manager_allows_register_command_with_capability() {
    // The hello plugin declares RegisterCommands and HookEvents.
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .unwrap();

    manager
        .register_command(&name, "test", "allowed")
        .expect("register_command with capability should succeed");
}

#[test]
fn manager_denies_hook_event_without_capability() {
    // We need a plugin that does NOT declare HookEvents.
    // All three example plugins declare HookEvents, so we test this via
    // the check_send_capability path (SendMessages) which no example plugin
    // declares.
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .unwrap();

    let err = manager
        .check_send_capability(&name)
        .expect_err("send capability should be denied");
    assert!(matches!(err, ManagerError::PermissionDenied { .. }));
}

#[test]
fn manager_allows_hook_event_with_capability() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .unwrap();

    // hello-plugin declares HookEvents.
    manager
        .hook_event(&name, PluginEventType::Connected)
        .expect("hook_event with capability should succeed");
}

// ── Plugin-specific capability profiles ─────────────────────────────────────

#[test]
fn hello_plugin_has_register_commands_and_hook_events() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .unwrap();

    let plugin = manager.get_plugin(&name).unwrap();
    assert!(plugin.capabilities().check(PluginCapability::RegisterCommands));
    assert!(plugin.capabilities().check(PluginCapability::HookEvents));
    assert!(!plugin.capabilities().check(PluginCapability::ReadConfig));
    assert!(!plugin.capabilities().check(PluginCapability::SendMessages));
}

#[test]
fn auto_respond_plugin_has_hook_events_and_read_config() {
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("auto_respond_plugin"), &host_api)
        .unwrap();

    let plugin = manager.get_plugin(&name).unwrap();
    assert!(plugin.capabilities().check(PluginCapability::HookEvents));
    assert!(plugin.capabilities().check(PluginCapability::ReadConfig));
    assert!(!plugin.capabilities().check(PluginCapability::RegisterCommands));
}

#[test]
fn logger_plugin_has_hook_events_and_read_config() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("logger_plugin"), &host_api)
        .unwrap();

    let plugin = manager.get_plugin(&name).unwrap();
    assert!(plugin.capabilities().check(PluginCapability::HookEvents));
    assert!(plugin.capabilities().check(PluginCapability::ReadConfig));
    assert!(!plugin.capabilities().check(PluginCapability::RegisterCommands));
}
