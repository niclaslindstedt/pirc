//! Plugin loading integration tests.
//!
//! Tests cover loading plugins from dynamic libraries, error handling for
//! invalid paths, symbol lookup failures, and directory scanning.

use std::path::Path;

use pirc_plugin::loader::{LoadError, PluginLoader};
use pirc_plugin::manager::{ManagerError, PluginManager};

use super::{example_plugin_exists, example_plugin_path, noop_host_api};

// ── PluginLoader basic operations ───────────────────────────────────────────

#[test]
fn loader_library_extension_matches_platform() {
    let ext = PluginLoader::library_extension();
    if cfg!(target_os = "macos") {
        assert_eq!(ext, "dylib");
    } else {
        assert_eq!(ext, "so");
    }
}

#[test]
fn load_nonexistent_file_returns_library_load_error() {
    let loader = PluginLoader::new();
    let result = loader.load("/tmp/pirc_test_nonexistent_plugin.dylib");
    let err = result.expect_err("loading nonexistent file should fail");
    assert!(
        matches!(err, LoadError::LibraryLoadError { .. }),
        "expected LibraryLoadError, got: {err}"
    );
}

#[test]
fn load_non_plugin_library_returns_symbol_not_found() {
    // Load a system library that definitely does NOT have pirc_plugin_init.
    let system_lib = if cfg!(target_os = "macos") {
        "/usr/lib/libSystem.B.dylib"
    } else {
        "/lib/x86_64-linux-gnu/libc.so.6"
    };

    if !Path::new(system_lib).exists() {
        return; // Skip on platforms where the system lib isn't at this path.
    }

    let loader = PluginLoader::new();
    let err = loader
        .load(system_lib)
        .expect_err("loading non-plugin library should fail");
    assert!(
        matches!(err, LoadError::SymbolNotFound { .. }),
        "expected SymbolNotFound, got: {err}"
    );
}

// ── PluginLoader loads real example plugins ─────────────────────────────────

#[test]
fn load_hello_plugin_from_dylib() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let loader = PluginLoader::new();
    let loaded = loader
        .load(example_plugin_path("hello_plugin"))
        .expect("hello-plugin should load");

    assert_eq!(loaded.path(), example_plugin_path("hello_plugin"));

    #[allow(unsafe_code)]
    let name = unsafe { loaded.plugin_name() };
    assert_eq!(name, "hello-plugin");

    #[allow(unsafe_code)]
    let version = unsafe { loaded.plugin_version() };
    assert!(!version.is_empty());
}

#[test]
fn load_auto_respond_plugin_from_dylib() {
    if !example_plugin_exists("auto_respond_plugin") {
        return;
    }

    let loader = PluginLoader::new();
    let loaded = loader
        .load(example_plugin_path("auto_respond_plugin"))
        .expect("auto-respond-plugin should load");

    #[allow(unsafe_code)]
    let name = unsafe { loaded.plugin_name() };
    assert_eq!(name, "auto-respond");
}

#[test]
fn load_logger_plugin_from_dylib() {
    if !example_plugin_exists("logger_plugin") {
        return;
    }

    let loader = PluginLoader::new();
    let loaded = loader
        .load(example_plugin_path("logger_plugin"))
        .expect("logger-plugin should load");

    #[allow(unsafe_code)]
    let name = unsafe { loaded.plugin_name() };
    assert_eq!(name, "channel-logger");
}

// ── PluginManager load operations ───────────────────────────────────────────

#[test]
fn manager_load_plugin_invalid_path_returns_error() {
    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let err = manager
        .load_plugin(Path::new("/nonexistent/plugin.dylib"), &host_api)
        .expect_err("loading from invalid path should fail");
    assert!(matches!(err, ManagerError::LoadFailed(_)));
}

#[test]
fn manager_load_hello_plugin() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .expect("hello-plugin should load via manager");

    assert_eq!(name, "hello-plugin");
    assert!(manager.has_plugin("hello-plugin"));
    assert_eq!(manager.plugin_count(), 1);
}

#[test]
fn manager_load_duplicate_plugin_returns_error() {
    if !example_plugin_exists("hello_plugin") {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();

    manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .expect("first load should succeed");

    let err = manager
        .load_plugin(&example_plugin_path("hello_plugin"), &host_api)
        .expect_err("duplicate load should fail");
    assert!(matches!(err, ManagerError::DuplicateName(_)));
}

// ── Directory scanning ──────────────────────────────────────────────────────

#[test]
fn load_plugins_dir_nonexistent_returns_empty() {
    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let loaded = manager.load_plugins_dir(Path::new("/nonexistent/plugins"), &host_api);
    assert!(loaded.is_empty());
}

#[test]
fn load_plugins_dir_empty_directory() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let loaded = manager.load_plugins_dir(dir.path(), &host_api);
    assert!(loaded.is_empty());
    assert_eq!(manager.plugin_count(), 0);
}

#[test]
fn load_plugins_dir_skips_non_library_files() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    // Create some non-library files.
    std::fs::write(dir.path().join("readme.txt"), "not a plugin").unwrap();
    std::fs::write(dir.path().join("config.toml"), "[plugin]\nenabled = true").unwrap();

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();
    let loaded = manager.load_plugins_dir(dir.path(), &host_api);
    assert!(loaded.is_empty());
}

// ── Multiple plugins loaded simultaneously ──────────────────────────────────

#[test]
fn load_multiple_example_plugins() {
    let plugins = ["hello_plugin", "auto_respond_plugin", "logger_plugin"];
    let available: Vec<_> = plugins
        .iter()
        .filter(|name| example_plugin_exists(name))
        .collect();

    if available.is_empty() {
        return;
    }

    let host_api = noop_host_api();
    let mut manager = PluginManager::new();

    for name in &available {
        manager
            .load_plugin(&example_plugin_path(name), &host_api)
            .unwrap_or_else(|e| panic!("{name} should load: {e}"));
    }

    assert_eq!(manager.plugin_count(), available.len());

    // Verify each plugin is accessible by its registered name.
    let infos = manager.list_plugins();
    assert_eq!(infos.len(), available.len());
}
