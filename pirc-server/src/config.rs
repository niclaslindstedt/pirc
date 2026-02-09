//! Server configuration types and defaults.
//!
//! Defines [`ServerConfig`] and its nested sub-structs for the `pircd` server.
//! All structs derive `Serialize`, `Deserialize`, `Debug`, and `Clone`, and
//! provide sensible defaults via [`Default`].
//!
//! The [`ServerConfig::load`] method handles file-based loading with automatic
//! path discovery, and [`ServerConfig::validate`] ensures all values are within
//! acceptable ranges.

use std::net::IpAddr;
use std::path::Path;

use pirc_common::config::default_server_config_path;
use pirc_common::PircError;
use serde::{Deserialize, Serialize};

/// Top-level server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub network: NetworkConfig,
    pub limits: LimitsConfig,
    pub cluster: ClusterConfig,
    pub motd: MotdConfig,
    pub log_level: String,
}

impl ServerConfig {
    /// Loads configuration from the given path, or auto-discovers from default paths.
    ///
    /// If an explicit path is provided and the file does not exist, returns an error.
    /// If no path is provided, attempts to discover a config file from the default
    /// locations (`$XDG_CONFIG_HOME/pirc/pircd.toml`, `~/.pirc/pircd.toml`,
    /// `/etc/pirc/pircd.toml`). If no config file exists at any location, returns
    /// the default configuration for zero-config startup.
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
            None => match default_server_config_path() {
                Some(p) if p.exists() => p,
                _ => return Ok(Self::default()),
            },
        };

        let contents =
            std::fs::read_to_string(&config_path).map_err(|e| PircError::ConfigError {
                message: format!("failed to read {}: {e}", config_path.display()),
            })?;

        let config: Self = toml::from_str(&contents).map_err(|e| PircError::ConfigError {
            message: format!("failed to parse {}: {e}", config_path.display()),
        })?;

        Ok(config)
    }

    /// Validates the configuration values are within acceptable ranges.
    ///
    /// Returns `Ok(())` if all values are valid, or a `ConfigError` describing
    /// the first invalid value encountered.
    pub fn validate(&self) -> pirc_common::Result<()> {
        if self.network.port == 0 {
            return Err(PircError::ConfigError {
                message: "port must be between 1 and 65535".into(),
            });
        }

        if self.network.bind_address.parse::<IpAddr>().is_err() {
            return Err(PircError::ConfigError {
                message: format!(
                    "bind_address '{}' is not a valid IP address",
                    self.network.bind_address
                ),
            });
        }

        if self.network.max_connections == 0 {
            return Err(PircError::ConfigError {
                message: "max_connections must be greater than 0".into(),
            });
        }

        if self.limits.max_nick_length == 0 || self.limits.max_nick_length > 30 {
            return Err(PircError::ConfigError {
                message: "max_nick_length must be between 1 and 30".into(),
            });
        }

        if self.limits.max_channel_name_length < 2 || self.limits.max_channel_name_length > 50 {
            return Err(PircError::ConfigError {
                message: "max_channel_name_length must be between 2 and 50".into(),
            });
        }

        if self.cluster.enabled && self.cluster.node_id.is_none() {
            return Err(PircError::ConfigError {
                message: "cluster.node_id must be set when cluster is enabled".into(),
            });
        }

        if let Some(ref motd_path) = self.motd.path {
            if !Path::new(motd_path).exists() {
                eprintln!("warning: motd path does not exist: {motd_path}");
            }
        }

        Ok(())
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            limits: LimitsConfig::default(),
            cluster: ClusterConfig::default(),
            motd: MotdConfig::default(),
            log_level: String::from("info"),
        }
    }
}

/// Network binding and connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub bind_address: String,
    pub port: u16,
    pub max_connections: u32,
    pub tls: Option<TlsConfig>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: String::from("0.0.0.0"),
            port: 6667,
            max_connections: 1000,
            tls: None,
        }
    }
}

/// TLS configuration for secure connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
    pub port: u16,
}

/// Resource limits for the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_field_names)]
pub struct LimitsConfig {
    pub max_channels_per_user: u32,
    pub max_nick_length: u32,
    pub max_channel_name_length: u32,
    pub max_message_length: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_channels_per_user: 20,
            max_nick_length: 30,
            max_channel_name_length: 50,
            max_message_length: 512,
        }
    }
}

/// Cluster configuration for multi-node deployments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub node_id: Option<String>,
    pub peers: Vec<String>,
    pub raft_port: Option<u16>,
}

