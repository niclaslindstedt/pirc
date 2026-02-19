# Iteration I024 Analysis

## Summary

Iteration I024 completed Epic E024 (Example Plugins & Plugin SDK), delivering the plugin SDK prelude module and three example plugins that demonstrate all major features of the pirc-plugin API. This iteration focused on developer experience — making it easy for third-party authors to build plugins by providing a convenient import prelude and well-commented example code covering command registration, event hooking, configuration, state management, and lifecycle management.

With E024 complete, the plugin system ecosystem is fully delivered: the core plugin infrastructure (E023) plus the SDK and examples (E024) give plugin authors everything they need to build, configure, and deploy pirc plugins.

## Completed Work

### SDK Enhancement (1 ticket, 1 CR)

- **T274** (CR228): Prelude module — added `pirc_plugin::prelude` re-exporting `Plugin`, `PluginHost`, `PluginError`, `PluginEvent`, `LogLevel`, `PluginCapability`, and `PluginEventType`. Allows plugin authors to write `use pirc_plugin::prelude::*;` instead of importing from multiple submodules. 10-line module, purely additive.

### Example Plugins (3 tickets, 3 CRs)

- **T275** (CR229): Hello-world plugin — minimal cdylib crate demonstrating the bare essentials: `declare_plugin!` macro usage, `/hello` command registration, `CommandExecuted` event hooking, and basic `host.log()` calls. Includes teaching comments explaining the FFI adapter, capability declarations, and current API limitations (host not available in `on_event`). 139 lines.

- **T276** (CR230): Auto-respond plugin — event-driven plugin demonstrating `MessageReceived` event hooking and `ReadConfig` capability. Reads a configurable greeting from TOML config via `host.get_config_value()`, pattern-matches on incoming message content, and sends auto-responses. Demonstrates plugin configuration and the event dispatch system. 310 lines.

- **T277** (CR231): Channel-logger plugin — multi-event hooking plugin demonstrating lifecycle management and internal state. Hooks 5 event types (`MessageReceived`, `UserJoined`, `UserParted`, `UserQuit`, `NickChanged`), maintains an in-memory log buffer, reads `log_dir` from config, and flushes to disk on shutdown. Demonstrates the full plugin lifecycle (init → events → shutdown), state management, and file I/O from a cdylib. 498 lines.

### Follow-up Tickets (2 tickets, no additional CRs)

- **T278**: Plugin development guide documentation — auto-closed when T279 was claimed. The documentation requirements were partially fulfilled by the extensive teaching comments in the example plugins themselves.

- **T279**: Fix hello-plugin doc comment — addressed review feedback from CR229 that caught inaccurate claims about `host.echo()` being called. The fix was incorporated into the T275 branch before merge.

## Metrics

| Metric | Value |
|--------|-------|
| Total tickets | 6 (4 implementation + 2 follow-up) |
| CRs merged | 4 |
| CRs with changes requested | 1 (CR229 — doc comment accuracy) |
| Example plugin code | 947 lines across 3 plugins |
| Prelude module | 10 lines |
| Workspace crates added | 3 (hello-plugin, auto-respond-plugin, logger-plugin) |

## Challenges

1. **Inaccurate doc comment caught in review**: CR229 (hello-world plugin) had a crate-level doc comment that claimed the plugin calls `host.echo()` and logs on command invocation — neither was true. The inner code correctly acknowledged the API limitation, but the top-level doc contradicted the actual behavior. Review caught this discrepancy, spawning T279 which was fixed before merge.

2. **`unsafe-code = "deny"` workspace lint vs cdylib plugins**: Each example plugin needed an explicit `[lints.rust] unsafe-code = "allow"` override in its Cargo.toml because the `declare_plugin!` macro generates `#[no_mangle] extern "C"` FFI entry points. This is an inherent tension between workspace-wide safety lints and the FFI requirements of dynamic library plugins.

3. **Host not available in `on_event`**: The current plugin API limitation means plugins cannot call `host.echo()` from event handlers. The hello-world plugin documents this honestly as a teaching point. This is a known API gap that could be addressed in a future iteration.

## Learnings

1. **Example code as documentation**: Well-commented example plugins serve as the most effective form of plugin development documentation. Each of the three examples progressively demonstrates more API features, creating a natural learning path: hello-world (basics) → auto-respond (events + config) → channel-logger (multi-event + state + lifecycle).

2. **Prelude pattern reduces friction**: The prelude module is only 10 lines but eliminates the need for plugin authors to know the internal module structure of `pirc-plugin`. This is a standard Rust ecosystem pattern that pays dividends in developer experience.

3. **cdylib lint overrides are per-crate**: The workspace `unsafe-code = "deny"` lint is correct for the main crates but must be overridden per plugin crate. This is well-documented in each Cargo.toml with a comment explaining why, establishing a pattern for future plugin authors to follow.

4. **Review catches doc/code mismatch**: The code review process correctly identified that documentation (doc comments) can become inconsistent with implementation during iterative development. Top-level descriptions should be written last, after implementation is finalized.

## Recommendations

- **Epic E024 is complete.** The plugin SDK and example plugins are delivered. Phase P009 (Native Plugin System) is fully delivered across both E023 and E024.
- The deferred plugin development guide (T278) could be revisited as a standalone task. The example plugins provide good coverage but a formal README.md in `examples/plugins/` would help discoverability.
- The `on_event` host access limitation surfaced in the hello-world plugin is worth tracking. A future enhancement could provide a host reference or callback mechanism within event handlers.
- The three example plugins form a good test suite for plugin API changes — any breaking change to `pirc-plugin` will be caught by `make build` since they're workspace members.
