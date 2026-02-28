# Plugin Development Guide

pirc supports native plugins written in Rust and loaded as dynamic libraries (`.dylib` on macOS, `.so` on Linux). Plugins interact with the client through a C FFI interface, allowing them to handle events, register custom commands, and send messages.

## Quick Start

### 1. Create a New Plugin

```bash
cargo new --lib my-plugin
cd my-plugin
```

### 2. Configure Cargo.toml

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
pirc-plugin = { path = "../pirc-plugin" }
```

The `crate-type = ["cdylib"]` setting produces a C-compatible dynamic library.

### 3. Implement the Plugin

```rust
use pirc_plugin::prelude::*;

#[derive(Default)]
struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &str { "my-plugin" }
    fn version(&self) -> &str { "0.1.0" }

    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError> {
        host.log(LogLevel::Info, "MyPlugin loaded!");
        Ok(())
    }
}

// Generate the C FFI bridge
declare_plugin!(MyPlugin);
```

### 4. Build and Install

```bash
cargo build --release

# Copy to the plugins directory
cp target/release/libmy_plugin.dylib ~/.pirc/plugins/  # macOS
cp target/release/libmy_plugin.so ~/.pirc/plugins/      # Linux
```

### 5. Configure

Create or edit `~/.pirc/plugins/my-plugin.toml`:

```toml
path = "libmy_plugin.dylib"
enabled = true
capabilities = ["ReadConfig", "RegisterCommands"]
```

## Plugin Trait

The `Plugin` trait defines the interface every plugin must implement:

```rust
pub trait Plugin: Send {
    /// Returns the human-readable name of this plugin.
    fn name(&self) -> &str;

    /// Returns the semantic version string (e.g. "1.0.0").
    fn version(&self) -> &str;

    /// Returns a short description of what this plugin does.
    fn description(&self) -> &str { "" }

    /// Returns the author name or identifier.
    fn author(&self) -> &str { "" }

    /// Returns the capabilities this plugin requires.
    fn capabilities(&self) -> &[PluginCapability] { &[] }

    /// Called once after loading to initialise the plugin.
    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError>;

    /// Called when the plugin is about to be unloaded.
    fn shutdown(&mut self) -> Result<(), PluginError> { Ok(()) }

    /// Called when the plugin is enabled (after init or after a disable/enable cycle).
    fn on_enable(&mut self) -> Result<(), PluginError> { Ok(()) }

    /// Called when the plugin is disabled but not yet unloaded.
    fn on_disable(&mut self) -> Result<(), PluginError> { Ok(()) }

    /// Called when an event the plugin subscribed to fires.
    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> { Ok(()) }

    /// Called when the user executes a command registered by this plugin.
    /// Returns true if the command was handled.
    fn on_command(&mut self, cmd: &str, args: &[&str]) -> Result<bool, PluginError> { Ok(false) }
}
```

## Plugin Host API

The `PluginHost` trait provides callbacks for plugins to interact with the client:

| Method | Description |
|--------|-------------|
| `register_command(name, description)` | Register a new slash-command |
| `unregister_command(name)` | Unregister a previously registered command |
| `hook_event(event_type)` | Subscribe to an event type |
| `unhook_event(event_type)` | Unsubscribe from an event type |
| `echo(text)` | Print a message to the user's active window |
| `log(level, message)` | Write to the log at a given level |
| `get_config_value(key)` | Read a configuration value (returns `Option<String>`) |

### Log Levels

```rust
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}
```

## Plugin Events

The `PluginEvent` struct carries event data to plugin handlers:

```rust
pub struct PluginEvent {
    pub event_type: PluginEventType,  // e.g., MessageReceived, UserJoined
    pub data: String,                 // Primary payload (message text, nick, etc.)
    pub source: String,               // Secondary payload (channel, new nick, etc.)
}
```

### Event Types

```rust
pub enum PluginEventType {
    MessageReceived,
    UserJoined,
    UserParted,
    UserQuit,
    NickChanged,
    Connected,
    Disconnected,
    CommandExecuted,
}
```

## Capabilities and Sandboxing

Plugins declare their required capabilities, and the host enforces them at runtime. This prevents plugins from performing unauthorized actions.

```rust
pub enum PluginCapability {
    ReadConfig,         // Read configuration values
    RegisterCommands,   // Register and unregister commands
    HookEvents,         // Subscribe to and unsubscribe from events
    SendMessages,       // Send messages to channels and users
    AccessNetwork,      // Make outbound network requests
}
```

Capabilities are specified in the plugin's TOML configuration:

```toml
capabilities = ["ReadConfig", "RegisterCommands"]
```

If a plugin attempts an action it doesn't have permission for, the host returns a `PluginError::PermissionDenied`.

## Plugin Lifecycle

```
                 ┌──────────┐
                 │  Load    │  Dynamic library loaded into memory
                 └────┬─────┘
                      │
                 ┌────▼─────┐
                 │  Init    │  init() called, plugin sets up state
                 └────┬─────┘
                      │
                 ┌────▼─────┐
                 │  Enable  │  on_enable() called
                 └────┬─────┘
                      │
              ┌───────▼───────┐
              │   Running     │  on_event() and on_command()
              │               │  called as events occur
              └───────┬───────┘
                      │
                 ┌────▼─────┐
                 │ Disable  │  on_disable() called
                 └────┬─────┘
                      │
                 ┌────▼─────┐
                 │ Shutdown │  shutdown() called, plugin cleans up
                 └────┬─────┘
                      │
                 ┌────▼─────┐
                 │  Unload  │  Dynamic library unloaded from memory
                 └──────────┘
