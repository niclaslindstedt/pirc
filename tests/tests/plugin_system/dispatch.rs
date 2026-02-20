//! Event dispatch and command routing integration tests.
//!
//! Tests cover event fan-out to subscribed plugins, command dispatch routing,
//! and interaction between the registry and manager dispatch methods.

use pirc_plugin::ffi::PluginEventType;
use pirc_plugin::manager::PluginManager;

use super::{
    example_plugin_exists, example_plugin_path, load_plugin_into_manager,
    noop_host_api,
};

// ── Event dispatch with no subscribers ──────────────────────────────────────

#[test]
fn dispatch_event_no_subscribers_returns_zero() {
    let manager = PluginManager::new();
    let count = manager.dispatch_event(PluginEventType::Connected, "", "");
    assert_eq!(count, 0);
}

#[test]
fn dispatch_event_no_subscribers_for_type() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);
    manager.enable_plugin(&name).unwrap();

    // hello-plugin only hooks CommandExecuted, not Connected.
    let count = manager.dispatch_event(PluginEventType::Connected, "", "");
    assert_eq!(count, 0);
}

// ── Command dispatch ────────────────────────────────────────────────────────

#[test]
fn dispatch_command_no_match_returns_false() {
    let manager = PluginManager::new();
    let handled = manager
        .dispatch_command("nonexistent", "")
        .expect("dispatch should not fail");
    assert!(!handled);
}

#[test]
fn dispatch_command_to_disabled_plugin_returns_false() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // Register the "hello" command manually (since noop_host_api doesn't
    // actually forward the registration through the manager).
    manager
        .register_command(&name, "hello", "Say hello")
        .unwrap();
    manager.hook_event(&name, PluginEventType::CommandExecuted).unwrap();

    // Plugin is still in Loaded state (not Enabled) — command should not dispatch.
    let handled = manager.dispatch_command("hello", "").unwrap();
    assert!(!handled);
}

#[test]
fn dispatch_command_to_enabled_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "hello", "Say hello")
        .unwrap();
    manager.hook_event(&name, PluginEventType::CommandExecuted).unwrap();
    manager.enable_plugin(&name).unwrap();

    let handled = manager
        .dispatch_command("hello", "world")
        .expect("dispatch should succeed");
    assert!(handled, "hello command should be handled by enabled plugin");
}

// ── Event dispatch to enabled plugins ───────────────────────────────────────

#[test]
fn dispatch_event_to_enabled_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager.hook_event(&name, PluginEventType::CommandExecuted).unwrap();
    manager.enable_plugin(&name).unwrap();

    let count =
        manager.dispatch_event(PluginEventType::CommandExecuted, "hello", "");
    assert_eq!(count, 1);
}

#[test]
fn dispatch_event_skips_disabled_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager.hook_event(&name, PluginEventType::CommandExecuted).unwrap();

    // Plugin is in Loaded state — should be skipped.
    let count =
        manager.dispatch_event(PluginEventType::CommandExecuted, "hello", "");
    assert_eq!(count, 0);
}

// ── Multi-plugin event fan-out ──────────────────────────────────────────────

#[test]
fn dispatch_event_fans_out_to_multiple_plugins() {
    let available: Vec<&str> = ["hello_plugin", "logger_plugin"]
        .iter()
        .copied()
        .filter(|n| example_plugin_exists(n))
        .collect();

    if available.len() < 2 {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();

    let mut names = Vec::new();
    for lib_name in &available {
        let name = manager
            .load_plugin(&example_plugin_path(lib_name), &host_api)
            .unwrap();
        names.push(name);
    }

    // Hook the same event type for both plugins.
    for name in &names {
        manager
            .hook_event(name, PluginEventType::MessageReceived)
            .unwrap();
        manager.enable_plugin(name).unwrap();
    }

    let count =
        manager.dispatch_event(PluginEventType::MessageReceived, "hello", "alice");
    assert_eq!(
        count,
        names.len(),
        "event should be delivered to all enabled subscribers"
    );
}

// ── Command case-insensitive dispatch ───────────────────────────────────────

#[test]
fn dispatch_command_case_insensitive() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .register_command(&name, "hello", "Say hello")
        .unwrap();
    manager.hook_event(&name, PluginEventType::CommandExecuted).unwrap();
    manager.enable_plugin(&name).unwrap();

    // Command lookup is case-insensitive.
    let handled = manager.dispatch_command("HELLO", "").unwrap();
    assert!(handled);

    let handled = manager.dispatch_command("Hello", "").unwrap();
    assert!(handled);
}
