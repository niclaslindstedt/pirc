//! Command and event registry integration tests.
//!
//! Tests the interaction between the registries and the plugin manager,
//! including multi-plugin command conflicts and event subscription fan-out.

use pirc_plugin::ffi::PluginEventType;
use pirc_plugin::manager::ManagerError;

use super::{
    example_plugin_exists, example_plugin_path, load_plugin_into_manager,
    noop_host_api,
};

// ── Command registration through manager ────────────────────────────────────

#[test]
fn register_command_and_lookup() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "greet", "Greet the user")
        .expect("registration should succeed");

    let entry = manager
        .command_registry()
        .lookup("greet")
        .expect("command should be found");
    assert_eq!(entry.plugin_name, name);
    assert_eq!(entry.description, "Greet the user");
}

#[test]
fn register_command_case_insensitive_lookup() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "MyCommand", "Test cmd")
        .unwrap();

    assert!(manager.command_registry().lookup("mycommand").is_some());
    assert!(manager.command_registry().lookup("MYCOMMAND").is_some());
    assert!(manager.command_registry().lookup("MyCommand").is_some());
}

#[test]
fn command_conflict_detected_at_registry_level() {
    // Test command conflict directly through the registry since the manager
    // also enforces capability checks. The registry is the canonical source
    // of truth for command ownership.
    use pirc_plugin::registry::CommandRegistry;

    let mut reg = CommandRegistry::new();
    reg.register("plugin-a", "shared", "First registrant")
        .expect("first registration should succeed");

    let err = reg
        .register("plugin-b", "shared", "Second registrant")
        .expect_err("duplicate registration should fail");
    let msg = err.to_string();
    assert!(msg.contains("shared"), "error should mention the command");
    assert!(msg.contains("plugin-a"), "error should mention the owner");
}

#[test]
fn same_plugin_reregisters_command_idempotently() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // First registration.
    manager
        .register_command(&name, "mycmd", "My command")
        .expect("first registration should succeed");

    // Same plugin re-registers the same command — should be idempotent.
    manager
        .register_command(&name, "mycmd", "My command updated")
        .expect("re-registration by same plugin should succeed");

    assert_eq!(manager.command_registry().len(), 1);
}

// ── Command unregistration ──────────────────────────────────────────────────

#[test]
fn unregister_command_removes_from_registry() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "temp", "Temporary command")
        .unwrap();
    assert!(manager.command_registry().lookup("temp").is_some());

    let removed = manager.unregister_command(&name, "temp").unwrap();
    assert!(removed);
    assert!(manager.command_registry().lookup("temp").is_none());
}

// ── Event subscription through manager ──────────────────────────────────────

#[test]
fn hook_event_and_check_subscribers() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .hook_event(&name, PluginEventType::UserJoined)
        .expect("hook should succeed");

    let subs = manager
        .event_registry()
        .subscribers(PluginEventType::UserJoined);
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0], name);
}

#[test]
fn hook_multiple_event_types() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("logger_plugin"), &host_api);

    let event_types = [
        PluginEventType::MessageReceived,
        PluginEventType::UserJoined,
        PluginEventType::UserParted,
        PluginEventType::UserQuit,
        PluginEventType::NickChanged,
    ];

    for et in &event_types {
        manager.hook_event(&name, *et).unwrap();
    }

    for et in &event_types {
        assert!(
            manager.event_registry().has_subscribers(*et),
            "event type {et:?} should have subscribers"
        );
    }

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.hooked_events().len(), event_types.len());
}

#[test]
fn unhook_event_removes_subscription() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .hook_event(&name, PluginEventType::Connected)
        .unwrap();
    assert!(
        manager
            .event_registry()
            .has_subscribers(PluginEventType::Connected)
    );

    let removed = manager
        .unhook_event(&name, PluginEventType::Connected)
        .unwrap();
    assert!(removed);
    assert!(
        !manager
            .event_registry()
            .has_subscribers(PluginEventType::Connected)
    );
}

// ── Multiple plugins subscribe to same event ────────────────────────────────

#[test]
fn multiple_plugins_subscribe_same_event() {
    let plugins: Vec<&str> = ["hello_plugin", "logger_plugin"]
        .iter()
        .copied()
        .filter(|n| example_plugin_exists(n))
        .collect();

    if plugins.len() < 2 {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = pirc_plugin::manager::PluginManager::new();

    let mut names = Vec::new();
    for lib_name in &plugins {
        let name = manager
            .load_plugin(&example_plugin_path(lib_name), &host_api)
            .unwrap();
        names.push(name);
    }

    for name in &names {
        manager
            .hook_event(name, PluginEventType::MessageReceived)
            .unwrap();
    }

    let subs = manager
        .event_registry()
        .subscribers(PluginEventType::MessageReceived);
    assert_eq!(subs.len(), names.len());

    // Subscribers should be sorted deterministically.
    let mut expected = subs.clone();
    expected.sort();
    assert_eq!(subs, expected);
}

// ── Unload cleans up registries ─────────────────────────────────────────────

#[test]
fn unload_cleans_up_commands_and_events() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "mycmd", "Test cmd")
        .unwrap();
    manager
        .hook_event(&name, PluginEventType::Connected)
        .unwrap();

    assert!(manager.command_registry().lookup("mycmd").is_some());
    assert!(
        manager
            .event_registry()
            .has_subscribers(PluginEventType::Connected)
    );

    // Unload should clean up both registries.
    manager.unload_plugin(&name).unwrap();

    assert!(manager.command_registry().lookup("mycmd").is_none());
    assert!(
        !manager
            .event_registry()
            .has_subscribers(PluginEventType::Connected)
    );
}

// ── Register command for nonexistent plugin ─────────────────────────────────

#[test]
fn register_command_nonexistent_plugin_returns_not_found() {
    let mut manager = pirc_plugin::manager::PluginManager::new();
    let err = manager
        .register_command("ghost", "cmd", "desc")
        .expect_err("should fail for nonexistent plugin");
    assert!(matches!(err, ManagerError::NotFound(_)));
}

#[test]
fn hook_event_nonexistent_plugin_returns_not_found() {
    let mut manager = pirc_plugin::manager::PluginManager::new();
    let err = manager
        .hook_event("ghost", PluginEventType::Connected)
        .expect_err("should fail for nonexistent plugin");
    assert!(matches!(err, ManagerError::NotFound(_)));
}
