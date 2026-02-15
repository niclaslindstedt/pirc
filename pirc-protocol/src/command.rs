use std::fmt;

/// Subcommands for the `PIRC` extension command namespace.
///
/// The PIRC namespace groups pirc-specific protocol extensions that go beyond
/// standard IRC. Each subcommand has its own wire-format keyword that appears
/// as the first parameter after `PIRC`.
///
/// Subcommands are organized into categories:
/// - **Core**: `VERSION`, `CAP`
/// - **Encryption**: `KEYEXCHANGE`, `KEYEXCHANGE-ACK`, `KEYEXCHANGE-COMPLETE`,
///   `FINGERPRINT`, `ENCRYPTED`
/// - **Cluster** (server-to-server): `CLUSTER JOIN`, `CLUSTER WELCOME`,
///   `CLUSTER SYNC`, `CLUSTER HEARTBEAT`, `CLUSTER MIGRATE`, `CLUSTER RAFT`
/// - **P2P** (signaling): `P2P OFFER`, `P2P ANSWER`, `P2P ICE`,
///   `P2P ESTABLISHED`, `P2P FAILED`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PircSubcommand {
    // ---- Core ----
    /// Protocol version announcement: `PIRC VERSION <version>`.
    Version,
    /// Capability announcement (for future use): `PIRC CAP <capability> [...]`.
    Cap,

    // ---- Encryption ----
    /// Initiate key exchange: `PIRC KEYEXCHANGE <target> <public-key-data>`.
    KeyExchange,
    /// Acknowledge key exchange: `PIRC KEYEXCHANGE-ACK <target> <public-key-data>`.
    KeyExchangeAck,
    /// Key exchange completed: `PIRC KEYEXCHANGE-COMPLETE <target>`.
    KeyExchangeComplete,
    /// Share identity fingerprint: `PIRC FINGERPRINT <target> <fingerprint>`.
    Fingerprint,
    /// E2E encrypted message wrapper: `PIRC ENCRYPTED <target> <encrypted-payload>`.
    Encrypted,

    // ---- Cluster (server-to-server) ----
    /// Server requests to join cluster: `PIRC CLUSTER JOIN <invite-key>`.
    ClusterJoin,
    /// Cluster accepts new server: `PIRC CLUSTER WELCOME <server-id> <cluster-config>`.
    ClusterWelcome,
    /// State synchronization: `PIRC CLUSTER SYNC <state-data>`.
    ClusterSync,
    /// Cluster keepalive: `PIRC CLUSTER HEARTBEAT <server-id>`.
    ClusterHeartbeat,
    /// User migration notification: `PIRC CLUSTER MIGRATE <user-id> <target-server>`.
    ClusterMigrate,
    /// Raft consensus protocol message: `PIRC CLUSTER RAFT <raft-message>`.
    ClusterRaft,
    /// Query cluster status: `PIRC CLUSTER STATUS`.
    ClusterStatus,
    /// List cluster members: `PIRC CLUSTER MEMBERS`.
    ClusterMembers,

    // ---- Invite-key management ----
    /// Generate an invite key: `PIRC INVITE-KEY GENERATE [ttl]`.
    InviteKeyGenerate,
    /// List active invite keys: `PIRC INVITE-KEY LIST`.
    InviteKeyList,
    /// Revoke an invite key: `PIRC INVITE-KEY REVOKE <token>`.
    InviteKeyRevoke,

    // ---- Network ----
    /// Network info query: `PIRC NETWORK INFO`.
    NetworkInfo,

    // ---- P2P (signaling) ----
    /// P2P connection offer: `PIRC P2P OFFER <target> <sdp-or-signal-data>`.
    P2pOffer,
    /// P2P connection answer: `PIRC P2P ANSWER <target> <sdp-or-signal-data>`.
    P2pAnswer,
    /// ICE/NAT traversal candidate: `PIRC P2P ICE <target> <candidate-data>`.
    P2pIce,
    /// P2P connection established: `PIRC P2P ESTABLISHED <target>`.
    P2pEstablished,
    /// P2P connection failed: `PIRC P2P FAILED <target> <reason>`.
    P2pFailed,
}

