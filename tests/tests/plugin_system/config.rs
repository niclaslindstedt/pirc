//! Plugin configuration integration tests.
//!
//! Tests cover TOML configuration loading, per-plugin settings, and the
//! interaction between configuration and the plugin manager.

use pirc_plugin::config::{load_plugin_config, parse_plugin_config, PluginConfig};
use pirc_plugin::manager::PluginManager;

use super::{
    config_host_api, example_plugin_exists, example_plugin_path,
};

// ── parse_plugin_config ─────────────────────────────────────────────────────

#[test]
fn parse_full_config_with_all_types() {
    let toml = r#"
[plugin]
enabled = true

[settings]
greeting = "hello"
count = 42
debug = false
ratio = 3.14
"#;
    let config = parse_plugin_config(toml).expect("should parse");
    assert!(config.enabled);
    assert_eq!(config.settings_count(), 4);
    assert_eq!(config.get_setting("greeting").unwrap(), "hello");
    assert_eq!(config.get_setting("count").unwrap(), "42");
    assert_eq!(config.get_setting("debug").unwrap(), "false");
    assert_eq!(config.get_setting("ratio").unwrap(), "3.14");
}

#[test]
fn parse_disabled_plugin_config() {
    let toml = r#"
[plugin]
enabled = false

[settings]
server = "irc.example.com"
"#;
    let config = parse_plugin_config(toml).unwrap();
    assert!(!config.enabled);
    assert_eq!(config.get_setting("server").unwrap(), "irc.example.com");
}

#[test]
fn parse_empty_config_returns_defaults() {
    let config = parse_plugin_config("").unwrap();
    assert!(config.enabled);
    assert_eq!(config.settings_count(), 0);
}

#[test]
fn parse_settings_only_defaults_enabled() {
    let toml = r#"
[settings]
key = "value"
"#;
    let config = parse_plugin_config(toml).unwrap();
    assert!(config.enabled);
    assert_eq!(config.get_setting("key").unwrap(), "value");
}

#[test]
fn parse_malformed_toml_returns_error() {
    let result = parse_plugin_config("not valid [[ toml {{");
    assert!(result.is_err());
}

// ── load_plugin_config from filesystem ──────────────────────────────────────

#[test]
fn load_missing_config_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = load_plugin_config(dir.path(), "nonexistent-plugin");
    assert!(config.enabled);
    assert_eq!(config.settings_count(), 0);
}

#[test]
fn load_valid_config_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let toml_content = r#"
[plugin]
enabled = false

[settings]
port = 6667
server = "irc.example.com"
"#;
    std::fs::write(dir.path().join("my-plugin.toml"), toml_content).unwrap();

    let config = load_plugin_config(dir.path(), "my-plugin");
    assert!(!config.enabled);
    assert_eq!(config.get_setting("port").unwrap(), "6667");
    assert_eq!(config.get_setting("server").unwrap(), "irc.example.com");
}

#[test]
fn load_malformed_config_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.toml"), "not valid [[ toml {{").unwrap();

    let config = load_plugin_config(dir.path(), "bad");
    assert!(config.enabled);
    assert_eq!(config.settings_count(), 0);
}

// ── PluginConfig default ────────────────────────────────────────────────────

#[test]
fn default_config_is_enabled_with_no_settings() {
    let config = PluginConfig::default();
    assert!(config.enabled);
    assert_eq!(config.settings_count(), 0);
    assert!(config.get_setting("anything").is_none());
}

// ── Plugin loaded with config ───────────────────────────────────────────────

#[test]
fn load_plugin_with_custom_config() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = config_host_api();
    let config = parse_plugin_config(
        r#"
[plugin]
enabled = true

[settings]
greeting = "Custom Hello"
"#,
    )
    .unwrap();

    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin_with_config(
            &example_plugin_path("hello_plugin"),
            &host_api,
            config,
        )
        .expect("plugin should load with config");

    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(
        plugin.config().get_setting("greeting").unwrap(),
        "Custom Hello"
    );
}

// ── Manager config lookup ───────────────────────────────────────────────────

#[test]
fn manager_get_plugin_config_value_with_read_config_capability() {
    // The auto-respond plugin declares ReadConfig, so it should be allowed
    // to access its config values through the manager.
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let host_api = config_host_api();
    let config = parse_plugin_config("[settings]\nfoo = \"bar\"").unwrap();

    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin_with_config(
            &example_plugin_path("auto_respond_plugin"),
            &host_api,
            config,
        )
        .unwrap();

    assert_eq!(
        manager.get_plugin_config_value(&name, "foo").unwrap(),
        "bar"
    );
    assert!(manager.get_plugin_config_value(&name, "missing").is_none());
    assert!(
        manager
            .get_plugin_config_value("nonexistent", "foo")
            .is_none()
    );
}

#[test]
fn manager_get_plugin_config_value_denied_without_capability() {
    // The hello-plugin does NOT declare ReadConfig, so config access
    // through the manager should be denied.
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = config_host_api();
    let config = parse_plugin_config("[settings]\nfoo = \"bar\"").unwrap();

    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin_with_config(
            &example_plugin_path("hello_plugin"),
            &host_api,
            config,
        )
        .unwrap();

    // Config access should return None due to missing ReadConfig capability.
    assert!(
        manager.get_plugin_config_value(&name, "foo").is_none(),
        "plugin without ReadConfig capability should not access config"
    );

    // But the config is still accessible directly for internal use.
    let plugin = manager.get_plugin(&name).unwrap();
    assert_eq!(plugin.config().get_setting("foo").unwrap(), "bar");
}

// ── Directory scan with config files ────────────────────────────────────────

#[test]
fn load_plugins_dir_reads_config_files() {
    // This test validates that load_plugins_dir picks up .toml files in the
    // same directory as the plugin libraries. We create a temp directory with
    // a config file but no actual libraries — the config loading should not
    // crash even when plugin loading fails.
    let dir = tempfile::tempdir().unwrap();
    let toml = r#"
[plugin]
enabled = false

[settings]
log_level = "debug"
"#;
    std::fs::write(dir.path().join("test-plugin.toml"), toml).unwrap();

    let host_api = config_host_api();
    let mut manager = PluginManager::new();
    let loaded = manager.load_plugins_dir(dir.path(), &host_api);
    // No actual plugins to load, but the scan should complete without error.
    assert!(loaded.is_empty());
}
