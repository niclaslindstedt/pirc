//! Client configuration types and defaults.
//!
//! Defines [`ClientConfig`] and its nested sub-structs for the `pirc` client.
//! All structs derive `Serialize`, `Deserialize`, `Debug`, and `Clone`, and
//! provide sensible defaults via [`Default`].
//!
//! The [`ClientConfig::load`] method handles file-based loading with automatic
//! path discovery, and [`ClientConfig::validate`] ensures all values are within
//! acceptable ranges.

use std::path::Path;

use pirc_common::config::default_client_config_path;
use pirc_common::{Nickname, PircError};
use serde::{Deserialize, Serialize};

/// Top-level client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub server: ServerConnection,
    pub identity: IdentityConfig,
    pub ui: UiConfig,
    pub scripting: ScriptingConfig,
    pub plugins: PluginsConfig,
}

impl ClientConfig {
    /// Loads configuration from the given path, or auto-discovers from default paths.
    ///
    /// If an explicit path is provided and the file does not exist, returns an error.
    /// If no path is provided, attempts to discover a config file from the default
    /// location (`$XDG_CONFIG_HOME/pirc/pirc.toml` or `~/.pirc/pirc.toml`).
    /// If no config file exists, returns the default configuration for zero-config startup.
    pub fn load(path: Option<&Path>) -> pirc_common::Result<Self> {
        let config_path = match path {
            Some(p) => {
                if !p.exists() {
                    return Err(PircError::ConfigError {
                        message: format!("config file not found: {}", p.display()),
                    });
                }
                p.to_path_buf()
            }
            None => match default_client_config_path() {
                Some(p) if p.exists() => p,
                _ => return Ok(Self::default()),
            },
        };

        let contents =
            std::fs::read_to_string(&config_path).map_err(|e| PircError::ConfigError {
                message: format!("failed to read {}: {e}", config_path.display()),
            })?;

        let config: Self =
            toml::from_str(&contents).map_err(|e| PircError::ConfigError {
                message: format!("failed to parse {}: {e}", config_path.display()),
            })?;

        Ok(config)
    }

    /// Validates the configuration values are within acceptable ranges.
    ///
    /// Returns `Ok(())` if all values are valid, or a `ConfigError` describing
    /// the first invalid value encountered.
    pub fn validate(&self) -> pirc_common::Result<()> {
        if self.server.port == 0 {
            return Err(PircError::ConfigError {
                message: "port must be between 1 and 65535".into(),
            });
        }

        if let Some(ref nick) = self.identity.nick {
            Nickname::new(nick).map_err(|e| PircError::ConfigError {
                message: format!("invalid nick '{}': {e}", nick),
            })?;
        }

        for alt in &self.identity.alt_nicks {
            Nickname::new(alt).map_err(|e| PircError::ConfigError {
                message: format!("invalid alt nick '{}': {e}", alt),
            })?;
        }

        if self.ui.scrollback_lines == 0 {
            return Err(PircError::ConfigError {
                message: "scrollback_lines must be greater than 0".into(),
            });
        }

        if self.server.reconnect_delay_secs == 0 {
            return Err(PircError::ConfigError {
                message: "reconnect_delay_secs must be greater than 0".into(),
            });
        }

        Ok(())
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server: ServerConnection::default(),
            identity: IdentityConfig::default(),
            ui: UiConfig::default(),
            scripting: ScriptingConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

/// Server connection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConnection {
    pub address: String,
    pub port: u16,
    pub tls: bool,
    pub auto_reconnect: bool,
    pub reconnect_delay_secs: u64,
}

impl Default for ServerConnection {
    fn default() -> Self {
        Self {
            address: String::from("localhost"),
            port: 6667,
            tls: false,
            auto_reconnect: true,
            reconnect_delay_secs: 5,
        }
    }
}

/// User identity settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityConfig {
    pub nick: Option<String>,
    pub alt_nicks: Vec<String>,
    pub realname: Option<String>,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            nick: None,
            alt_nicks: Vec::new(),
            realname: None,
        }
    }
}

/// UI display settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub timestamps: bool,
    pub timestamp_format: String,
    pub scrollback_lines: u32,
    pub show_joins_parts: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            timestamps: true,
            timestamp_format: String::from("%H:%M"),
            scrollback_lines: 1000,
            show_joins_parts: true,
        }
    }
}

/// Scripting engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptingConfig {
    pub enabled: bool,
    pub scripts_dir: Option<String>,
}

impl Default for ScriptingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scripts_dir: None,
        }
    }
}