```

1. **Load:** The `PluginLoader` uses `libloading` to load the `.dylib`/`.so` file and looks up the `pirc_plugin_init` symbol (generated by `declare_plugin!`)
2. **Init:** The `init()` method is called with an immutable reference to the host. The plugin can log messages, register commands, and set up initial state.
3. **Enable:** The `on_enable()` method is called. Plugins can also be disabled and re-enabled without unloading.
4. **Running:** The plugin receives events via `on_event()` and command invocations via `on_command()`. The `PluginManager` dispatches these calls.
5. **Shutdown:** When the client exits or the plugin is unloaded, `shutdown()` is called for cleanup.
6. **Unload:** The dynamic library is removed from memory.

## The declare_plugin! Macro

The `declare_plugin!` macro generates the C FFI bridge that the plugin loader expects:

```rust
declare_plugin!(MyPlugin);
```

This expands to:
- An `extern "C"` function `pirc_plugin_init` that returns a C-compatible vtable (`PluginApi`)
- Conversion code between Rust types and C FFI types
- A static plugin instance protected by a mutex

The macro takes one argument: the plugin struct type (must implement `Plugin` and `Default`).

## Error Handling

```rust
pub enum PluginError {
    InitFailed(String),       // Initialization failure
    ShutdownFailed(String),   // Shutdown failure
    CommandFailed(String),    // Command handling error
    EventFailed(String),      // Event handling error
    PermissionDenied {        // Capability not granted
        plugin: String,
        action: String,
    },
    Other(String),            // Catch-all for other failures
}
```

Return errors from trait methods to signal failures. The plugin manager logs errors and continues running other plugins.

## Example: Auto-Respond Plugin

This plugin automatically responds to messages containing specific keywords:

```rust
use pirc_plugin::prelude::*;

struct AutoRespond {
    responses: Vec<(String, String)>,
}

impl Default for AutoRespond {
    fn default() -> Self {
        Self {
            responses: vec![
                ("hello".into(), "Hi there!".into()),
                ("help".into(), "How can I help you?".into()),
            ],
        }
    }
}

impl Plugin for AutoRespond {
    fn name(&self) -> &str { "auto-respond" }
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str { "Automatically responds to messages matching patterns" }
    fn author(&self) -> &str { "pirc" }

    fn capabilities(&self) -> &[PluginCapability] {
        &[PluginCapability::HookEvents, PluginCapability::RegisterCommands]
    }

    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError> {
        host.hook_event(PluginEventType::MessageReceived)?;
        host.register_command("addresponse", "Add an auto-response trigger")?;
        host.log(LogLevel::Info, "AutoRespond plugin loaded");
        Ok(())
    }

    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> {
        if event.event_type == PluginEventType::MessageReceived {
            let message = event.data.to_lowercase();
            for (trigger, _response) in &self.responses {
                if message.contains(trigger) {
                    // In a real plugin, you would send the response via the host
                    break;
                }
            }
        }
        Ok(())
    }

    fn on_command(&mut self, cmd: &str, args: &[&str]) -> Result<bool, PluginError> {
        if cmd == "addresponse" && args.len() >= 2 {
            let trigger = args[0].to_string();
            let response = args[1..].join(" ");
            self.responses.push((trigger, response));
            return Ok(true);
        }
        Ok(false)
    }
}

declare_plugin!(AutoRespond);
```

## Example Plugins in the Repository

The `examples/` directory contains ready-to-use reference plugins:

| Plugin | Description |
|--------|-------------|
| `hello-plugin` | Minimal plugin demonstrating the basic trait implementation |
| `auto-respond-plugin` | Automatically responds to messages matching configured patterns |
| `logger-plugin` | Logs all events to a file for debugging |

Build an example:

```bash
cd examples/hello-plugin
cargo build --release
cp target/release/libhello_plugin.dylib ~/.pirc/plugins/
```

## Plugin Directory Structure

```
~/.pirc/plugins/
├── libhello_plugin.dylib       # Plugin binary
├── hello-plugin.toml           # Plugin configuration
├── libauto_respond.dylib
├── auto-respond.toml
└── ...
```

Each plugin needs both a dynamic library file and a TOML configuration file to be loaded.
