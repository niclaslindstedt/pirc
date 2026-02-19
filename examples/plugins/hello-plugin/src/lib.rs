//! # Hello Plugin — a minimal pirc plugin example
//!
//! This crate demonstrates how to write a pirc plugin from scratch.  It is
//! compiled as a `cdylib` so the pirc client can load it at runtime via
//! `libloading`.
//!
//! ## What it does
//!
//! * Registers a `/hello` slash-command during initialisation.
//! * Hooks the [`CommandExecuted`](pirc_plugin::prelude::PluginEventType::CommandExecuted)
//!   event so it is notified when a user types `/hello`.
//! * Logs a startup message during `init`.
//! * The `on_event` handler matches `/hello` commands but is currently a no-op
//!   because the [`PluginHost`](pirc_plugin::prelude::PluginHost) reference is
//!   not available inside `on_event` in the current API.  A future API revision
//!   may pass the host into `on_event`, enabling richer command responses.
//!
//! ## Plugin anatomy
//!
//! Every pirc plugin needs three things:
//!
//! 1. A struct that implements [`Plugin`] **and** [`Default`].
//! 2. Trait method implementations for at least `name`, `version`, and `init`.
//! 3. A call to [`declare_plugin!`](pirc_plugin::declare_plugin) which
//!    generates the `extern "C"` FFI entry point the host looks for.

// Import everything a plugin needs from the prelude.
use pirc_plugin::prelude::*;

// ---------------------------------------------------------------------------
// Plugin struct
// ---------------------------------------------------------------------------

/// The hello-world plugin.
///
/// This struct holds any state the plugin needs between calls.  For this
/// minimal example there is nothing to store, but a real plugin might keep
/// configuration, caches, or counters here.
#[derive(Default)]
struct HelloPlugin;

// ---------------------------------------------------------------------------
// Plugin trait implementation
// ---------------------------------------------------------------------------

#[allow(clippy::unnecessary_literal_bound)]
impl Plugin for HelloPlugin {
    /// A unique, human-readable name for the plugin.
    ///
    /// The host uses this as the plugin's identity in logs, the command
    /// registry, and the `/plugin list` output.
    fn name(&self) -> &str {
        "hello-plugin"
    }

    /// Semantic version of the plugin (displayed by `/plugin info`).
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// Short description shown in `/plugin info`.
    fn description(&self) -> &str {
        "A minimal example plugin that registers a /hello command"
    }

    /// Author information.
    fn author(&self) -> &str {
        "pirc contributors"
    }

    /// Declare the capabilities this plugin requires.
    ///
    /// The host's sandbox checks these before allowing API calls.  We need
    /// [`RegisterCommands`](PluginCapability::RegisterCommands) to call
    /// `register_command` and [`HookEvents`](PluginCapability::HookEvents) to
    /// subscribe to event types.
    fn capabilities(&self) -> &[PluginCapability] {
        &[
            PluginCapability::RegisterCommands,
            PluginCapability::HookEvents,
        ]
    }

    /// Called once after the plugin is loaded.
    ///
    /// This is the place to register commands, hook events, read
    /// configuration, and perform any one-time setup.  The `host` reference
    /// provides the full [`PluginHost`] API — but note that it is only
    /// available during this call.
    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError> {
        // Register "/hello" so it appears in the client's command list.
        // The second argument is a human-readable description.
        host.register_command("hello", "Say hello from the example plugin")?;

        // Subscribe to CommandExecuted events.  Without this, the plugin's
        // `on_event` would never be called when someone types `/hello`.
        host.hook_event(PluginEventType::CommandExecuted)?;

        // Log that we started up successfully.
        host.log(LogLevel::Info, "hello-plugin initialised");

        Ok(())
    }

    /// Called when an event the plugin subscribed to is fired.
    ///
    /// For [`CommandExecuted`](PluginEventType::CommandExecuted) events:
    /// - `event.data`   — the command name (e.g. `"hello"`)
    /// - `event.source` — the raw argument string (e.g. `"arg1 arg2"`)
    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> {
        // Only handle the `/hello` command; ignore everything else.
        if event.event_type == PluginEventType::CommandExecuted && event.data == "hello" {
            // NOTE: The PluginHost reference is not available inside on_event
            // in the current API, so we cannot call host.echo() here.  The
            // host acknowledges command dispatch on the client side.  A future
            // API revision may pass the host into on_event.
            //
            // For now, returning Ok(()) signals to the host that the command
            // was handled successfully.
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FFI entry point
// ---------------------------------------------------------------------------

// This macro generates the `pirc_plugin_init` symbol that the host's plugin
// loader looks for when opening the dynamic library.  It creates:
//
// - A static `Mutex<Option<HelloPlugin>>` holding the singleton instance
// - An `FfiHostAdapter` that translates raw C function pointers into the
//   safe `PluginHost` trait
// - All required `extern "C"` lifecycle functions (init, on_enable,
//   on_disable, shutdown, on_event)
// - The `pirc_plugin_init() -> *const PluginApi` entry point
pirc_plugin::declare_plugin!(HelloPlugin);
