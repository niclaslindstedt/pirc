# Iteration I023 Analysis

## Summary

Iteration I023 completed Epic E023 (Native Plugin API & Loading), delivering a full native plugin system for the pirc client. The iteration built the entire plugin stack from the ground up: a stable C FFI ABI with `#[repr(C)]` types, a safe Rust `Plugin` trait with `declare_plugin!` macro, dynamic library loading via `libloading`, a `PluginManager` lifecycle coordinator, command registration and event dispatch registries, per-plugin TOML configuration, capability-based sandboxing, optional hot-reloading, and client integration with `/plugin` management commands. The `pirc-plugin` crate is now a complete plugin system — plugins can be compiled as dynamic libraries, dropped into the plugins directory, and loaded at runtime with full access to commands, events, and the client API.

With E023 complete, Phase P009 (Native Plugin System) is fully delivered.

## Completed Work

### Core Implementation (9 tickets, 9 CRs merged)

- **T254** (CR216): C FFI ABI types — `#[repr(C)]` types for `PluginApi`, `PluginHostApi`, `PluginCapability`, `PluginInfo`, `PluginEventType`, `PluginEvent`, `PluginResult`, and `FfiString` helper for safe C string passing across the FFI boundary.

- **T255** (CR218): Plugin trait and safe Rust wrapper — `Plugin` trait with lifecycle methods (init, shutdown, enable, disable, on_event, on_command), `PluginHost` trait for host callbacks, `PluginError` type, and `declare_plugin!` macro that generates the `extern "C"` entry point bridging a Rust `Plugin` impl to the C FFI ABI.

- **T256** (CR219): Dynamic library loader — `PluginLoader` using `libloading::Library` to load `.so`/`.dylib` files, look up the `pirc_plugin_create` symbol, validate `PluginApi` function pointers, and return a `LoadedPlugin` struct. Cross-platform support for Linux and macOS.

- **T257** (CR221): PluginManager lifecycle coordinator — top-level coordinator with `HashMap<String, ManagedPlugin>` tracking loaded plugins, supporting load/unload/enable/disable operations, directory scanning with deterministic sorted loading, and graceful error handling where one plugin's failure doesn't block others.

- **T258** (CR222): Command registration and event dispatch — `CommandRegistry` with case-insensitive lookup and first-registrant-wins conflict detection, `EventRegistry` with per-event-type subscriber fan-out, and `PluginManager` integration for `dispatch_command()` and `dispatch_event()` routing.

- **T259** (CR224): Plugin configuration from TOML — `PluginConfig` loading from `<plugins_dir>/<name>.toml` with `[plugin]` section for enabled flag and `[settings]` section for arbitrary key/value pairs, routed to plugins via `PluginHost.get_config_value()`. Missing config files default gracefully.

- **T260** (CR225): Capability-based sandboxing — `CapabilityChecker` enforcing declared `PluginCapability` permissions (ReadConfig, RegisterCommands, HookEvents, SendMessages, AccessNetwork) before every host callback. Denied actions return `PluginError::PermissionDenied` and are logged.

- **T261** (CR226): Hot-reloading — `reload_plugin()` and `reload_all()` methods with clean shutdown-unload-reload-reinit cycle, file modification time tracking via `SystemTime`, and `check_for_changes()` to detect modified libraries. Opt-in, not automatic.

- **T262** (CR227): Client integration — `PluginManager` initialization on startup (respecting `PluginsConfig.enabled` flag), `/plugin` command family (list, load, unload, reload, enable, disable, info), plugin command dispatch from the client input handler, and event dispatch for IRC events.

### Follow-up/Refinement Tickets (11 tickets)

- **T263**: End-to-end integration tests with test plugin cdylib — auto-closed (scope deferred).
- **T264** (CR216/T254): Redesign `PluginResult` as plain `#[repr(C)]` struct — review feedback fixing the FFI result type.
- **T265** (CR216/T254): Document `PluginInfo` capabilities lifetime and add `free_info` — memory safety fix for capability array lifetime.
- **T266** (CR216/T254): Remove unused dependencies from pirc-plugin Cargo.toml — cleanup.
- **T267** (CR216/T254): Fix `plugin_init` storing instance on init error — bug fix preventing dangling plugin instances after failed init.
- **T268** (CR219/T256): Extract shared test helper for noop `PluginHostApi` — test deduplication.
- **T269** (CR221/T257): Extract dispatch logic from manager.rs into dedicated module — file size management.
- **T270** (CR221/T257): Fix `FfiString` memory leaks in dispatch_command and dispatch_event — memory safety fix.
- **T271** (CR222/T258): Rebase T258 branch onto current main to resolve merge conflicts.
- **T272** (CR222/T258): Deduplicate unsafe FFI code in load_plugin methods — incorporated into T259 branch.
- **T273** (CR222/T258): Split manager.rs into submodules to stay under 1000-line limit — incorporated into T259 branch.

