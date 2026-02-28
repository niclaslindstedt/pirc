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
use pirc_plugin::{Plugin, PluginError, PluginEvent, PluginHost, declare_plugin};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn init(&mut self, host: &mut dyn PluginHost) -> Result<(), PluginError> {
        host.log(pirc_plugin::LogLevel::Info, "MyPlugin loaded!");
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    fn handle_event(
        &mut self,
        event: &PluginEvent,
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError> {
        // React to IRC events here
        Ok(())
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        args: &[&str],
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError> {
        // Handle custom commands here
        Ok(())
    }
}

// Generate the C FFI bridge
declare_plugin!(MyPlugin, "0.1.0");
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
capabilities = ["AccessChat", "RegisterCommand"]
```

## Plugin Trait

The `Plugin` trait defines the interface every plugin must implement:

```rust
pub trait Plugin {
    /// Called once when the plugin is loaded.
    /// Use this to initialize state, register commands, etc.
    fn init(&mut self, host: &mut dyn PluginHost) -> Result<(), PluginError>;

    /// Called when the plugin is being unloaded.
    /// Clean up resources here.
    fn shutdown(&mut self) -> Result<(), PluginError>;

    /// Called when an IRC event occurs that the plugin should know about.
    fn handle_event(
        &mut self,
        event: &PluginEvent,
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError>;

    /// Called when a user invokes a command registered by this plugin.
    fn handle_command(
        &mut self,
        cmd: &str,
        args: &[&str],
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError>;
}
```

## Plugin Host API

The `PluginHost` trait provides callbacks for plugins to interact with the client:

| Method | Description |
|--------|-------------|
| `log(level, message)` | Write to the log at a given level |
| `send_message(target, text)` | Send an IRC message (PRIVMSG) |
| `get_config(key)` | Read a configuration value |

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
    pub event_type: String,    // e.g., "PRIVMSG", "JOIN", "PART"
    pub data: String,          // Event payload
    pub context: String,       // Additional context (channel, etc.)
}
```

## Capabilities and Sandboxing

Plugins declare their required capabilities, and the host enforces them at runtime. This prevents plugins from performing unauthorized actions.

```rust
pub enum PluginCapability {
    ReadConfig,        // Read configuration values
    WriteConfig,       // Modify configuration
    AccessChat,        // Read and send chat messages
    RegisterCommand,   // Register custom slash commands
    // Additional capabilities may be added
}
```

Capabilities are specified in the plugin's TOML configuration:

```toml
capabilities = ["AccessChat", "RegisterCommand"]
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
              ┌───────▼───────┐
              │   Running     │  handle_event() and handle_command()
              │               │  called as events occur
              └───────┬───────┘
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
2. **Init:** The `init()` method is called with a mutable reference to the host. The plugin can log messages, register commands, and set up initial state.
3. **Running:** The plugin receives events via `handle_event()` and command invocations via `handle_command()`. The `PluginManager` dispatches these calls.
4. **Shutdown:** When the client exits or the plugin is unloaded, `shutdown()` is called for cleanup.
5. **Unload:** The dynamic library is removed from memory.

## The declare_plugin! Macro

The `declare_plugin!` macro generates the C FFI bridge that the plugin loader expects:

```rust
declare_plugin!(MyPlugin, "0.1.0");
```

This expands to:
- An `extern "C"` function `pirc_plugin_init` that returns a C-compatible vtable (`PluginApi`)
- Conversion code between Rust types and C FFI types
- Version string embedded in the binary for compatibility checking

The macro takes two arguments:
1. The plugin struct type (must implement `Plugin` and have a `Default` or constructor)
2. The plugin version string

## Error Handling

```rust
pub enum PluginError {
    Init(String),           // Initialization failure
    Shutdown(String),       // Shutdown failure
    Command(String),        // Command handling error
    Event(String),          // Event handling error
    PermissionDenied(String), // Capability not granted
}
```

Return errors from trait methods to signal failures. The plugin manager logs errors and continues running other plugins.

## Example: Auto-Respond Plugin

This plugin automatically responds to messages containing specific keywords:

```rust
use pirc_plugin::{Plugin, PluginError, PluginEvent, PluginHost, LogLevel, declare_plugin};

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
    fn init(&mut self, host: &mut dyn PluginHost) -> Result<(), PluginError> {
        host.log(LogLevel::Info, "AutoRespond plugin loaded");
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    fn handle_event(
        &mut self,
        event: &PluginEvent,
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError> {
        if event.event_type == "PRIVMSG" {
            let message = event.data.to_lowercase();
            for (trigger, response) in &self.responses {
                if message.contains(trigger) {
                    host.send_message(&event.context, response);
                    break;
                }
            }
        }
        Ok(())
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        args: &[&str],
        host: &mut dyn PluginHost,
    ) -> Result<(), PluginError> {
        if cmd == "addresponse" && args.len() >= 2 {
            let trigger = args[0].to_string();
            let response = args[1..].join(" ");
            self.responses.push((trigger.clone(), response));
            host.log(LogLevel::Info, &format!("Added response for: {trigger}"));
        }
        Ok(())
    }
}

declare_plugin!(AutoRespond, "1.0.0");
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
