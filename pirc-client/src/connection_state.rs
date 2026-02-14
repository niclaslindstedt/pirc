use std::fmt;

use crate::config::ClientConfig;

/// The current state of the client's connection to the IRC server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected to any server.
    Disconnected,
    /// TCP connection in progress.
    Connecting,
    /// Connected, NICK/USER sent, waiting for RPL_WELCOME.
    Registering,
    /// Fully registered; `server_name` comes from the welcome message.
    Connected { server_name: String },
    /// Disconnected, auto-reconnect in progress.
    Reconnecting { attempt: u32 },
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "Disconnected"),
            ConnectionState::Connecting => write!(f, "Connecting"),
            ConnectionState::Registering => write!(f, "Registering"),
            ConnectionState::Connected { server_name } => write!(f, "Connected ({server_name})"),
            ConnectionState::Reconnecting { attempt } => {
                write!(f, "Reconnecting (attempt {attempt})")
            }
        }
    }
}

/// Error returned when an invalid state transition is attempted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid state transition: {from} -> {to}")]
pub struct TransitionError {
    pub from: String,
    pub to: String,
}

/// Manages the client's connection state with validated transitions.
pub struct ConnectionManager {
    state: ConnectionState,
    server_addr: String,
    nick: String,
    auto_reconnect: bool,
}

impl ConnectionManager {
    /// Create a new connection manager from the client configuration.
    pub fn new(config: &ClientConfig) -> Self {
        let addr = format!("{}:{}", config.server.address, config.server.port);
        let nick = config
            .identity
            .nick
            .clone()
            .unwrap_or_else(|| "pirc_user".to_string());
        Self {
            state: ConnectionState::Disconnected,
            server_addr: addr,
            nick,
            auto_reconnect: config.server.auto_reconnect,
        }
    }

    /// Return the current connection state.
    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    /// Return the server address.
    pub fn server_addr(&self) -> &str {
        &self.server_addr
    }

    /// Return the nick.
    pub fn nick(&self) -> &str {
        &self.nick
    }

    /// Set the nick (e.g. after a NICK command or server-assigned nick).
    pub fn set_nick(&mut self, nick: String) {
        self.nick = nick;
    }

    /// Return whether auto-reconnect is enabled.
    pub fn auto_reconnect(&self) -> bool {
        self.auto_reconnect
    }