## Metrics

| Metric | Value |
|--------|-------|
| Total tickets | 20 (9 core + 11 follow-up) |
| CRs merged | 9 (plus ~4 rejected/superseded attempts) |
| Total CRs processed | ~13 |
| pirc-plugin crate size | 4,709 lines across 13 source files |
| Unit tests passing | 128 |
| Modules created | ffi, plugin, macros, loader, manager (mod/types/lifecycle/reload), registry, dispatch, config, sandbox |

## Challenges

1. **FfiString memory leaks caught in code review**: CR222 (command/event dispatch) had memory leaks where `FfiString::new()` allocated via `CString::into_raw()` but never freed after FFI calls. The review caught this, spawning T270 to add explicit `ffi_event.data.free()` / `ffi_event.source.free()` calls after dispatch. FFI memory management requires meticulous attention.

2. **Manager.rs exceeded 1000-line limit twice**: The `PluginManager` accumulated lifecycle, dispatch, and reload logic. CR222 review flagged it at 1171 lines, spawning T269 (extract dispatch.rs) and T273 (split into submodules). The final structure uses `manager/mod.rs`, `manager/types.rs`, `manager/lifecycle.rs`, and `manager/reload.rs`.

3. **Merge conflicts on T258 branch**: CR222 required rebasing (T271) because the branch contained its own copy of the T257 commit while main had a different version. This is a recurring challenge when multiple sequential tickets modify the same module.

4. **Multiple CR attempts for several tickets**: T254, T255, T257, T258, and T259 each required 2 CR submissions — the first receiving changes-requested or needing resubmission due to merge conflicts. This is expected for an FFI-heavy crate where memory safety and ABI stability are paramount.

## Learnings

1. **C FFI requires defense-in-depth**: Every FFI boundary is a potential memory safety issue. The combination of `#[repr(C)]` types, `FfiString` with explicit allocation/deallocation, and `CapabilityChecker` guards creates multiple layers of protection. The code review process caught two memory leak categories that would be invisible in safe Rust.

2. **`declare_plugin!` macro simplifies plugin authoring**: The procedural macro generates all the `extern "C"` boilerplate, letting plugin authors write pure safe Rust that implements the `Plugin` trait. This dramatically lowers the barrier to writing plugins.

3. **Capability-based sandboxing is lightweight but effective**: Rather than complex OS-level sandboxing, the capability system is a simple enum check before every host callback. Plugins declare their capabilities upfront, and the host enforces them. This prevents accidental misuse without the complexity of process isolation.

4. **Manager submodule pattern works well**: Splitting `manager.rs` into `mod.rs`, `types.rs`, `lifecycle.rs`, and `reload.rs` keeps each file under 700 lines while maintaining a clean public API. This pattern should be the default for any module that will accumulate lifecycle, dispatch, and configuration logic.

5. **Hot-reload is opt-in by design**: Making reload an explicit action (not automatic file watching) avoids the complexity of file system watchers and the risk of loading partially-written libraries. The client decides when to check for changes and trigger reloads.

## Recommendations

- **Phase P009 (Native Plugin System) is complete.** The pirc-plugin crate delivers a full plugin system with C FFI ABI, safe Rust wrappers, dynamic loading, lifecycle management, configuration, sandboxing, hot-reloading, and client integration.
- End-to-end integration tests with a real compiled cdylib plugin (T263) were deferred. This would be valuable for CI validation but requires building a test plugin as part of the test suite.
- The plugin system and scripting engine (Phase P008) are now both delivered. A future epic could bridge them — e.g., plugins that register script functions or scripts that call plugin APIs.
- The `/plugin` command family in the client provides full runtime management. Consider adding a `/plugin check` command that calls `check_for_changes()` and reports which plugins have been modified.
