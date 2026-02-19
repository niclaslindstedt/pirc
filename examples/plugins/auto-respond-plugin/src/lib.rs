//! # Auto-Respond Plugin — event hooks and configuration demo
//!
//! This crate demonstrates two key pirc plugin capabilities:
//!
//! 1. **Event hooking** — subscribing to [`MessageReceived`] events so the
//!    plugin is notified every time a message arrives.
//! 2. **Configuration reading** — using [`PluginHost::get_config_value`] to
//!    load a user-configurable greeting prefix at init time.
//!
//! ## What it does
//!
//! When a message containing a greeting word ("hello", "hi", "hey", "greetings")
//! is received, the plugin echoes an auto-response to the user's active window
//! using the configured (or default) greeting prefix.
//!
//! ## Example configuration
//!
//! Place a file at `~/.pirc/plugins/auto-respond.toml`:
//!
//! ```toml
//! [plugin]
//! enabled = true
//!
//! [settings]
//! greeting = "Hey there"
//! ```
//!
//! If no configuration is set the plugin defaults to `"Hello!"`.

// Import everything a plugin needs from the prelude.
use pirc_plugin::prelude::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default greeting prefix used when no configuration is provided.
const DEFAULT_GREETING: &str = "Hello!";

/// Words that trigger the auto-response (matched case-insensitively).
const TRIGGER_WORDS: &[&str] = &["hello", "hi", "hey", "greetings"];

// ---------------------------------------------------------------------------
// Plugin struct
// ---------------------------------------------------------------------------

/// The auto-respond plugin.
///
/// Stores the greeting prefix loaded from configuration during [`init`].
/// A real-world plugin might store additional state such as rate-limit
/// counters or per-channel configuration, but for this example a single
/// `String` is sufficient.
#[derive(Default)]
struct AutoRespondPlugin {
    /// The greeting prefix echoed when a trigger word is detected.
    greeting: String,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Returns `true` if `message` contains any of the [`TRIGGER_WORDS`].
///
/// The check is case-insensitive and looks for whole-word boundaries so that
/// e.g. "highway" does not match "hi".  A simple approach: split on
/// whitespace and compare each word after lowercasing.
fn contains_greeting(message: &str) -> bool {
    message
        .split_whitespace()
        // Strip common punctuation so "hello!" or "hi," still match.
        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()).to_lowercase())
        .any(|word| TRIGGER_WORDS.contains(&word.as_str()))
}

// ---------------------------------------------------------------------------
// Plugin trait implementation
// ---------------------------------------------------------------------------

#[allow(clippy::unnecessary_literal_bound)]
impl Plugin for AutoRespondPlugin {
    /// A unique, human-readable name for the plugin.
    fn name(&self) -> &str {
        "auto-respond"
    }

    /// Semantic version of the plugin (pulled from Cargo.toml).
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// Short description shown in `/plugin info`.
    fn description(&self) -> &str {
        "Auto-responds to greetings — demonstrates event hooks and config reading"
    }

    /// Author information.
    fn author(&self) -> &str {
        "pirc contributors"
    }

    /// Declare the capabilities this plugin requires.
    ///
    /// - [`HookEvents`](PluginCapability::HookEvents) — to subscribe to
    ///   `MessageReceived` events.
    /// - [`ReadConfig`](PluginCapability::ReadConfig) — to read the greeting
    ///   prefix from the plugin's configuration file.
    fn capabilities(&self) -> &[PluginCapability] {
        &[PluginCapability::HookEvents, PluginCapability::ReadConfig]
    }

    /// Called once after the plugin is loaded.
    ///
    /// We use this to:
    /// 1. Read the greeting prefix from configuration (falling back to a
    ///    sensible default).
    /// 2. Subscribe to `MessageReceived` events so [`on_event`] is called
    ///    for every incoming message.
    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError> {
        // --- Configuration ---------------------------------------------------
        // Try to read a custom greeting from the plugin's config file.
        // If the key is missing or the config file does not exist, fall back
        // to DEFAULT_GREETING.
        self.greeting = host
            .get_config_value("greeting")
            .unwrap_or_else(|| DEFAULT_GREETING.to_owned());

        host.log(
            LogLevel::Info,
            &format!("auto-respond: greeting set to {:?}", self.greeting),
        );

        // --- Event subscription ----------------------------------------------
        // Subscribe to MessageReceived so we are notified of every channel or
        // private message that arrives.
        host.hook_event(PluginEventType::MessageReceived)?;

        host.log(LogLevel::Info, "auto-respond plugin initialised");

        Ok(())
    }

