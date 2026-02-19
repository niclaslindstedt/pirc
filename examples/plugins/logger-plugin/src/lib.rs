//! # Channel Logger Plugin — file I/O and lifecycle management demo
//!
//! This crate demonstrates several advanced pirc plugin capabilities:
//!
//! 1. **Multi-event hooking** — subscribing to five different event types
//!    ([`MessageReceived`], [`UserJoined`], [`UserParted`], [`UserQuit`],
//!    [`NickChanged`]) to capture all channel activity.
//! 2. **Internal state management** — maintaining an in-memory log buffer
//!    (`Vec<String>`) that accumulates formatted log lines as events arrive.
//! 3. **Configuration reading** — using [`PluginHost::get_config_value`] to
//!    load a user-configurable log directory at init time.
//! 4. **Lifecycle management** — flushing the log buffer to disk during
//!    [`shutdown`](Plugin::shutdown) so no data is lost when the plugin
//!    is unloaded.
//! 5. **File I/O** — writing log lines to a file using `std::fs`, which is
//!    perfectly fine for `cdylib` plugins.
//!
//! ## Example configuration
//!
//! Place a file at `~/.pirc/plugins/channel-logger.toml`:
//!
//! ```toml
//! [plugin]
//! enabled = true
//!
//! [settings]
//! log_dir = "/tmp/pirc-logs"
//! ```
//!
//! If no configuration is set the plugin defaults to `~/.pirc/logs/`.

// Import everything a plugin needs from the prelude.
use pirc_plugin::prelude::*;

// We use std::fs for writing log files on shutdown.
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default log directory used when no `log_dir` config value is provided.
const DEFAULT_LOG_DIR: &str = "~/.pirc/logs";

// ---------------------------------------------------------------------------
// Plugin struct
// ---------------------------------------------------------------------------

/// The channel-logger plugin.
///
/// This struct holds the plugin's internal state:
/// - `log_dir` — the directory to write log files to (read from config).
/// - `buffer` — an in-memory buffer of formatted log lines.  Lines are
///   accumulated here as events arrive and flushed to disk on shutdown.
///
/// This pattern (buffer + flush) is common for plugins that need to batch
/// I/O operations rather than writing to disk on every single event.
#[derive(Default)]
struct ChannelLoggerPlugin {
    /// Directory where log files will be written.
    log_dir: String,
    /// In-memory buffer of formatted log lines.
    buffer: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns a simple timestamp string for log lines.
///
/// Uses `SystemTime::now()` and formats as seconds since the UNIX epoch.
/// A production plugin would use a proper datetime library, but for this
/// teaching example `SystemTime` keeps dependencies minimal.
fn timestamp() -> String {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or_else(
            |_| "0".to_owned(),
            |d| d.as_secs().to_string(),
        )
}

/// Expands a leading `~` to the user's home directory.
///
/// Falls back to the literal path if `$HOME` is not set.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Formats an event into a human-readable log line.
///
/// The format is: `[<timestamp>] <EVENT_TYPE> <details>`
fn format_log_line(event: &PluginEvent) -> String {
    let ts = timestamp();
    match event.event_type {
        PluginEventType::MessageReceived => {
            // data = message text, source = sender nick
            format!("[{ts}] MESSAGE {}: {}", event.source, event.data)
        }
        PluginEventType::UserJoined => {
            // data = nick, source = channel
            format!("[{ts}] JOIN {} -> {}", event.data, event.source)
        }
        PluginEventType::UserParted => {
            // data = nick, source = channel
            format!("[{ts}] PART {} <- {}", event.data, event.source)
        }
        PluginEventType::UserQuit => {
            // data = nick, source = quit message
            format!("[{ts}] QUIT {} ({})", event.data, event.source)
        }
        PluginEventType::NickChanged => {
            // data = old nick, source = new nick
            format!("[{ts}] NICK {} -> {}", event.data, event.source)
        }
        // We only hook the five types above, but handle the rest gracefully.
        _ => format!("[{ts}] OTHER {:?}: {}", event.event_type, event.data),
    }
}

// ---------------------------------------------------------------------------
// Plugin trait implementation
// ---------------------------------------------------------------------------

#[allow(clippy::unnecessary_literal_bound)]
impl Plugin for ChannelLoggerPlugin {
    /// A unique, human-readable name for the plugin.
    fn name(&self) -> &str {
        "channel-logger"
    }

    /// Semantic version of the plugin (pulled from Cargo.toml).
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// Short description shown in `/plugin info`.
    fn description(&self) -> &str {
        "Logs channel activity to files — demonstrates file I/O, lifecycle, and multi-event hooking"
    }

    /// Author information.
    fn author(&self) -> &str {
        "pirc contributors"
    }

