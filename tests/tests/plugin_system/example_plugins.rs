//! Example plugin verification integration tests.
//!
//! These tests load the compiled example plugins (hello-plugin,
//! auto-respond-plugin, logger-plugin) as real dynamic libraries and verify
//! their behaviour through the plugin system's public API.

use pirc_plugin::ffi::PluginEventType;
use pirc_plugin::manager::{PluginManager, PluginState};

use super::{
    config_host_api, example_plugin_exists, example_plugin_path,
    load_plugin_into_manager, noop_host_api,
};

// ── hello-plugin verification ───────────────────────────────────────────────

#[test]
fn hello_plugin_loads_and_has_correct_metadata() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.name(), "hello-plugin");
    assert!(!plugin.version().is_empty());
    assert_eq!(plugin.state(), PluginState::Loaded);
}

#[test]
fn hello_plugin_full_lifecycle() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    // Register command and hook events (simulating what init does via host API).
    manager
        .register_command(&name, "hello", "Say hello")
        .unwrap();
    manager
        .hook_event(&name, PluginEventType::CommandExecuted)
        .unwrap();

    // Enable and dispatch a command.
    manager.enable_plugin(&name).unwrap();
    let handled = manager.dispatch_command("hello", "world").unwrap();
    assert!(handled, "hello command should be dispatched");

    // Disable and unload.
    manager.disable_plugin(&name).unwrap();
    manager.unload_plugin(&name).unwrap();
    assert!(!manager.has_plugin("hello-plugin"));
}

#[test]
fn hello_plugin_responds_to_command_event() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("hello_plugin"), &host_api);

    manager
        .hook_event(&name, PluginEventType::CommandExecuted)
        .unwrap();
    manager.enable_plugin(&name).unwrap();

    // Dispatch a CommandExecuted event with data="hello".
    let count = manager.dispatch_event(
        PluginEventType::CommandExecuted,
        "hello",
        "arg1 arg2",
    );
    assert_eq!(count, 1, "hello-plugin should receive the event");
}

// ── auto-respond-plugin verification ────────────────────────────────────────

#[test]
fn auto_respond_plugin_loads_and_has_correct_metadata() {
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (manager, name) = load_plugin_into_manager(
        &example_plugin_path("auto_respond_plugin"),
        &host_api,
    );

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.name(), "auto-respond");
    assert!(!plugin.version().is_empty());
}

#[test]
fn auto_respond_plugin_hooks_message_received() {
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) = load_plugin_into_manager(
        &example_plugin_path("auto_respond_plugin"),
        &host_api,
    );

    manager
        .hook_event(&name, PluginEventType::MessageReceived)
        .unwrap();
    manager.enable_plugin(&name).unwrap();

    // Dispatch a greeting message.
    let count = manager.dispatch_event(
        PluginEventType::MessageReceived,
        "hello everyone!",
        "alice",
    );
    assert_eq!(
        count, 1,
        "auto-respond should receive the message event"
    );
}

#[test]
fn auto_respond_plugin_with_config() {
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = config_host_api();
    let config = pirc_plugin::config::parse_plugin_config(
        r#"
[settings]
greeting = "Howdy!"
"#,
    )
    .unwrap();

    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin_with_config(
            &example_plugin_path("auto_respond_plugin"),
            &host_api,
            config,
        )
        .unwrap();

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(
        plugin.config().get_setting("greeting").unwrap(),
        "Howdy!"
    );
}

// ── logger-plugin verification ──────────────────────────────────────────────

#[test]
fn logger_plugin_loads_and_has_correct_metadata() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (manager, name) =
        load_plugin_into_manager(&example_plugin_path("logger_plugin"), &host_api);

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.name(), "channel-logger");
    assert!(!plugin.version().is_empty());
}

#[test]
fn logger_plugin_hooks_multiple_events() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let (mut manager, name) =
        load_plugin_into_manager(&example_plugin_path("logger_plugin"), &host_api);

    let events = [
        PluginEventType::MessageReceived,
        PluginEventType::UserJoined,
        PluginEventType::UserParted,
        PluginEventType::UserQuit,
        PluginEventType::NickChanged,
    ];

    for et in &events {
        manager.hook_event(&name, *et).unwrap();
    }
    manager.enable_plugin(&name).unwrap();

    // Dispatch each event type and verify it's received.
    for et in &events {
        let count = manager.dispatch_event(*et, "test-data", "test-source");
        assert_eq!(count, 1, "logger should receive {et:?} event");
    }
}

#[test]
fn logger_plugin_with_custom_log_dir() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let host_api = config_host_api();
    let config = pirc_plugin::config::parse_plugin_config(
        r#"
[settings]
log_dir = "/tmp/pirc-custom-logs"
"#,
    )
    .unwrap();

    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin_with_config(
            &example_plugin_path("logger_plugin"),
            &host_api,
            config,
        )
        .unwrap();

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(
        plugin.config().get_setting("log_dir").unwrap(),
        "/tmp/pirc-custom-logs"
    );
}

// ── All three plugins loaded simultaneously ─────────────────────────────────

#[test]
fn all_example_plugins_loaded_together() {
    let plugin_libs = [
        ("hello_plugin", "hello-plugin"),
        ("auto_respond_plugin", "auto-respond"),
        ("logger_plugin", "channel-logger"),
    ];

    let available: Vec<_> = plugin_libs
        .iter()
        .filter(|(lib, _)| example_plugin_exists(lib))
        .collect();

    if available.is_empty() {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();

    for (lib_name, expected_name) in &available {
        let name = manager
            .load_plugin(&example_plugin_path(lib_name), &host_api)
            .unwrap();
        assert_eq!(&name, *expected_name);
    }

    assert_eq!(manager.plugin_count(), available.len());

    // Enable all.
    let infos = manager.list_plugins();
    for info in &infos {
        manager.enable_plugin(&info.name).unwrap();
    }

    // All should be enabled.
    for info_name in infos.iter().map(|i| &i.name) {
        assert_eq!(
            manager.get_plugin(info_name).unwrap().state(),
            PluginState::Enabled
        );
    }
}

// ── Plugin version is non-empty ─────────────────────────────────────────────

#[test]
fn all_example_plugins_have_valid_versions() {
    let plugins = ["hello_plugin", "auto_respond_plugin", "logger_plugin"];

    for lib_name in &plugins {
        if !example_plugin_exists(lib_name) {
            continue;
        }

        let host_api = noop_host_api();
        let mut manager = PluginManager::new();
        let name = manager
            .load_plugin(&example_plugin_path(lib_name), &host_api)
            .unwrap();

        let version = manager.get_plugin(&name).unwrap().version().to_owned();
        assert!(
            !version.is_empty(),
            "{lib_name} should have a non-empty version"
        );
        // Version should look like semver (at least contain a dot).
        assert!(
            version.contains('.'),
            "{lib_name} version '{version}' should contain a dot (semver)"
        );
    }
}
