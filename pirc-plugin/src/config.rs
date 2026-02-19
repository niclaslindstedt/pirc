//! Per-plugin TOML configuration loading.
//!
//! Each plugin may have an optional configuration file at
//! `<plugins_dir>/<name>.toml`. The expected format is:
//!
//! ```toml
//! [plugin]
//! enabled = true
//!
//! [settings]
//! key = "value"
//! count = 42
//! ```
//!
//! If the file does not exist the plugin loads with defaults
//! (`enabled = true`, empty settings). Malformed TOML produces a warning
//! but does not prevent loading.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use tracing::warn;

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// Errors that can occur when loading a plugin configuration file.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read the configuration file from disk.
    Io(std::io::Error),
    /// The file contents are not valid TOML.
    Parse(toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "config I/O error: {e}"),
            Self::Parse(e) => write!(f, "config parse error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        Self::Parse(err)
    }
}

// ---------------------------------------------------------------------------
// PluginConfig
// ---------------------------------------------------------------------------

/// Configuration for a single plugin, loaded from its TOML file.
#[derive(Debug, Clone)]
pub struct PluginConfig {
    /// Whether the plugin should be auto-enabled on load.
    pub enabled: bool,
    /// Arbitrary plugin-specific settings as string key/value pairs.
    ///
    /// TOML values are converted to their string representation so that
    /// they can be returned through the FFI `get_config_value` callback.
    settings: HashMap<String, toml::Value>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            settings: HashMap::new(),
        }
    }
}

impl PluginConfig {
    /// Looks up a setting value by key, returning its string representation.
    ///
    /// TOML strings are returned without surrounding quotes. Other types
    /// (integers, floats, booleans) use their TOML display form.
    #[must_use]
    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.settings.get(key).map(value_to_string)
    }

    /// Returns the number of settings entries.
    #[must_use]
    pub fn settings_count(&self) -> usize {
        self.settings.len()
    }
}

/// Converts a TOML value to a user-facing string.
///
/// Strings are returned without quotes; other types use their display form.
fn value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Loads the configuration for a plugin from `<plugins_dir>/<name>.toml`.
///
/// - If the file does not exist, returns a default [`PluginConfig`]
///   (enabled, no settings).
/// - If the file exists but is malformed, logs a warning and returns defaults.
/// - On success, parses `[plugin]` for the `enabled` flag and `[settings]`
///   for arbitrary key-value pairs.
#[must_use]
pub fn load_plugin_config(plugins_dir: &Path, plugin_name: &str) -> PluginConfig {
    let path = plugins_dir.join(format!("{plugin_name}.toml"));

    if !path.exists() {
        return PluginConfig::default();
    }

    match load_plugin_config_from_file(&path) {
        Ok(config) => config,
        Err(e) => {
            warn!(
                plugin = %plugin_name,
                path = %path.display(),
                error = %e,
                "malformed plugin config, using defaults"
            );
            PluginConfig::default()
        }
    }
}

/// Internal helper that reads and parses a TOML file into a [`PluginConfig`].
fn load_plugin_config_from_file(path: &Path) -> Result<PluginConfig, ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    parse_plugin_config(&contents)
}

