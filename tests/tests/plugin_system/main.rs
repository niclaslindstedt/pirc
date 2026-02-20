//! Plugin system integration tests.
//!
//! Tests cover the full plugin lifecycle: loading from dynamic libraries,
//! initialisation, event dispatch, command registration, configuration
//! loading, error handling, and verification of the example plugins.
//!
//! ## Test modules
//!
//! - `loading` — plugin loading from `.dylib`/`.so` files, error paths
//! - `lifecycle` — init, enable, disable, unload, hot-reload
//! - `dispatch` — event fan-out, command routing, subscriber filtering
//! - `registry` — command and event registry integration
//! - `config` — TOML configuration loading and per-plugin settings
//! - `sandbox` — capability-based permission enforcement
//! - `example_plugins` — verification of hello, auto-respond, and logger plugins

mod config;
mod dispatch;
mod example_plugins;
mod lifecycle;
mod loading;
mod registry;
mod sandbox;

use std::path::{Path, PathBuf};

use pirc_plugin::ffi::{
    FfiString, PluginEventType, PluginHostApi, PluginResult,
};
use pirc_plugin::manager::PluginManager;

// ── Shared test infrastructure ──────────────────────────────────────────────

/// Returns the path to the workspace target directory.
///
/// Example plugins are built as cdylib crates and their `.dylib`/`.so` files
/// land in `target/debug/` (or `target/release/`).
fn target_dir() -> PathBuf {
    // Walk up from the test binary to find the target directory.
    let mut dir = std::env::current_exe()
        .expect("current_exe must be available")
        .parent()
        .expect("binary must have a parent directory")
        .to_path_buf();

    // The test binary is typically at target/debug/deps/<binary>, so go up
    // until we find a directory named "target".
    loop {
        if dir.file_name().map_or(false, |n| n == "target") {
            return dir;
        }
        if !dir.pop() {
            panic!("could not locate target directory from test binary path");
        }
    }
}

/// Returns the platform-appropriate dynamic library file extension.
fn lib_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Returns the path to a compiled example plugin library.
///
/// On macOS: `target/debug/lib<name>.dylib`
/// On Linux: `target/debug/lib<name>.so`
///
/// The `name` parameter uses underscores (matching the crate name after
/// Cargo's normalisation), e.g. `"hello_plugin"` for `hello-plugin`.
fn example_plugin_path(name: &str) -> PathBuf {
    let target = target_dir();
    target
        .join("debug")
        .join(format!("lib{name}.{}", lib_extension()))
}

/// Returns `true` if the example plugin library exists on disk.
///
/// Tests that require a compiled plugin should call this and skip gracefully
/// if the library is not available (e.g. in CI environments that only run
/// unit tests).
fn example_plugin_exists(name: &str) -> bool {
    example_plugin_path(name).exists()
}

/// Creates a no-op [`PluginHostApi`] vtable for tests that load real plugins
/// but don't need to exercise host callbacks.
fn noop_host_api() -> PluginHostApi {
    extern "C" fn noop_register(
        _name: FfiString,
        _cb: extern "C" fn(FfiString) -> PluginResult,
    ) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_unregister(_name: FfiString) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_hook(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_unhook(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_echo(_msg: FfiString) {}
    extern "C" fn noop_log(_level: u32, _msg: FfiString) {}
    extern "C" fn noop_get_config(_key: FfiString) -> FfiString {
        FfiString::empty()
    }

    PluginHostApi {
        register_command: noop_register,
        unregister_command: noop_unregister,
        hook_event: noop_hook,
        unhook_event: noop_unhook,
        echo: noop_echo,
        log: noop_log,
        get_config_value: noop_get_config,
    }
}

/// Creates a [`PluginHostApi`] that records config values from a static map.
///
/// The `get_config_value` callback returns the value for "greeting" as
/// "Test Greeting!" and empty for all other keys.
fn config_host_api() -> PluginHostApi {
    extern "C" fn noop_register(
        _name: FfiString,
        _cb: extern "C" fn(FfiString) -> PluginResult,
    ) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_unregister(_name: FfiString) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_hook(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_unhook(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn noop_echo(_msg: FfiString) {}
    extern "C" fn noop_log(_level: u32, _msg: FfiString) {}

    #[allow(unsafe_code)]
    extern "C" fn get_config(key: FfiString) -> FfiString {
        let key_str = unsafe { key.as_str() };
        let result = match key_str {
            "greeting" => FfiString::new("Test Greeting!"),
            "log_dir" => FfiString::new("/tmp/pirc-test-logs"),
            _ => FfiString::empty(),
        };
        // Free the incoming key.
        unsafe { key.free() };
        result
    }

    PluginHostApi {
        register_command: noop_register,
        unregister_command: noop_unregister,
        hook_event: noop_hook,
        unhook_event: noop_unhook,
        echo: noop_echo,
        log: noop_log,
        get_config_value: get_config,
    }
}

/// Creates a fresh [`PluginManager`] and loads a plugin from the given path.
///
/// Returns the manager and the loaded plugin name.
fn load_plugin_into_manager(
    path: &Path,
    host_api: &PluginHostApi,
) -> (PluginManager, String) {
    let mut manager = PluginManager::new();
    let name = manager
        .load_plugin(path, host_api)
        .expect("plugin should load successfully");
    (manager, name)
}