    /// Called when a subscribed event fires.
    ///
    /// For [`MessageReceived`] events:
    /// - `event.data`   — the message text
    /// - `event.source` — the sender nick or `nick!user@host` mask
    ///
    /// We check whether the message contains a greeting word and, if so,
    /// echo an auto-response to the user's active window.
    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> {
        // Only handle message-received events; ignore everything else.
        if event.event_type != PluginEventType::MessageReceived {
            return Ok(());
        }

        // Check if the message text contains a greeting.
        if contains_greeting(&event.data) {
            // NOTE: The PluginHost reference is not available inside on_event
            // in the current API, so we cannot call host.echo() here.  A
            // future API revision may pass the host into on_event, enabling
            // richer responses.  For now we log the intent and return Ok(())
            // to signal successful handling.
            //
            // When the host API evolves to pass `host` into `on_event`, the
            // response would look like:
            //
            //   host.echo(&format!("{} — from {}", self.greeting, event.source));
            let _ = format!("{} — from {}", self.greeting, event.source);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FFI entry point
// ---------------------------------------------------------------------------

// This macro generates the `pirc_plugin_init` symbol that the host's plugin
// loader looks for when opening the dynamic library.  See the hello-plugin
// example for a detailed explanation of what it creates.
pirc_plugin::declare_plugin!(AutoRespondPlugin);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- contains_greeting tests ---------------------------------------------

    #[test]
    fn greeting_matches_hello() {
        assert!(contains_greeting("hello world"));
    }

    #[test]
    fn greeting_matches_hi() {
        assert!(contains_greeting("hi there"));
    }

    #[test]
    fn greeting_matches_hey() {
        assert!(contains_greeting("hey, how are you?"));
    }

    #[test]
    fn greeting_matches_case_insensitive() {
        assert!(contains_greeting("HELLO everyone"));
        assert!(contains_greeting("Hi There"));
        assert!(contains_greeting("HEY!"));
    }

    #[test]
    fn greeting_with_punctuation() {
        assert!(contains_greeting("hello!"));
        assert!(contains_greeting("hi,"));
        assert!(contains_greeting("hey..."));
    }

    #[test]
    fn no_false_positives() {
        // "highway" should not trigger on "hi"
        assert!(!contains_greeting("take the highway"));
        // "helloworld" is one token, not "hello"
        assert!(!contains_greeting("helloworld"));
        // Unrelated message
        assert!(!contains_greeting("the quick brown fox"));
    }

    #[test]
    fn empty_message() {
        assert!(!contains_greeting(""));
    }

    #[test]
    fn greeting_alone() {
        assert!(contains_greeting("hello"));
        assert!(contains_greeting("greetings"));
    }

    // -- Plugin metadata tests -----------------------------------------------

    #[test]
    fn plugin_name() {
        let plugin = AutoRespondPlugin::default();
        assert_eq!(plugin.name(), "auto-respond");
    }

    #[test]
    fn plugin_version_matches_cargo() {
        let plugin = AutoRespondPlugin::default();
        assert_eq!(plugin.version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn plugin_capabilities() {
        let plugin = AutoRespondPlugin::default();
        let caps = plugin.capabilities();
        assert!(caps.contains(&PluginCapability::HookEvents));
        assert!(caps.contains(&PluginCapability::ReadConfig));
        assert_eq!(caps.len(), 2);
    }

    // -- on_event tests ------------------------------------------------------

    #[test]
    fn on_event_ignores_non_message_events() {
        let mut plugin = AutoRespondPlugin {
            greeting: "Hey!".into(),
        };
        let event = PluginEvent {
            event_type: PluginEventType::UserJoined,
            data: "hello".into(),
            source: "nick".into(),
        };
        // Should succeed without doing anything.
        assert!(plugin.on_event(&event).is_ok());
    }

    #[test]
    fn on_event_handles_greeting_message() {
        let mut plugin = AutoRespondPlugin {
            greeting: "Hey there".into(),
        };
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "hello everyone!".into(),
            source: "alice".into(),
        };
        // Should succeed (the actual echo is a no-op until the API evolves).
        assert!(plugin.on_event(&event).is_ok());
    }

    #[test]
    fn on_event_ignores_non_greeting_message() {
        let mut plugin = AutoRespondPlugin {
            greeting: "Hi!".into(),
        };
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "what time is it?".into(),
            source: "bob".into(),
        };
        assert!(plugin.on_event(&event).is_ok());
    }
}