/// Parses a TOML string into a [`PluginConfig`].
///
/// Expects an optional `[plugin]` table with an `enabled` boolean, and an
/// optional `[settings]` table with arbitrary key-value pairs.
pub fn parse_plugin_config(toml_str: &str) -> Result<PluginConfig, ConfigError> {
    let table: toml::Table = toml_str.parse()?;

    let enabled = table
        .get("plugin")
        .and_then(toml::Value::as_table)
        .and_then(|t| t.get("enabled"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);

    let settings = table
        .get("settings")
        .and_then(toml::Value::as_table)
        .cloned()
        .map(|t| t.into_iter().collect::<HashMap<String, toml::Value>>())
        .unwrap_or_default();

    Ok(PluginConfig { enabled, settings })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // -- PluginConfig default -------------------------------------------------

    #[test]
    fn default_config_is_enabled_with_no_settings() {
        let config = PluginConfig::default();
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 0);
        assert!(config.get_setting("anything").is_none());
    }

    // -- parse_plugin_config --------------------------------------------------

    #[test]
    fn parse_full_config() {
        let toml = r#"
[plugin]
enabled = true

[settings]
greeting = "hello"
count = 42
debug = false
ratio = 3.14
"#;
        let config = parse_plugin_config(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 4);
        assert_eq!(config.get_setting("greeting").unwrap(), "hello");
        assert_eq!(config.get_setting("count").unwrap(), "42");
        assert_eq!(config.get_setting("debug").unwrap(), "false");
        assert_eq!(config.get_setting("ratio").unwrap(), "3.14");
    }

    #[test]
    fn parse_disabled_plugin() {
        let toml = r#"
[plugin]
enabled = false
"#;
        let config = parse_plugin_config(toml).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.settings_count(), 0);
    }

    #[test]
    fn parse_empty_string_returns_defaults() {
        let config = parse_plugin_config("").unwrap();
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 0);
    }

    #[test]
    fn parse_only_settings_section_defaults_enabled() {
        let toml = r#"
[settings]
foo = "bar"
"#;
        let config = parse_plugin_config(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.get_setting("foo").unwrap(), "bar");
    }

    #[test]
    fn parse_only_plugin_section_no_settings() {
        let toml = r#"
[plugin]
enabled = true
"#;
        let config = parse_plugin_config(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 0);
    }

    #[test]
    fn parse_malformed_toml_returns_error() {
        let result = parse_plugin_config("this is not [ valid toml");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
        assert!(err.to_string().contains("config parse error"));
    }

    #[test]
    fn parse_missing_enabled_defaults_to_true() {
        let toml = r#"
[plugin]
some_other_key = "value"
"#;
        let config = parse_plugin_config(toml).unwrap();
        assert!(config.enabled);
    }

    #[test]
    fn get_setting_nonexistent_key_returns_none() {
        let config = parse_plugin_config("[settings]\nfoo = 1").unwrap();
        assert!(config.get_setting("nonexistent").is_none());
    }

    // -- load_plugin_config from filesystem -----------------------------------

    #[test]
    fn load_missing_file_returns_defaults() {
        let dir = std::env::temp_dir().join("pirc_config_test_missing");
        let _ = fs::create_dir_all(&dir);
        let config = load_plugin_config(&dir, "nonexistent-plugin");
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_valid_config_file() {
        let dir = std::env::temp_dir().join("pirc_config_test_valid");
        let _ = fs::create_dir_all(&dir);

        let toml_content = r#"
[plugin]
enabled = false

[settings]
server = "irc.example.com"
port = 6667
"#;
        fs::write(dir.join("my-plugin.toml"), toml_content).unwrap();

        let config = load_plugin_config(&dir, "my-plugin");
        assert!(!config.enabled);
        assert_eq!(config.get_setting("server").unwrap(), "irc.example.com");
        assert_eq!(config.get_setting("port").unwrap(), "6667");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_malformed_file_returns_defaults() {
        let dir = std::env::temp_dir().join("pirc_config_test_malformed");
        let _ = fs::create_dir_all(&dir);

        fs::write(dir.join("bad-plugin.toml"), "not valid [[ toml {{").unwrap();

        let config = load_plugin_config(&dir, "bad-plugin");
        // Should return defaults, not crash
        assert!(config.enabled);
        assert_eq!(config.settings_count(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // -- ConfigError ----------------------------------------------------------

    #[test]
    fn config_error_display_io() {
        let err = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("config I/O error"));
    }

    #[test]
    fn config_error_source_chain() {
        use std::error::Error;

        let io_err = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "test",
        ));
        assert!(io_err.source().is_some());

        let parse_err = ConfigError::from(
            "bad".parse::<toml::Table>().unwrap_err(),
        );
        assert!(parse_err.source().is_some());
    }

    // -- value_to_string ------------------------------------------------------

    #[test]
    fn value_to_string_preserves_string_without_quotes() {
        let v = toml::Value::String("hello".into());
        assert_eq!(value_to_string(&v), "hello");
    }

    #[test]
    fn value_to_string_integer() {
        let v = toml::Value::Integer(42);
        assert_eq!(value_to_string(&v), "42");
    }

    #[test]
    fn value_to_string_boolean() {
        let v = toml::Value::Boolean(true);
        assert_eq!(value_to_string(&v), "true");
    }

    #[test]
    fn value_to_string_float() {
        let v = toml::Value::Float(1.5);
        assert_eq!(value_to_string(&v), "1.5");
    }
}