    /// Declare the capabilities this plugin requires.
    ///
    /// - [`HookEvents`](PluginCapability::HookEvents) — to subscribe to
    ///   multiple event types.
    /// - [`ReadConfig`](PluginCapability::ReadConfig) — to read the log
    ///   directory from the plugin's configuration file.
    fn capabilities(&self) -> &[PluginCapability] {
        &[PluginCapability::HookEvents, PluginCapability::ReadConfig]
    }

    /// Called once after the plugin is loaded.
    ///
    /// We use this to:
    /// 1. Read the `log_dir` setting from configuration.
    /// 2. Subscribe to all five channel-activity event types.
    /// 3. Log a startup message to the host's logging system.
    fn init(&mut self, host: &dyn PluginHost) -> Result<(), PluginError> {
        // --- Configuration ---------------------------------------------------
        // Read the log directory from the plugin's config.  If missing, fall
        // back to the default (~/.pirc/logs/).
        self.log_dir = host
            .get_config_value("log_dir")
            .unwrap_or_else(|| DEFAULT_LOG_DIR.to_owned());

        host.log(
            LogLevel::Info,
            &format!("channel-logger: log directory set to {:?}", self.log_dir),
        );

        // --- Event subscriptions ---------------------------------------------
        // Hook all five event types that represent channel activity.  Each
        // call tells the host to deliver events of that type to our on_event.
        host.hook_event(PluginEventType::MessageReceived)?;
        host.hook_event(PluginEventType::UserJoined)?;
        host.hook_event(PluginEventType::UserParted)?;
        host.hook_event(PluginEventType::UserQuit)?;
        host.hook_event(PluginEventType::NickChanged)?;

        host.log(
            LogLevel::Info,
            "channel-logger plugin initialised — logging 5 event types",
        );

        Ok(())
    }

    /// Called when the plugin is about to be unloaded.
    ///
    /// This is the lifecycle hook where we flush our in-memory buffer to disk.
    /// We create the log directory if it doesn't exist, then write all buffered
    /// lines to a file named `channel.log`.
    fn shutdown(&mut self) -> Result<(), PluginError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let dir = expand_tilde(&self.log_dir);

        // Create the log directory if it doesn't already exist.
        fs::create_dir_all(&dir).map_err(|e| {
            PluginError::ShutdownFailed(format!(
                "failed to create log directory {}: {e}",
                dir.display()
            ))
        })?;

        let log_path = dir.join("channel.log");

        // Open the file in append mode so we don't overwrite previous logs.
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| {
                PluginError::ShutdownFailed(format!(
                    "failed to open log file {}: {e}",
                    log_path.display()
                ))
            })?;

        // Write each buffered line to the file.
        for line in &self.buffer {
            writeln!(file, "{line}").map_err(|e| {
                PluginError::ShutdownFailed(format!("failed to write log line: {e}"))
            })?;
        }

        // Clear the buffer now that everything is persisted.
        let count = self.buffer.len();
        self.buffer.clear();

        // NOTE: We cannot call host.log() here because the host reference is
        // not available during shutdown.  The buffer count is used only for
        // the internal record — the host will see the file on disk.
        let _ = count;

