//! Server configuration types and defaults.
//!
//! Defines [`ServerConfig`] and its nested sub-structs for the `pircd` server.
//! All structs derive `Serialize`, `Deserialize`, `Debug`, and `Clone`, and
//! provide sensible defaults via [`Default`].

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub node_id: Option<String>,
    pub peers: Vec<String>,
    pub raft_port: Option<u16>,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: None,
            peers: Vec::new(),
            raft_port: None,
        }
    }
}

/// Message of the Day configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MotdConfig {
    pub path: Option<String>,
    pub text: Option<String>,
}

impl Default for MotdConfig {
    fn default() -> Self {
        Self {
            path: None,
            text: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(parsed.network.max_connections, config.network.max_connections);
        assert_eq!(parsed.limits.max_channels_per_user, config.limits.max_channels_per_user);
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
                peers: vec![
                    String::from("10.0.0.2:7000"),
                    String::from("10.0.0.3:7000"),
                ],
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
