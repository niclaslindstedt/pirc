//! Plugin lifecycle integration tests.
//!
//! Tests cover the full lifecycle state machine: Load -> Enable -> Disable ->
//! Unload, including invalid state transitions and hot-reload.

use pirc_plugin::manager::{ManagerError, PluginManager, PluginState};

use super::{
    example_plugin_exists, example_plugin_path, load_plugin_into_manager,
    noop_host_api,
};

// ── Full lifecycle: load -> enable -> disable -> unload ─────────────────────

#[test]
fn full_lifecycle_hello_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // After load: state is Loaded.
    let plugin = manager.get_plugin(&name).expect("plugin should exist");
    assert_eq!(plugin.state(), PluginState::Loaded);

    // Enable the plugin.
    manager.enable_plugin(&name).expect("enable should succeed");
    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.state(), PluginState::Enabled);

    // Disable the plugin.
    manager
        .disable_plugin(&name)
        .expect("disable should succeed");
    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.state(), PluginState::Disabled);

    // Unload the plugin.
    manager
        .unload_plugin(&name)
        .expect("unload should succeed");
    assert!(!manager.has_plugin(&name));
    assert_eq!(manager.plugin_count(), 0);
}

// ── Enable/disable cycle ────────────────────────────────────────────────────

#[test]
fn enable_disable_cycle_multiple_times() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    for _ in 0..3 {
        manager.enable_plugin(&name).expect("enable should succeed");
        assert_eq!(
            manager.get_plugin(&name).unwrap().state(),
            PluginState::Enabled
        );

        manager
            .disable_plugin(&name)
            .expect("disable should succeed");
        assert_eq!(
            manager.get_plugin(&name).unwrap().state(),
            PluginState::Disabled
        );
    }
}

// ── Invalid state transitions ───────────────────────────────────────────────

#[test]
fn enable_already_enabled_returns_error() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager.enable_plugin(&name).expect("first enable");
    let err = manager
        .enable_plugin(&name)
        .expect_err("double enable should fail");
    assert!(matches!(err, ManagerError::InvalidState { .. }));
}

#[test]
fn disable_loaded_plugin_returns_error() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // Plugin is in Loaded state — cannot disable without enabling first.
    let err = manager
        .disable_plugin(&name)
        .expect_err("disable without enable should fail");
    assert!(matches!(err, ManagerError::InvalidState { .. }));
}

#[test]
fn unload_enabled_plugin_returns_error() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager.enable_plugin(&name).expect("enable");
    let err = manager
        .unload_plugin(&name)
        .expect_err("unload while enabled should fail");
    assert!(matches!(err, ManagerError::InvalidState { .. }));
}

// ── Operations on nonexistent plugins ───────────────────────────────────────

#[test]
fn enable_nonexistent_plugin_returns_not_found() {
    let mut manager = PluginManager::new();
    let err = manager
        .enable_plugin("ghost")
        .expect_err("enable ghost should fail");
    assert!(matches!(err, ManagerError::NotFound(_)));
}

#[test]
fn disable_nonexistent_plugin_returns_not_found() {
    let mut manager = PluginManager::new();
    let err = manager
        .disable_plugin("ghost")
        .expect_err("disable ghost should fail");
    assert!(matches!(err, ManagerError::NotFound(_)));
}

#[test]
fn unload_nonexistent_plugin_returns_not_found() {
    let mut manager = PluginManager::new();
    let err = manager
        .unload_plugin("ghost")
        .expect_err("unload ghost should fail");
    assert!(matches!(err, ManagerError::NotFound(_)));
}

// ── Plugin info after lifecycle stages ──────────────────────────────────────

#[test]
fn plugin_info_reflects_state_transitions() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    let info = manager.get_plugin(&name).unwrap().info();
    assert_eq!(info.name, "hello-plugin");
    assert_eq!(info.state, PluginState::Loaded);
    assert!(!info.version.is_empty());

    manager.enable_plugin(&name).unwrap();
    let info = manager.get_plugin(&name).unwrap().info();
    assert_eq!(info.state, PluginState::Enabled);

    manager.disable_plugin(&name).unwrap();
    let info = manager.get_plugin(&name).unwrap().info();
    assert_eq!(info.state, PluginState::Disabled);
}

// ── list_plugins ordering ───────────────────────────────────────────────────

#[test]
fn list_plugins_sorted_by_name() {
    let plugins = ["hello_plugin", "auto_respond_plugin", "logger_plugin"];
    let available: Vec<_> = plugins
        .iter()
        .filter(|n| example_plugin_exists(n))
        .collect();

    if available.len() < 2 {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    for name in &available {
        manager
            .load_plugin(&example_plugin_path(name), &host_api)
            .unwrap();
    }

    let infos = manager.list_plugins();
    let names: Vec<_> = infos.iter().map(|i| &i.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "list_plugins should be sorted by name");
}

// ── Unload then reload ──────────────────────────────────────────────────────

#[test]
fn unload_then_reload_same_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // Unload the plugin (from Loaded state — allowed).
    manager.unload_plugin(&name).expect("unload should succeed");
    assert_eq!(manager.plugin_count(), 0);

    // Reload it.
    let name2 = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .expect("reload should succeed");
    assert_eq!(name2, "hello-plugin");
    assert_eq!(manager.plugin_count(), 1);
}

// ── Hot-reload ──────────────────────────────────────────────────────────────

#[test]
fn reload_plugin_from_loaded_state() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // Reload from Loaded state (not Enabled).
    manager
        .reload_plugin(&name, &host_api)
        .expect("reload should succeed");
    assert!(manager.has_plugin("hello-plugin"));
}

#[test]
fn reload_enabled_plugin_returns_error() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager.enable_plugin(&name).unwrap();

    let err = manager
        .reload_plugin(&name, &host_api)
        .expect_err("reload while enabled should fail");
    assert!(matches!(err, ManagerError::InvalidState { .. }));
}
