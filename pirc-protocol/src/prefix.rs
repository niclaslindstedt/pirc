use std::fmt;

use pirc_common::Nickname;

/// The source of an IRC protocol message.
///
/// A prefix identifies who sent a message. It appears at the start of a
/// protocol line, prefixed with `:`.
///
/// Two forms exist:
/// - **Server**: A server name string (e.g., `irc.example.com`)
/// - **User**: A full `nick!user@host` triple
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prefix {
    /// A server-originated message.
    Server(String),
    /// A user-originated message with nick, user, and host components.
    User {
        nick: Nickname,
        user: String,
        host: String,
    },
}

impl Prefix {
    /// Create a user prefix from string components.
    ///
    /// # Panics
    ///
    /// Panics if `nick` is not a valid [`Nickname`].
    pub fn user(nick: &str, user: &str, host: &str) -> Self {
        Self::User {
            nick: Nickname::new(nick).expect("valid nickname"),
            user: user.to_owned(),
            host: host.to_owned(),
        }
    }

    /// Create a server prefix.
    pub fn server(name: &str) -> Self {
        Self::Server(name.to_owned())
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server(name) => f.write_str(name),
            Self::User { nick, user, host } => write!(f, "{}!{user}@{host}", nick.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    #[test]
    fn server_prefix_construction() {
        let prefix = Prefix::Server("irc.example.com".to_owned());
        assert_eq!(prefix, Prefix::Server("irc.example.com".to_owned()));
    }

    #[test]
    fn user_prefix_construction() {
        let prefix = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        if let Prefix::User {
            nick: n,
            user,
            host,
        } = &prefix
        {
            assert_eq!(n.as_ref(), "alice");
            assert_eq!(user, "alice");
            assert_eq!(host, "example.com");
        } else {
            panic!("expected User prefix");
        }
    }

    #[test]
    fn server_prefix_display() {
        let prefix = Prefix::Server("irc.example.com".to_owned());
        assert_eq!(prefix.to_string(), "irc.example.com");
    }

    #[test]
    fn user_prefix_display() {
        let prefix = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        assert_eq!(prefix.to_string(), "alice!alice@example.com");
    }

    #[test]
    fn server_prefix_equality() {
        let a = Prefix::Server("irc.example.com".to_owned());
        let b = Prefix::Server("irc.example.com".to_owned());
        assert_eq!(a, b);
    }

    #[test]
    fn server_prefix_inequality() {
        let a = Prefix::Server("irc.example.com".to_owned());
        let b = Prefix::Server("other.server.com".to_owned());
        assert_ne!(a, b);
    }

    #[test]
    fn user_prefix_equality() {
        let a = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        let b = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn user_prefix_inequality_different_nick() {
        let a = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        let b = Prefix::User {
            nick: nick("bob"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn server_and_user_prefix_inequality() {
        let server = Prefix::Server("alice".to_owned());
        let user = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "alice".to_owned(),
        };
        assert_ne!(server, user);
    }

    #[test]
    fn prefix_clone() {
        let prefix = Prefix::User {
            nick: nick("alice"),
            user: "alice".to_owned(),
            host: "example.com".to_owned(),
        };
        let cloned = prefix.clone();
        assert_eq!(prefix, cloned);
    }

    #[test]
    fn prefix_debug() {
        let prefix = Prefix::Server("irc.test.com".to_owned());
        let debug = format!("{prefix:?}");
        assert!(debug.contains("Server"));
        assert!(debug.contains("irc.test.com"));
    }

    // ---- Convenience constructors ----

    #[test]
    fn prefix_user_constructor() {
        let p = Prefix::user("alice", "alice", "example.com");
        assert_eq!(
            p,
            Prefix::User {
                nick: nick("alice"),
                user: "alice".to_owned(),
                host: "example.com".to_owned(),
            }
        );
    }

    #[test]
    fn prefix_server_constructor() {
        let p = Prefix::server("irc.example.com");
        assert_eq!(p, Prefix::Server("irc.example.com".to_owned()));
    }

    #[test]
    fn prefix_user_display() {
        let p = Prefix::user("bob", "bob", "host.com");
        assert_eq!(p.to_string(), "bob!bob@host.com");
    }

    #[test]
    fn prefix_server_display() {
        let p = Prefix::server("irc.test.com");
        assert_eq!(p.to_string(), "irc.test.com");
    }
}