/// Message of the Day configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MotdConfig {
    pub path: Option<String>,
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ---- Load tests ----

    #[test]
    fn load_returns_defaults_when_no_config_exists() {
        let config = ServerConfig::load(None).expect("load defaults");
        assert_eq!(config.network.port, 6667);
        assert_eq!(config.network.bind_address, "0.0.0.0");
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn load_from_explicit_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pircd.toml");
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(
            br#"
log_level = "debug"

[network]
port = 6697
bind_address = "127.0.0.1"
"#,
        )
        .expect("write");

        let config = ServerConfig::load(Some(&path)).expect("load from file");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.network.port, 6697);
        assert_eq!(config.network.bind_address, "127.0.0.1");
        // Defaults for unset fields
        assert_eq!(config.network.max_connections, 1000);
        assert_eq!(config.limits.max_nick_length, 30);
    }

    #[test]
    fn load_from_nonexistent_path_returns_error() {
        let result = ServerConfig::load(Some(Path::new("/tmp/nonexistent_pirc_config.toml")));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("config file not found"));
    }

    #[test]
    fn load_from_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid { toml }}}").expect("write");

        let result = ServerConfig::load(Some(&path));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    // ---- Validate tests ----

    #[test]
    fn validate_defaults_pass() {
        let config = ServerConfig::default();
        config.validate().expect("defaults should be valid");
    }

    #[test]
    fn validate_port_zero() {
        let mut config = ServerConfig::default();
        config.network.port = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("port must be between 1 and 65535"));
    }

    #[test]
    fn validate_invalid_bind_address() {
        let mut config = ServerConfig::default();
        config.network.bind_address = "not-an-ip".into();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("not a valid IP address"));
    }

    #[test]
    fn validate_max_connections_zero() {
        let mut config = ServerConfig::default();
        config.network.max_connections = 0;
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("max_connections must be greater than 0"));
    }

    #[test]
    fn validate_max_nick_length_zero() {
        let mut config = ServerConfig::default();
        config.limits.max_nick_length = 0;
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("max_nick_length must be between 1 and 30"));
    }

    #[test]
    fn validate_max_nick_length_too_large() {
        let mut config = ServerConfig::default();
        config.limits.max_nick_length = 31;
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("max_nick_length must be between 1 and 30"));
    }

    #[test]
    fn validate_max_channel_name_length_too_small() {
        let mut config = ServerConfig::default();
        config.limits.max_channel_name_length = 1;
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("max_channel_name_length must be between 2 and 50"));
    }

    #[test]
    fn validate_max_channel_name_length_too_large() {
        let mut config = ServerConfig::default();
        config.limits.max_channel_name_length = 51;
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("max_channel_name_length must be between 2 and 50"));
    }

    #[test]
    fn validate_cluster_enabled_without_node_id() {
        let mut config = ServerConfig::default();
        config.cluster.enabled = true;
        config.cluster.node_id = None;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("cluster.node_id must be set"));
    }

    #[test]
    fn validate_cluster_enabled_with_node_id() {
        let mut config = ServerConfig::default();
        config.cluster.enabled = true;
        config.cluster.node_id = Some("node-1".into());
        config
            .validate()
            .expect("cluster with node_id should be valid");
    }

    #[test]
    fn validate_ipv6_bind_address() {
        let mut config = ServerConfig::default();
        config.network.bind_address = "::1".into();
        config.validate().expect("IPv6 address should be valid");
    }

    #[test]
    fn validate_motd_nonexistent_path_is_warning_not_error() {
        let mut config = ServerConfig::default();
        config.motd.path = Some("/tmp/nonexistent_pirc_motd.txt".into());
        config
            .validate()
            .expect("nonexistent motd path should warn but not error");
    }

    // ---- Existing default tests ----

    #[test]
    fn server_config_defaults() {
        let config = ServerConfig::default();
        assert_eq!(config.log_level, "info");
        assert_eq!(config.network.bind_address, "0.0.0.0");
        assert_eq!(config.network.port, 6667);
        assert_eq!(config.network.max_connections, 1000);
        assert!(config.network.tls.is_none());
    }

    #[test]
    fn limits_config_defaults() {
        let limits = LimitsConfig::default();
        assert_eq!(limits.max_channels_per_user, 20);
        assert_eq!(limits.max_nick_length, 30);
        assert_eq!(limits.max_channel_name_length, 50);
        assert_eq!(limits.max_message_length, 512);
    }

    #[test]
    fn cluster_config_defaults() {
        let cluster = ClusterConfig::default();
        assert!(!cluster.enabled);
        assert!(cluster.node_id.is_none());
        assert!(cluster.peers.is_empty());
        assert!(cluster.raft_port.is_none());
    }

    #[test]
    fn motd_config_defaults() {
        let motd = MotdConfig::default();
        assert!(motd.path.is_none());
        assert!(motd.text.is_none());
    }

    #[test]
    fn toml_round_trip_defaults() {
        let config = ServerConfig::default();
        let toml_str = toml::to_string(&config).expect("serialize to TOML");
        let parsed: ServerConfig = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(parsed.log_level, config.log_level);
        assert_eq!(parsed.network.bind_address, config.network.bind_address);
        assert_eq!(parsed.network.port, config.network.port);
        assert_eq!(
            parsed.network.max_connections,
            config.network.max_connections
        );
        assert_eq!(
            parsed.limits.max_channels_per_user,
            config.limits.max_channels_per_user
        );
        assert_eq!(parsed.limits.max_nick_length, config.limits.max_nick_length);
        assert_eq!(parsed.cluster.enabled, config.cluster.enabled);
        assert!(parsed.cluster.peers.is_empty());
    }

    #[test]
    fn toml_round_trip_with_all_fields() {
        let config = ServerConfig {
            network: NetworkConfig {
                bind_address: String::from("127.0.0.1"),
                port: 6697,
                max_connections: 500,
                tls: Some(TlsConfig {
                    cert_path: String::from("/etc/pirc/cert.pem"),
                    key_path: String::from("/etc/pirc/key.pem"),
                    port: 6697,
                }),
            },
            limits: LimitsConfig {
                max_channels_per_user: 10,
                max_nick_length: 15,
                max_channel_name_length: 40,
                max_message_length: 1024,
            },
            cluster: ClusterConfig {
                enabled: true,
                node_id: Some(String::from("node-1")),
                peers: vec![String::from("10.0.0.2:7000"), String::from("10.0.0.3:7000")],
                raft_port: Some(7000),
            },
            motd: MotdConfig {
                path: Some(String::from("/etc/pirc/motd.txt")),
                text: Some(String::from("Welcome to pirc!")),
            },
            log_level: String::from("debug"),
        };

        let toml_str = toml::to_string(&config).expect("serialize to TOML");
        let parsed: ServerConfig = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(parsed.network.bind_address, "127.0.0.1");
        assert_eq!(parsed.network.port, 6697);
        assert_eq!(parsed.network.max_connections, 500);
        let tls = parsed.network.tls.as_ref().expect("tls present");
        assert_eq!(tls.cert_path, "/etc/pirc/cert.pem");
        assert_eq!(tls.key_path, "/etc/pirc/key.pem");
        assert_eq!(tls.port, 6697);
        assert_eq!(parsed.limits.max_channels_per_user, 10);
        assert_eq!(parsed.limits.max_nick_length, 15);
        assert!(parsed.cluster.enabled);
        assert_eq!(parsed.cluster.node_id.as_deref(), Some("node-1"));
        assert_eq!(parsed.cluster.peers.len(), 2);
        assert_eq!(parsed.cluster.raft_port, Some(7000));
        assert_eq!(parsed.motd.path.as_deref(), Some("/etc/pirc/motd.txt"));
        assert_eq!(parsed.motd.text.as_deref(), Some("Welcome to pirc!"));
        assert_eq!(parsed.log_level, "debug");
    }

    #[test]
    fn toml_deserialize_partial_uses_defaults() {
        let toml_str = r#"
log_level = "warn"

[network]
port = 6697
"#;
        let config: ServerConfig = toml::from_str(toml_str).expect("deserialize partial TOML");

        assert_eq!(config.log_level, "warn");
        assert_eq!(config.network.port, 6697);
        // Remaining fields should use defaults
        assert_eq!(config.network.bind_address, "0.0.0.0");
        assert_eq!(config.network.max_connections, 1000);
        assert_eq!(config.limits.max_channels_per_user, 20);
        assert!(!config.cluster.enabled);
        assert!(config.motd.path.is_none());
    }

    #[test]
    fn toml_empty_string_deserializes_to_defaults() {
        let config: ServerConfig = toml::from_str("").expect("deserialize empty TOML");

        assert_eq!(config.log_level, "info");
        assert_eq!(config.network.bind_address, "0.0.0.0");
        assert_eq!(config.network.port, 6667);
        assert_eq!(config.limits.max_message_length, 512);
    }
}