/// Plugin system settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub plugins_dir: Option<String>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            plugins_dir: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ---- Load tests ----

    #[test]
    fn load_returns_defaults_when_no_config_exists() {
        let config = ClientConfig::load(None).expect("load defaults");
        assert_eq!(config.server.address, "localhost");
        assert_eq!(config.server.port, 6667);
        assert!(!config.server.tls);
        assert!(config.identity.nick.is_none());
    }

    #[test]
    fn load_from_explicit_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pirc.toml");
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(
            br#"
[server]
address = "irc.example.com"
port = 6697
tls = true

[identity]
nick = "rustacean"
alt_nicks = ["rustacean_", "rustacean__"]
"#,
        )
        .expect("write");

        let config = ClientConfig::load(Some(&path)).expect("load from file");
        assert_eq!(config.server.address, "irc.example.com");
        assert_eq!(config.server.port, 6697);
        assert!(config.server.tls);
        assert_eq!(config.identity.nick.as_deref(), Some("rustacean"));
        assert_eq!(config.identity.alt_nicks.len(), 2);
        // Defaults for unset fields
        assert!(config.server.auto_reconnect);
        assert_eq!(config.server.reconnect_delay_secs, 5);
        assert_eq!(config.ui.scrollback_lines, 1000);
    }

    #[test]
    fn load_from_nonexistent_path_returns_error() {
        let result = ClientConfig::load(Some(Path::new("/tmp/nonexistent_pirc_client_config.toml")));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("config file not found"));
    }

    #[test]
    fn load_from_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid { toml }}}").expect("write");

        let result = ClientConfig::load(Some(&path));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    // ---- Validate tests ----

    #[test]
    fn validate_defaults_pass() {
        let config = ClientConfig::default();
        config.validate().expect("defaults should be valid");
    }

    #[test]
    fn validate_port_zero() {
        let mut config = ClientConfig::default();
        config.server.port = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("port must be between 1 and 65535"));
    }

    #[test]
    fn validate_invalid_nick() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("123invalid".into());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("invalid nick"));
    }

    #[test]
    fn validate_valid_nick() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("validnick".into());
        config.validate().expect("valid nick should pass");
    }

    #[test]
    fn validate_invalid_alt_nick() {
        let mut config = ClientConfig::default();
        config.identity.alt_nicks = vec!["good".into(), "123bad".into()];
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("invalid alt nick"));
    }

    #[test]
    fn validate_valid_alt_nicks() {
        let mut config = ClientConfig::default();
        config.identity.alt_nicks = vec!["nick1".into(), "nick2".into()];
        config.validate().expect("valid alt nicks should pass");
    }

    #[test]
    fn validate_scrollback_lines_zero() {
        let mut config = ClientConfig::default();
        config.ui.scrollback_lines = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("scrollback_lines must be greater than 0"));
    }

    #[test]
    fn validate_reconnect_delay_zero() {
        let mut config = ClientConfig::default();
        config.server.reconnect_delay_secs = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("reconnect_delay_secs must be greater than 0"));
    }

    #[test]
    fn validate_no_nick_passes() {
        let config = ClientConfig::default();
        assert!(config.identity.nick.is_none());
        config.validate().expect("no nick should be valid");
    }

    // ---- Default value tests ----

    #[test]
    fn client_config_defaults() {
        let config = ClientConfig::default();
        assert_eq!(config.server.address, "localhost");
        assert_eq!(config.server.port, 6667);
        assert!(!config.server.tls);
        assert!(config.server.auto_reconnect);
        assert_eq!(config.server.reconnect_delay_secs, 5);
    }

    #[test]
    fn identity_config_defaults() {
        let id = IdentityConfig::default();
        assert!(id.nick.is_none());
        assert!(id.alt_nicks.is_empty());
        assert!(id.realname.is_none());
    }

    #[test]
    fn ui_config_defaults() {
        let ui = UiConfig::default();
        assert!(ui.timestamps);
        assert_eq!(ui.timestamp_format, "%H:%M");
        assert_eq!(ui.scrollback_lines, 1000);
        assert!(ui.show_joins_parts);
    }

    #[test]
    fn scripting_config_defaults() {
        let scripting = ScriptingConfig::default();
        assert!(scripting.enabled);
        assert!(scripting.scripts_dir.is_none());
    }

    #[test]
    fn plugins_config_defaults() {
        let plugins = PluginsConfig::default();
        assert!(plugins.enabled);
        assert!(plugins.plugins_dir.is_none());
    }

    // ---- TOML round-trip tests ----

    #[test]
    fn toml_round_trip_defaults() {
        let config = ClientConfig::default();
        let toml_str = toml::to_string(&config).expect("serialize to TOML");
        let parsed: ClientConfig = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(parsed.server.address, config.server.address);
        assert_eq!(parsed.server.port, config.server.port);
        assert_eq!(parsed.server.tls, config.server.tls);
        assert_eq!(parsed.server.auto_reconnect, config.server.auto_reconnect);
        assert_eq!(parsed.server.reconnect_delay_secs, config.server.reconnect_delay_secs);
        assert_eq!(parsed.identity.nick, config.identity.nick);
        assert!(parsed.identity.alt_nicks.is_empty());
        assert_eq!(parsed.identity.realname, config.identity.realname);
        assert_eq!(parsed.ui.timestamps, config.ui.timestamps);
        assert_eq!(parsed.ui.timestamp_format, config.ui.timestamp_format);
        assert_eq!(parsed.ui.scrollback_lines, config.ui.scrollback_lines);
        assert_eq!(parsed.ui.show_joins_parts, config.ui.show_joins_parts);
        assert_eq!(parsed.scripting.enabled, config.scripting.enabled);
        assert_eq!(parsed.scripting.scripts_dir, config.scripting.scripts_dir);
        assert_eq!(parsed.plugins.enabled, config.plugins.enabled);
        assert_eq!(parsed.plugins.plugins_dir, config.plugins.plugins_dir);
    }

    #[test]
    fn toml_round_trip_with_all_fields() {
        let config = ClientConfig {
            server: ServerConnection {
                address: String::from("irc.example.com"),
                port: 6697,
                tls: true,
                auto_reconnect: false,
                reconnect_delay_secs: 10,
            },
            identity: IdentityConfig {
                nick: Some(String::from("rustacean")),
                alt_nicks: vec![String::from("rustacean_"), String::from("rustacean__")],
                realname: Some(String::from("A Rust User")),
            },
            ui: UiConfig {
                timestamps: false,
                timestamp_format: String::from("%H:%M:%S"),
                scrollback_lines: 5000,
                show_joins_parts: false,
            },
            scripting: ScriptingConfig {
                enabled: false,
                scripts_dir: Some(String::from("/home/user/.pirc/scripts")),
            },
            plugins: PluginsConfig {
                enabled: false,
                plugins_dir: Some(String::from("/home/user/.pirc/plugins")),
            },
        };

        let toml_str = toml::to_string(&config).expect("serialize to TOML");
        let parsed: ClientConfig = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(parsed.server.address, "irc.example.com");
        assert_eq!(parsed.server.port, 6697);
        assert!(parsed.server.tls);
        assert!(!parsed.server.auto_reconnect);
        assert_eq!(parsed.server.reconnect_delay_secs, 10);
        assert_eq!(parsed.identity.nick.as_deref(), Some("rustacean"));
        assert_eq!(parsed.identity.alt_nicks.len(), 2);
        assert_eq!(parsed.identity.alt_nicks[0], "rustacean_");
        assert_eq!(parsed.identity.alt_nicks[1], "rustacean__");
        assert_eq!(parsed.identity.realname.as_deref(), Some("A Rust User"));
        assert!(!parsed.ui.timestamps);
        assert_eq!(parsed.ui.timestamp_format, "%H:%M:%S");
        assert_eq!(parsed.ui.scrollback_lines, 5000);
        assert!(!parsed.ui.show_joins_parts);
        assert!(!parsed.scripting.enabled);
        assert_eq!(parsed.scripting.scripts_dir.as_deref(), Some("/home/user/.pirc/scripts"));
        assert!(!parsed.plugins.enabled);
        assert_eq!(parsed.plugins.plugins_dir.as_deref(), Some("/home/user/.pirc/plugins"));
    }

    #[test]
    fn toml_deserialize_partial_uses_defaults() {
        let toml_str = r#"
[server]
address = "irc.freenode.net"
port = 6697
tls = true

[identity]
nick = "mybot"
"#;
        let config: ClientConfig = toml::from_str(toml_str).expect("deserialize partial TOML");

        assert_eq!(config.server.address, "irc.freenode.net");
        assert_eq!(config.server.port, 6697);
        assert!(config.server.tls);
        // Remaining server fields should use defaults
        assert!(config.server.auto_reconnect);
        assert_eq!(config.server.reconnect_delay_secs, 5);
        assert_eq!(config.identity.nick.as_deref(), Some("mybot"));
        assert!(config.identity.alt_nicks.is_empty());
        // UI, scripting, plugins should all be defaults
        assert!(config.ui.timestamps);
        assert_eq!(config.ui.scrollback_lines, 1000);
        assert!(config.scripting.enabled);
        assert!(config.plugins.enabled);
    }

    #[test]
    fn toml_empty_string_deserializes_to_defaults() {
        let config: ClientConfig = toml::from_str("").expect("deserialize empty TOML");

        assert_eq!(config.server.address, "localhost");
        assert_eq!(config.server.port, 6667);
        assert!(!config.server.tls);
        assert!(config.identity.nick.is_none());
        assert!(config.ui.timestamps);
        assert_eq!(config.ui.timestamp_format, "%H:%M");
        assert!(config.scripting.enabled);
        assert!(config.plugins.enabled);
    }
}