impl PircSubcommand {
    /// Returns the wire-format keyword(s) for this subcommand.
    ///
    /// For namespaced subcommands (Cluster, P2P), returns the full
    /// compound keyword (e.g., `"CLUSTER JOIN"`, `"P2P OFFER"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Version => "VERSION",
            Self::Cap => "CAP",
            // Encryption
            Self::KeyExchange => "KEYEXCHANGE",
            Self::KeyExchangeAck => "KEYEXCHANGE-ACK",
            Self::KeyExchangeComplete => "KEYEXCHANGE-COMPLETE",
            Self::Fingerprint => "FINGERPRINT",
            Self::Encrypted => "ENCRYPTED",
            // Cluster
            Self::ClusterJoin => "CLUSTER JOIN",
            Self::ClusterWelcome => "CLUSTER WELCOME",
            Self::ClusterSync => "CLUSTER SYNC",
            Self::ClusterHeartbeat => "CLUSTER HEARTBEAT",
            Self::ClusterMigrate => "CLUSTER MIGRATE",
            Self::ClusterRaft => "CLUSTER RAFT",
            Self::ClusterStatus => "CLUSTER STATUS",
            Self::ClusterMembers => "CLUSTER MEMBERS",
            // Invite-key
            Self::InviteKeyGenerate => "INVITE-KEY GENERATE",
            Self::InviteKeyList => "INVITE-KEY LIST",
            Self::InviteKeyRevoke => "INVITE-KEY REVOKE",
            // Network
            Self::NetworkInfo => "NETWORK INFO",
            // P2P
            Self::P2pOffer => "P2P OFFER",
            Self::P2pAnswer => "P2P ANSWER",
            Self::P2pIce => "P2P ICE",
            Self::P2pEstablished => "P2P ESTABLISHED",
            Self::P2pFailed => "P2P FAILED",
        }
    }

    /// Parses a top-level subcommand keyword into a `PircSubcommand`.
    ///
    /// For flat subcommands (`VERSION`, `CAP`, encryption keywords), returns
    /// the variant directly. For namespaced subcommands (`CLUSTER`, `P2P`),
    /// returns `None` — callers should use [`from_namespace`] instead.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "VERSION" => Some(Self::Version),
            "CAP" => Some(Self::Cap),
            "KEYEXCHANGE" => Some(Self::KeyExchange),
            "KEYEXCHANGE-ACK" => Some(Self::KeyExchangeAck),
            "KEYEXCHANGE-COMPLETE" => Some(Self::KeyExchangeComplete),
            "FINGERPRINT" => Some(Self::Fingerprint),
            "ENCRYPTED" => Some(Self::Encrypted),
            _ => None,
        }
    }

    /// Parses a namespaced subcommand from a namespace prefix and inner keyword.
    ///
    /// For example, `from_namespace("CLUSTER", "JOIN")` returns
    /// `Some(PircSubcommand::ClusterJoin)`.
    pub fn from_namespace(namespace: &str, inner: &str) -> Option<Self> {
        match namespace {
            "CLUSTER" => match inner {
                "JOIN" => Some(Self::ClusterJoin),
                "WELCOME" => Some(Self::ClusterWelcome),
                "SYNC" => Some(Self::ClusterSync),
                "HEARTBEAT" => Some(Self::ClusterHeartbeat),
                "MIGRATE" => Some(Self::ClusterMigrate),
                "RAFT" => Some(Self::ClusterRaft),
                "STATUS" => Some(Self::ClusterStatus),
                "MEMBERS" => Some(Self::ClusterMembers),
                _ => None,
            },
            "INVITE-KEY" => match inner {
                "GENERATE" => Some(Self::InviteKeyGenerate),
                "LIST" => Some(Self::InviteKeyList),
                "REVOKE" => Some(Self::InviteKeyRevoke),
                _ => None,
            },
            "NETWORK" => match inner {
                "INFO" => Some(Self::NetworkInfo),
                _ => None,
            },
            "P2P" => match inner {
                "OFFER" => Some(Self::P2pOffer),
                "ANSWER" => Some(Self::P2pAnswer),
                "ICE" => Some(Self::P2pIce),
                "ESTABLISHED" => Some(Self::P2pEstablished),
                "FAILED" => Some(Self::P2pFailed),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns `true` if this subcommand uses a namespace prefix.
    pub fn is_namespaced(&self) -> bool {
        matches!(
            self,
            Self::ClusterJoin
                | Self::ClusterWelcome
                | Self::ClusterSync
                | Self::ClusterHeartbeat
                | Self::ClusterMigrate
                | Self::ClusterRaft
                | Self::ClusterStatus
                | Self::ClusterMembers
                | Self::InviteKeyGenerate
                | Self::InviteKeyList
                | Self::InviteKeyRevoke
                | Self::NetworkInfo
                | Self::P2pOffer
                | Self::P2pAnswer
                | Self::P2pIce
                | Self::P2pEstablished
                | Self::P2pFailed
        )
    }
}

impl fmt::Display for PircSubcommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// IRC-style protocol commands supported by pirc.
///
/// Each variant represents a distinct protocol action. The wire format uses
/// the uppercase keyword (e.g., `PRIVMSG`, `JOIN`) as the command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Set or change nickname.
    Nick,
    /// Register username and realname: `USER <username> <mode> <unused> :<realname>`.
    User,
    /// Join a channel.
    Join,
    /// Leave a channel.
    Part,
    /// Send a message to a channel or user.
    Privmsg,
    /// Send a notice (no auto-reply expected).
    Notice,
    /// Disconnect from the server.
    Quit,
    /// Kick a user from a channel.
    Kick,
    /// Ban a user from a channel (pirc extension).
    Ban,
    /// Set channel or user modes.
    Mode,
    /// Get or set a channel topic.
    Topic,
    /// Query user information.
    Whois,
    /// List channels.
    List,
    /// Query channel member names.
    Names,
    /// Invite a user to a channel.
    Invite,
    /// Set away status.
    Away,
    /// Authenticate as an IRC operator.
    Oper,
    /// Forcibly disconnect a user from the server.
    Kill,
    /// Shut down the server (operator only).
    Die,
    /// Restart the server (operator only).
    Restart,
    /// Send a message to all operators.
    Wallops,
    /// Request the message of the day.
    Motd,
    /// Connection keepalive request.
    Ping,
    /// Connection keepalive response.
    Pong,
    /// Server error message.
    Error,
    /// A numeric reply code (e.g., 001 for `RPL_WELCOME`).
    Numeric(u16),
    /// Pirc extension command with a subcommand (e.g., `PIRC VERSION 1.0`).
    Pirc(PircSubcommand),
}

impl Command {
    /// Returns the wire-format string for this command.
    ///
    /// For named commands, returns the uppercase keyword. For numeric
    /// replies, returns the zero-padded three-digit code.
    pub fn as_str(&self) -> String {
        match self {
            Self::Nick => "NICK".to_owned(),
            Self::User => "USER".to_owned(),
            Self::Join => "JOIN".to_owned(),
            Self::Part => "PART".to_owned(),
            Self::Privmsg => "PRIVMSG".to_owned(),
            Self::Notice => "NOTICE".to_owned(),
            Self::Quit => "QUIT".to_owned(),
            Self::Kick => "KICK".to_owned(),
            Self::Ban => "BAN".to_owned(),
            Self::Mode => "MODE".to_owned(),
            Self::Topic => "TOPIC".to_owned(),
            Self::Whois => "WHOIS".to_owned(),
            Self::List => "LIST".to_owned(),
            Self::Names => "NAMES".to_owned(),
            Self::Invite => "INVITE".to_owned(),
            Self::Away => "AWAY".to_owned(),
            Self::Oper => "OPER".to_owned(),
            Self::Kill => "KILL".to_owned(),
            Self::Die => "DIE".to_owned(),
            Self::Restart => "RESTART".to_owned(),
            Self::Wallops => "WALLOPS".to_owned(),
            Self::Motd => "MOTD".to_owned(),
            Self::Ping => "PING".to_owned(),
            Self::Pong => "PONG".to_owned(),
            Self::Error => "ERROR".to_owned(),
            Self::Numeric(code) => format!("{code:03}"),
            Self::Pirc(_) => "PIRC".to_owned(),
        }
    }
}

impl Command {
    /// Parses a command string (uppercase keyword or 3-digit numeric) into a `Command`.
    ///
    /// Returns `None` if the string is not a recognized command keyword and is
    /// not a valid 3-digit numeric code.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "NICK" => Some(Self::Nick),
            "USER" => Some(Self::User),
            "JOIN" => Some(Self::Join),
            "PART" => Some(Self::Part),
            "PRIVMSG" => Some(Self::Privmsg),
            "NOTICE" => Some(Self::Notice),
            "QUIT" => Some(Self::Quit),
            "KICK" => Some(Self::Kick),
            "BAN" => Some(Self::Ban),
            "MODE" => Some(Self::Mode),
            "TOPIC" => Some(Self::Topic),
            "WHOIS" => Some(Self::Whois),
            "LIST" => Some(Self::List),
            "NAMES" => Some(Self::Names),
            "INVITE" => Some(Self::Invite),
            "AWAY" => Some(Self::Away),
            "OPER" => Some(Self::Oper),
            "KILL" => Some(Self::Kill),
            "DIE" => Some(Self::Die),
            "RESTART" => Some(Self::Restart),
            "WALLOPS" => Some(Self::Wallops),
            "MOTD" => Some(Self::Motd),
            "PING" => Some(Self::Ping),
            "PONG" => Some(Self::Pong),
            "ERROR" => Some(Self::Error),
            _ => {
                // Try parsing as a 3-digit numeric code
                if s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()) {
                    s.parse::<u16>().ok().map(Self::Numeric)
                } else {
                    None
                }
            }
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_as_str() {
        assert_eq!(Command::Nick.as_str(), "NICK");
        assert_eq!(Command::User.as_str(), "USER");
        assert_eq!(Command::Join.as_str(), "JOIN");
        assert_eq!(Command::Part.as_str(), "PART");
        assert_eq!(Command::Privmsg.as_str(), "PRIVMSG");
        assert_eq!(Command::Notice.as_str(), "NOTICE");
        assert_eq!(Command::Quit.as_str(), "QUIT");
        assert_eq!(Command::Kick.as_str(), "KICK");
        assert_eq!(Command::Ban.as_str(), "BAN");
        assert_eq!(Command::Mode.as_str(), "MODE");
        assert_eq!(Command::Topic.as_str(), "TOPIC");
        assert_eq!(Command::Whois.as_str(), "WHOIS");
        assert_eq!(Command::List.as_str(), "LIST");
        assert_eq!(Command::Names.as_str(), "NAMES");
        assert_eq!(Command::Invite.as_str(), "INVITE");
        assert_eq!(Command::Away.as_str(), "AWAY");
        assert_eq!(Command::Oper.as_str(), "OPER");
        assert_eq!(Command::Kill.as_str(), "KILL");
        assert_eq!(Command::Die.as_str(), "DIE");
        assert_eq!(Command::Restart.as_str(), "RESTART");
        assert_eq!(Command::Wallops.as_str(), "WALLOPS");
        assert_eq!(Command::Motd.as_str(), "MOTD");
        assert_eq!(Command::Ping.as_str(), "PING");
        assert_eq!(Command::Pong.as_str(), "PONG");
        assert_eq!(Command::Error.as_str(), "ERROR");
        assert_eq!(Command::Pirc(PircSubcommand::Version).as_str(), "PIRC");
        assert_eq!(Command::Pirc(PircSubcommand::Cap).as_str(), "PIRC");
    }

    #[test]
    fn numeric_command_as_str_zero_padded() {
        assert_eq!(Command::Numeric(1).as_str(), "001");
        assert_eq!(Command::Numeric(2).as_str(), "002");
        assert_eq!(Command::Numeric(3).as_str(), "003");
        assert_eq!(Command::Numeric(353).as_str(), "353");
        assert_eq!(Command::Numeric(433).as_str(), "433");
    }

    #[test]
    fn command_display() {
        assert_eq!(Command::Privmsg.to_string(), "PRIVMSG");
        assert_eq!(Command::Numeric(1).to_string(), "001");
    }

    #[test]
    fn command_equality() {
        assert_eq!(Command::Nick, Command::Nick);
        assert_eq!(Command::Numeric(1), Command::Numeric(1));
        assert_ne!(Command::Nick, Command::Join);
        assert_ne!(Command::Numeric(1), Command::Numeric(2));
        assert_ne!(Command::Nick, Command::Numeric(1));
    }

    #[test]
    fn command_clone() {
        let cmd = Command::Privmsg;
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn numeric_command_clone() {
        let cmd = Command::Numeric(353);
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn command_user_round_trip() {
        assert_eq!(Command::from_keyword("USER"), Some(Command::User));
        assert_eq!(Command::User.as_str(), "USER");
        assert_eq!(Command::User.to_string(), "USER");
    }

    // ---- PircSubcommand: core ----

    #[test]
    fn pirc_subcommand_as_str() {
        assert_eq!(PircSubcommand::Version.as_str(), "VERSION");
        assert_eq!(PircSubcommand::Cap.as_str(), "CAP");
    }

    #[test]
    fn pirc_subcommand_from_keyword() {
        assert_eq!(
            PircSubcommand::from_keyword("VERSION"),
            Some(PircSubcommand::Version)
        );
        assert_eq!(
            PircSubcommand::from_keyword("CAP"),
            Some(PircSubcommand::Cap)
        );
        assert_eq!(PircSubcommand::from_keyword("UNKNOWN"), None);
    }

    #[test]
    fn pirc_subcommand_display() {
        assert_eq!(PircSubcommand::Version.to_string(), "VERSION");
        assert_eq!(PircSubcommand::Cap.to_string(), "CAP");
    }

    #[test]
    fn pirc_command_equality() {
        assert_eq!(
            Command::Pirc(PircSubcommand::Version),
            Command::Pirc(PircSubcommand::Version)
        );
        assert_ne!(
            Command::Pirc(PircSubcommand::Version),
            Command::Pirc(PircSubcommand::Cap)
        );
    }

    #[test]
    fn command_debug() {
        let debug = format!("{:?}", Command::Privmsg);
        assert_eq!(debug, "Privmsg");

        let debug = format!("{:?}", Command::Numeric(1));
        assert!(debug.contains("Numeric"));
        assert!(debug.contains("1"));
    }

    // ---- PircSubcommand: encryption ----

    #[test]
    fn pirc_encryption_subcommand_as_str() {
        assert_eq!(PircSubcommand::KeyExchange.as_str(), "KEYEXCHANGE");
        assert_eq!(PircSubcommand::KeyExchangeAck.as_str(), "KEYEXCHANGE-ACK");
        assert_eq!(
            PircSubcommand::KeyExchangeComplete.as_str(),
            "KEYEXCHANGE-COMPLETE"
        );
        assert_eq!(PircSubcommand::Fingerprint.as_str(), "FINGERPRINT");
        assert_eq!(PircSubcommand::Encrypted.as_str(), "ENCRYPTED");
    }

    #[test]
    fn pirc_encryption_subcommand_from_keyword() {
        assert_eq!(
            PircSubcommand::from_keyword("KEYEXCHANGE"),
            Some(PircSubcommand::KeyExchange)
        );
        assert_eq!(
            PircSubcommand::from_keyword("KEYEXCHANGE-ACK"),
            Some(PircSubcommand::KeyExchangeAck)
        );
        assert_eq!(
            PircSubcommand::from_keyword("KEYEXCHANGE-COMPLETE"),
            Some(PircSubcommand::KeyExchangeComplete)
        );
        assert_eq!(
            PircSubcommand::from_keyword("FINGERPRINT"),
            Some(PircSubcommand::Fingerprint)
        );
        assert_eq!(
            PircSubcommand::from_keyword("ENCRYPTED"),
            Some(PircSubcommand::Encrypted)
        );
    }

    #[test]
    fn pirc_encryption_subcommand_display() {
        assert_eq!(PircSubcommand::KeyExchange.to_string(), "KEYEXCHANGE");
        assert_eq!(
            PircSubcommand::KeyExchangeAck.to_string(),
            "KEYEXCHANGE-ACK"
        );
        assert_eq!(
            PircSubcommand::KeyExchangeComplete.to_string(),
            "KEYEXCHANGE-COMPLETE"
        );
        assert_eq!(PircSubcommand::Fingerprint.to_string(), "FINGERPRINT");
        assert_eq!(PircSubcommand::Encrypted.to_string(), "ENCRYPTED");
    }

    #[test]
    fn pirc_encryption_not_namespaced() {
        assert!(!PircSubcommand::KeyExchange.is_namespaced());
        assert!(!PircSubcommand::KeyExchangeAck.is_namespaced());
        assert!(!PircSubcommand::KeyExchangeComplete.is_namespaced());
        assert!(!PircSubcommand::Fingerprint.is_namespaced());
        assert!(!PircSubcommand::Encrypted.is_namespaced());
    }

    // ---- PircSubcommand: cluster ----

    #[test]
    fn pirc_cluster_subcommand_as_str() {
        assert_eq!(PircSubcommand::ClusterJoin.as_str(), "CLUSTER JOIN");
        assert_eq!(PircSubcommand::ClusterWelcome.as_str(), "CLUSTER WELCOME");
        assert_eq!(PircSubcommand::ClusterSync.as_str(), "CLUSTER SYNC");
        assert_eq!(
            PircSubcommand::ClusterHeartbeat.as_str(),
            "CLUSTER HEARTBEAT"
        );
        assert_eq!(PircSubcommand::ClusterMigrate.as_str(), "CLUSTER MIGRATE");
        assert_eq!(PircSubcommand::ClusterRaft.as_str(), "CLUSTER RAFT");
    }

    #[test]
    fn pirc_cluster_subcommand_from_namespace() {
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "JOIN"),
            Some(PircSubcommand::ClusterJoin)
        );
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "WELCOME"),
            Some(PircSubcommand::ClusterWelcome)
        );
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "SYNC"),
            Some(PircSubcommand::ClusterSync)
        );
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "HEARTBEAT"),
            Some(PircSubcommand::ClusterHeartbeat)
        );
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "MIGRATE"),
            Some(PircSubcommand::ClusterMigrate)
        );
        assert_eq!(
            PircSubcommand::from_namespace("CLUSTER", "RAFT"),
            Some(PircSubcommand::ClusterRaft)
        );
        assert_eq!(PircSubcommand::from_namespace("CLUSTER", "UNKNOWN"), None);
    }

    #[test]
    fn pirc_cluster_subcommand_display() {
        assert_eq!(PircSubcommand::ClusterJoin.to_string(), "CLUSTER JOIN");
        assert_eq!(
            PircSubcommand::ClusterWelcome.to_string(),
            "CLUSTER WELCOME"
        );
        assert_eq!(PircSubcommand::ClusterSync.to_string(), "CLUSTER SYNC");
        assert_eq!(
            PircSubcommand::ClusterHeartbeat.to_string(),
            "CLUSTER HEARTBEAT"
        );
        assert_eq!(
            PircSubcommand::ClusterMigrate.to_string(),
            "CLUSTER MIGRATE"
        );
        assert_eq!(PircSubcommand::ClusterRaft.to_string(), "CLUSTER RAFT");
    }

    #[test]
    fn pirc_cluster_is_namespaced() {
        assert!(PircSubcommand::ClusterJoin.is_namespaced());
        assert!(PircSubcommand::ClusterWelcome.is_namespaced());
        assert!(PircSubcommand::ClusterSync.is_namespaced());
        assert!(PircSubcommand::ClusterHeartbeat.is_namespaced());
        assert!(PircSubcommand::ClusterMigrate.is_namespaced());
        assert!(PircSubcommand::ClusterRaft.is_namespaced());
    }

    // ---- PircSubcommand: P2P ----

    #[test]
    fn pirc_p2p_subcommand_as_str() {
        assert_eq!(PircSubcommand::P2pOffer.as_str(), "P2P OFFER");
        assert_eq!(PircSubcommand::P2pAnswer.as_str(), "P2P ANSWER");
        assert_eq!(PircSubcommand::P2pIce.as_str(), "P2P ICE");
        assert_eq!(PircSubcommand::P2pEstablished.as_str(), "P2P ESTABLISHED");
        assert_eq!(PircSubcommand::P2pFailed.as_str(), "P2P FAILED");
    }

    #[test]
    fn pirc_p2p_subcommand_from_namespace() {
        assert_eq!(
            PircSubcommand::from_namespace("P2P", "OFFER"),
            Some(PircSubcommand::P2pOffer)
        );
        assert_eq!(
            PircSubcommand::from_namespace("P2P", "ANSWER"),
            Some(PircSubcommand::P2pAnswer)
        );
        assert_eq!(
            PircSubcommand::from_namespace("P2P", "ICE"),
            Some(PircSubcommand::P2pIce)
        );
        assert_eq!(
            PircSubcommand::from_namespace("P2P", "ESTABLISHED"),
            Some(PircSubcommand::P2pEstablished)
        );
        assert_eq!(
            PircSubcommand::from_namespace("P2P", "FAILED"),
            Some(PircSubcommand::P2pFailed)
        );
        assert_eq!(PircSubcommand::from_namespace("P2P", "UNKNOWN"), None);
    }

    #[test]
    fn pirc_p2p_subcommand_display() {
        assert_eq!(PircSubcommand::P2pOffer.to_string(), "P2P OFFER");
        assert_eq!(PircSubcommand::P2pAnswer.to_string(), "P2P ANSWER");
        assert_eq!(PircSubcommand::P2pIce.to_string(), "P2P ICE");
        assert_eq!(
            PircSubcommand::P2pEstablished.to_string(),
            "P2P ESTABLISHED"
        );
        assert_eq!(PircSubcommand::P2pFailed.to_string(), "P2P FAILED");
    }

    #[test]
    fn pirc_p2p_is_namespaced() {
        assert!(PircSubcommand::P2pOffer.is_namespaced());
        assert!(PircSubcommand::P2pAnswer.is_namespaced());
        assert!(PircSubcommand::P2pIce.is_namespaced());
        assert!(PircSubcommand::P2pEstablished.is_namespaced());
        assert!(PircSubcommand::P2pFailed.is_namespaced());
    }

    // ---- PircSubcommand: unknown namespace ----

    #[test]
    fn pirc_unknown_namespace() {
        assert_eq!(PircSubcommand::from_namespace("FOOBAR", "JOIN"), None);
    }

    // ---- Operator commands: from_keyword ----

    #[test]
    fn command_oper_from_keyword() {
        assert_eq!(Command::from_keyword("OPER"), Some(Command::Oper));
    }

    #[test]
    fn command_kill_from_keyword() {
        assert_eq!(Command::from_keyword("KILL"), Some(Command::Kill));
    }

    #[test]
    fn command_die_from_keyword() {
        assert_eq!(Command::from_keyword("DIE"), Some(Command::Die));
    }

    #[test]
    fn command_restart_from_keyword() {
        assert_eq!(Command::from_keyword("RESTART"), Some(Command::Restart));
    }

    #[test]
    fn command_wallops_from_keyword() {
        assert_eq!(Command::from_keyword("WALLOPS"), Some(Command::Wallops));
    }

    #[test]
    fn command_motd_from_keyword() {
        assert_eq!(Command::from_keyword("MOTD"), Some(Command::Motd));
    }

    // ---- Operator commands: round-trip ----

    #[test]
    fn command_oper_round_trip() {
        assert_eq!(Command::from_keyword("OPER"), Some(Command::Oper));
        assert_eq!(Command::Oper.as_str(), "OPER");
        assert_eq!(Command::Oper.to_string(), "OPER");
    }

    #[test]
    fn command_kill_round_trip() {
        assert_eq!(Command::from_keyword("KILL"), Some(Command::Kill));
        assert_eq!(Command::Kill.as_str(), "KILL");
        assert_eq!(Command::Kill.to_string(), "KILL");
    }

    #[test]
    fn command_die_round_trip() {
        assert_eq!(Command::from_keyword("DIE"), Some(Command::Die));
        assert_eq!(Command::Die.as_str(), "DIE");
        assert_eq!(Command::Die.to_string(), "DIE");
    }

    #[test]
    fn command_restart_round_trip() {
        assert_eq!(Command::from_keyword("RESTART"), Some(Command::Restart));
        assert_eq!(Command::Restart.as_str(), "RESTART");
        assert_eq!(Command::Restart.to_string(), "RESTART");
    }

    #[test]
    fn command_wallops_round_trip() {
        assert_eq!(Command::from_keyword("WALLOPS"), Some(Command::Wallops));
        assert_eq!(Command::Wallops.as_str(), "WALLOPS");
        assert_eq!(Command::Wallops.to_string(), "WALLOPS");
    }

    #[test]
    fn command_motd_round_trip() {
        assert_eq!(Command::from_keyword("MOTD"), Some(Command::Motd));
        assert_eq!(Command::Motd.as_str(), "MOTD");
        assert_eq!(Command::Motd.to_string(), "MOTD");
    }

    // ---- PircSubcommand: core not namespaced ----

    #[test]
    fn pirc_core_not_namespaced() {
        assert!(!PircSubcommand::Version.is_namespaced());
        assert!(!PircSubcommand::Cap.is_namespaced());
    }

    // ---- PircSubcommand: equality across categories ----

    #[test]
    fn pirc_subcommand_equality_across_categories() {
        assert_ne!(PircSubcommand::KeyExchange, PircSubcommand::ClusterJoin);
        assert_ne!(PircSubcommand::ClusterJoin, PircSubcommand::P2pOffer);
        assert_ne!(PircSubcommand::Version, PircSubcommand::Encrypted);
    }
}