    /// Enable or disable auto-reconnect.
    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        self.auto_reconnect = enabled;
    }

    /// Attempt a state transition. Returns an error if the transition is invalid.
    pub fn transition(&mut self, new_state: ConnectionState) -> Result<(), TransitionError> {
        if Self::is_valid_transition(&self.state, &new_state) {
            self.state = new_state;
            Ok(())
        } else {
            Err(TransitionError {
                from: self.state.to_string(),
                to: new_state.to_string(),
            })
        }
    }

    /// Return `true` if the connection is fully registered.
    pub fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected { .. })
    }

    /// Return `true` if the client has completed registration (same as `is_connected`).
    pub fn is_registered(&self) -> bool {
        self.is_connected()
    }

    /// Return the server name if connected.
    pub fn server_name(&self) -> Option<&str> {
        match &self.state {
            ConnectionState::Connected { server_name } => Some(server_name),
            _ => None,
        }
    }

    /// Validate whether a transition from `from` to `to` is allowed.
    fn is_valid_transition(from: &ConnectionState, to: &ConnectionState) -> bool {
        matches!(
            (from, to),
            // Disconnected → Connecting
            (ConnectionState::Disconnected, ConnectionState::Connecting)
            // Connecting → Registering (TCP connected, sending NICK/USER)
            | (ConnectionState::Connecting, ConnectionState::Registering)
            // Connecting → Disconnected (connection failed)
            | (ConnectionState::Connecting, ConnectionState::Disconnected)
            // Registering → Connected (RPL_WELCOME received)
            | (ConnectionState::Registering, ConnectionState::Connected { .. })
            // Registering → Disconnected (registration rejected/timeout)
            | (ConnectionState::Registering, ConnectionState::Disconnected)
            // Connected → Disconnected (connection lost / quit)
            | (ConnectionState::Connected { .. }, ConnectionState::Disconnected)
            // Disconnected → Reconnecting (auto-reconnect enabled)
            | (ConnectionState::Disconnected, ConnectionState::Reconnecting { .. })
            // Reconnecting → Connecting (retry attempt)
            | (ConnectionState::Reconnecting { .. }, ConnectionState::Connecting)
            // Reconnecting → Disconnected (max retries exceeded)
            | (ConnectionState::Reconnecting { .. }, ConnectionState::Disconnected)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ClientConfig {
        ClientConfig::default()
    }

    fn config_with_nick(nick: &str) -> ClientConfig {
        let mut config = ClientConfig::default();
        config.identity.nick = Some(nick.to_string());
        config
    }

    // ── Construction ─────────────────────────────────────────────

    #[test]
    fn new_starts_disconnected() {
        let mgr = ConnectionManager::new(&default_config());
        assert_eq!(*mgr.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn new_uses_config_nick() {
        let mgr = ConnectionManager::new(&config_with_nick("testuser"));
        assert_eq!(mgr.nick(), "testuser");
    }

    #[test]
    fn new_uses_config_address() {
        let config = default_config();
        let mgr = ConnectionManager::new(&config);
        assert_eq!(mgr.server_addr(), "localhost:6667");
    }

    #[test]
    fn new_uses_auto_reconnect_from_config() {
        let mgr = ConnectionManager::new(&default_config());
        assert!(mgr.auto_reconnect());
    }

    #[test]
    fn new_auto_reconnect_disabled() {
        let mut config = default_config();
        config.server.auto_reconnect = false;
        let mgr = ConnectionManager::new(&config);
        assert!(!mgr.auto_reconnect());
    }

    // ── State queries ────────────────────────────────────────────

    #[test]
    fn is_connected_when_connected() {
        let mut mgr = ConnectionManager::new(&config_with_nick("user"));
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "irc.test.net".into(),
        })
        .unwrap();
        assert!(mgr.is_connected());
        assert!(mgr.is_registered());
    }

    #[test]
    fn is_connected_false_when_disconnected() {
        let mgr = ConnectionManager::new(&default_config());
        assert!(!mgr.is_connected());
        assert!(!mgr.is_registered());
    }

    #[test]
    fn server_name_when_connected() {
        let mut mgr = ConnectionManager::new(&config_with_nick("user"));
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "irc.test.net".into(),
        })
        .unwrap();
        assert_eq!(mgr.server_name(), Some("irc.test.net"));
    }

    #[test]
    fn server_name_none_when_not_connected() {
        let mgr = ConnectionManager::new(&default_config());
        assert_eq!(mgr.server_name(), None);
    }

    #[test]
    fn set_nick_updates_nick() {
        let mut mgr = ConnectionManager::new(&config_with_nick("old"));
        mgr.set_nick("new".into());
        assert_eq!(mgr.nick(), "new");
    }

    #[test]
    fn set_auto_reconnect() {
        let mut mgr = ConnectionManager::new(&default_config());
        assert!(mgr.auto_reconnect());
        mgr.set_auto_reconnect(false);
        assert!(!mgr.auto_reconnect());
        mgr.set_auto_reconnect(true);
        assert!(mgr.auto_reconnect());
    }

    // ── Valid transitions ────────────────────────────────────────

    #[test]
    fn disconnected_to_connecting() {
        let mut mgr = ConnectionManager::new(&default_config());
        assert!(mgr.transition(ConnectionState::Connecting).is_ok());
        assert_eq!(*mgr.state(), ConnectionState::Connecting);
    }

    #[test]
    fn connecting_to_registering() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        assert!(mgr.transition(ConnectionState::Registering).is_ok());
        assert_eq!(*mgr.state(), ConnectionState::Registering);
    }

    #[test]
    fn connecting_to_disconnected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        assert!(mgr.transition(ConnectionState::Disconnected).is_ok());
        assert_eq!(*mgr.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn registering_to_connected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        assert!(mgr
            .transition(ConnectionState::Connected {
                server_name: "irc.test.net".into(),
            })
            .is_ok());
        assert_eq!(
            *mgr.state(),
            ConnectionState::Connected {
                server_name: "irc.test.net".into()
            }
        );
    }

    #[test]
    fn registering_to_disconnected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        assert!(mgr.transition(ConnectionState::Disconnected).is_ok());
    }

    #[test]
    fn connected_to_disconnected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s".into(),
        })
        .unwrap();
        assert!(mgr.transition(ConnectionState::Disconnected).is_ok());
    }

    #[test]
    fn disconnected_to_reconnecting() {
        let mut mgr = ConnectionManager::new(&default_config());
        assert!(mgr
            .transition(ConnectionState::Reconnecting { attempt: 1 })
            .is_ok());
        assert_eq!(*mgr.state(), ConnectionState::Reconnecting { attempt: 1 });
    }

    #[test]
    fn reconnecting_to_connecting() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Reconnecting { attempt: 1 })
            .unwrap();
        assert!(mgr.transition(ConnectionState::Connecting).is_ok());
    }

    #[test]
    fn reconnecting_to_disconnected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Reconnecting { attempt: 1 })
            .unwrap();
        assert!(mgr.transition(ConnectionState::Disconnected).is_ok());
    }

    // ── Invalid transitions ──────────────────────────────────────

    #[test]
    fn disconnected_to_connected_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        let err = mgr
            .transition(ConnectionState::Connected {
                server_name: "s".into(),
            })
            .unwrap_err();
        assert!(err.to_string().contains("invalid state transition"));
    }

    #[test]
    fn disconnected_to_registering_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        let err = mgr.transition(ConnectionState::Registering).unwrap_err();
        assert!(err.to_string().contains("Disconnected"));
        assert!(err.to_string().contains("Registering"));
    }

    #[test]
    fn connecting_to_connected_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        assert!(mgr
            .transition(ConnectionState::Connected {
                server_name: "s".into(),
            })
            .is_err());
    }

    #[test]
    fn connecting_to_reconnecting_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        assert!(mgr
            .transition(ConnectionState::Reconnecting { attempt: 1 })
            .is_err());
    }

    #[test]
    fn connected_to_connecting_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s".into(),
        })
        .unwrap();
        assert!(mgr.transition(ConnectionState::Connecting).is_err());
    }

    #[test]
    fn connected_to_registering_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s".into(),
        })
        .unwrap();
        assert!(mgr.transition(ConnectionState::Registering).is_err());
    }

    #[test]
    fn connected_to_reconnecting_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s".into(),
        })
        .unwrap();
        assert!(mgr
            .transition(ConnectionState::Reconnecting { attempt: 1 })
            .is_err());
    }

    #[test]
    fn registering_to_reconnecting_invalid() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        assert!(mgr
            .transition(ConnectionState::Reconnecting { attempt: 1 })
            .is_err());
    }

    // ── Full lifecycle ───────────────────────────────────────────

    #[test]
    fn full_connect_lifecycle() {
        let mut mgr = ConnectionManager::new(&config_with_nick("user"));
        assert_eq!(*mgr.state(), ConnectionState::Disconnected);

        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "irc.example.com".into(),
        })
        .unwrap();

        assert!(mgr.is_connected());
        assert_eq!(mgr.server_name(), Some("irc.example.com"));

        mgr.transition(ConnectionState::Disconnected).unwrap();
        assert!(!mgr.is_connected());
    }

    #[test]
    fn reconnect_lifecycle() {
        let mut mgr = ConnectionManager::new(&config_with_nick("user"));

        // Connect fully
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s".into(),
        })
        .unwrap();

        // Disconnect
        mgr.transition(ConnectionState::Disconnected).unwrap();

        // Reconnect cycle
        mgr.transition(ConnectionState::Reconnecting { attempt: 1 })
            .unwrap();
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Registering).unwrap();
        mgr.transition(ConnectionState::Connected {
            server_name: "s2".into(),
        })
        .unwrap();

        assert_eq!(mgr.server_name(), Some("s2"));
    }

    #[test]
    fn reconnect_max_retries_gives_up() {
        let mut mgr = ConnectionManager::new(&default_config());

        // Reconnect attempt that gives up
        mgr.transition(ConnectionState::Reconnecting { attempt: 5 })
            .unwrap();
        mgr.transition(ConnectionState::Disconnected).unwrap();
        assert_eq!(*mgr.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn connection_failure_returns_to_disconnected() {
        let mut mgr = ConnectionManager::new(&default_config());
        mgr.transition(ConnectionState::Connecting).unwrap();
        mgr.transition(ConnectionState::Disconnected).unwrap();
        assert_eq!(*mgr.state(), ConnectionState::Disconnected);
    }

    // ── Display ──────────────────────────────────────────────────

    #[test]
    fn display_disconnected() {
        assert_eq!(ConnectionState::Disconnected.to_string(), "Disconnected");
    }

    #[test]
    fn display_connecting() {
        assert_eq!(ConnectionState::Connecting.to_string(), "Connecting");
    }

    #[test]
    fn display_registering() {
        assert_eq!(ConnectionState::Registering.to_string(), "Registering");
    }

    #[test]
    fn display_connected() {
        let state = ConnectionState::Connected {
            server_name: "irc.test.net".into(),
        };
        assert_eq!(state.to_string(), "Connected (irc.test.net)");
    }

    #[test]
    fn display_reconnecting() {
        let state = ConnectionState::Reconnecting { attempt: 3 };
        assert_eq!(state.to_string(), "Reconnecting (attempt 3)");
    }

    // ── TransitionError ──────────────────────────────────────────

    #[test]
    fn transition_error_message() {
        let err = TransitionError {
            from: "Disconnected".into(),
            to: "Connected (s)".into(),
        };
        assert_eq!(
            err.to_string(),
            "invalid state transition: Disconnected -> Connected (s)"
        );
    }
}