        Ok(())
    }

    /// Called when a subscribed event fires.
    ///
    /// For each event we format a log line and append it to the internal
    /// buffer.  The buffer is flushed to disk during [`shutdown`].
    fn on_event(&mut self, event: &PluginEvent) -> Result<(), PluginError> {
        // Format the event into a human-readable log line and buffer it.
        let line = format_log_line(event);
        self.buffer.push(line);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FFI entry point
// ---------------------------------------------------------------------------

// This macro generates the `pirc_plugin_init` symbol that the host's plugin
// loader looks for when opening the dynamic library.  See the hello-plugin
// example for a detailed explanation of what it creates.
pirc_plugin::declare_plugin!(ChannelLoggerPlugin);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- format_log_line tests ------------------------------------------------

    #[test]
    fn format_message_event() {
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "hello world".into(),
            source: "alice".into(),
        };
        let line = format_log_line(&event);
        assert!(line.contains("MESSAGE alice: hello world"));
    }

    #[test]
    fn format_join_event() {
        let event = PluginEvent {
            event_type: PluginEventType::UserJoined,
            data: "bob".into(),
            source: "#general".into(),
        };
        let line = format_log_line(&event);
        assert!(line.contains("JOIN bob -> #general"));
    }

    #[test]
    fn format_part_event() {
        let event = PluginEvent {
            event_type: PluginEventType::UserParted,
            data: "carol".into(),
            source: "#general".into(),
        };
        let line = format_log_line(&event);
        assert!(line.contains("PART carol <- #general"));
    }

    #[test]
    fn format_quit_event() {
        let event = PluginEvent {
            event_type: PluginEventType::UserQuit,
            data: "dave".into(),
            source: "Connection reset".into(),
        };
        let line = format_log_line(&event);
        assert!(line.contains("QUIT dave (Connection reset)"));
    }

    #[test]
    fn format_nick_change_event() {
        let event = PluginEvent {
            event_type: PluginEventType::NickChanged,
            data: "eve".into(),
            source: "eve_away".into(),
        };
        let line = format_log_line(&event);
        assert!(line.contains("NICK eve -> eve_away"));
    }

    #[test]
    fn format_log_line_includes_timestamp() {
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "test".into(),
            source: "user".into(),
        };
        let line = format_log_line(&event);
        // Should start with [<digits>]
        assert!(line.starts_with('['));
        assert!(line.contains(']'));
    }

    // -- expand_tilde tests ---------------------------------------------------

    #[test]
    fn expand_tilde_with_home() {
        // This test relies on $HOME being set, which is typical in CI/dev.
        if let Ok(home) = std::env::var("HOME") {
            let expanded = expand_tilde("~/logs");
            assert_eq!(expanded, PathBuf::from(home).join("logs"));
        }
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let expanded = expand_tilde("/tmp/logs");
        assert_eq!(expanded, PathBuf::from("/tmp/logs"));
    }

    // -- Plugin metadata tests ------------------------------------------------

    #[test]
    fn plugin_name() {
        let plugin = ChannelLoggerPlugin::default();
        assert_eq!(plugin.name(), "channel-logger");
    }

    #[test]
    fn plugin_version_matches_cargo() {
        let plugin = ChannelLoggerPlugin::default();
        assert_eq!(plugin.version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn plugin_capabilities() {
        let plugin = ChannelLoggerPlugin::default();
        let caps = plugin.capabilities();
        assert!(caps.contains(&PluginCapability::HookEvents));
        assert!(caps.contains(&PluginCapability::ReadConfig));
        assert_eq!(caps.len(), 2);
    }

    // -- on_event tests -------------------------------------------------------

    #[test]
    fn on_event_buffers_message() {
        let mut plugin = ChannelLoggerPlugin::default();
        let event = PluginEvent {
            event_type: PluginEventType::MessageReceived,
            data: "hello".into(),
            source: "alice".into(),
        };
        assert!(plugin.on_event(&event).is_ok());
        assert_eq!(plugin.buffer.len(), 1);
        assert!(plugin.buffer[0].contains("MESSAGE alice: hello"));
    }

    #[test]
    fn on_event_buffers_multiple_events() {
        let mut plugin = ChannelLoggerPlugin::default();

        let events = vec![
            PluginEvent {
                event_type: PluginEventType::UserJoined,
                data: "bob".into(),
                source: "#test".into(),
            },
            PluginEvent {
                event_type: PluginEventType::MessageReceived,
                data: "hi all".into(),
                source: "bob".into(),
            },
            PluginEvent {
                event_type: PluginEventType::UserParted,
                data: "bob".into(),
                source: "#test".into(),
            },
        ];

        for event in &events {
            assert!(plugin.on_event(event).is_ok());
        }
        assert_eq!(plugin.buffer.len(), 3);
    }

    // -- shutdown tests -------------------------------------------------------

    #[test]
    fn shutdown_empty_buffer_is_noop() {
        let mut plugin = ChannelLoggerPlugin::default();
        assert!(plugin.shutdown().is_ok());
    }

    #[test]
    fn shutdown_flushes_buffer_to_file() {
        let tmp_dir = std::env::temp_dir().join("pirc-logger-test");
        // Clean up from any previous test run.
        let _ = fs::remove_dir_all(&tmp_dir);

        let mut plugin = ChannelLoggerPlugin {
            log_dir: tmp_dir.to_string_lossy().into_owned(),
            buffer: vec![
                "[1234] MESSAGE alice: hello".into(),
                "[1235] JOIN bob -> #test".into(),
            ],
        };

        assert!(plugin.shutdown().is_ok());
        assert!(plugin.buffer.is_empty());

        // Verify the file was written.
        let log_path = tmp_dir.join("channel.log");
        assert!(log_path.exists());

        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("MESSAGE alice: hello"));
        assert!(contents.contains("JOIN bob -> #test"));

        // Clean up.
        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn shutdown_appends_to_existing_file() {
        let tmp_dir = std::env::temp_dir().join("pirc-logger-append-test");
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();

        // Write some pre-existing content.
        let log_path = tmp_dir.join("channel.log");
        fs::write(&log_path, "existing line\n").unwrap();

        let mut plugin = ChannelLoggerPlugin {
            log_dir: tmp_dir.to_string_lossy().into_owned(),
            buffer: vec!["[1236] QUIT dave (bye)".into()],
        };

        assert!(plugin.shutdown().is_ok());

        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("existing line"));
        assert!(contents.contains("QUIT dave (bye)"));

        // Clean up.
        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
