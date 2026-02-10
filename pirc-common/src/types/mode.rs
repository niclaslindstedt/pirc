use std::fmt;

use serde::{Deserialize, Serialize};

/// IRC channel modes that pirc supports.
///
/// Each variant corresponds to a standard IRC channel mode letter.
/// Parameterless modes are simple flags, while `KeyRequired` and `UserLimit`
/// carry associated data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelMode {
    /// Channel requires an invite to join (+i).
    InviteOnly,
    /// Only voiced/operators can speak (+m).
    Moderated,
    /// Only channel members can send messages (+n).
    NoExternalMessages,
    /// Channel is hidden from /list (+s).
    Secret,
    /// Only operators can change the topic (+t).
    TopicProtected,
    /// Channel requires a key (password) to join (+k).
    KeyRequired(String),
    /// Maximum number of users allowed in the channel (+l).
    UserLimit(u32),
}

impl ChannelMode {
    /// Returns the IRC mode character for this mode.
    pub fn mode_char(&self) -> char {
        match self {
            Self::InviteOnly => 'i',
            Self::Moderated => 'm',
            Self::NoExternalMessages => 'n',
            Self::Secret => 's',
            Self::TopicProtected => 't',
            Self::KeyRequired(_) => 'k',
            Self::UserLimit(_) => 'l',
        }
    }
}

impl fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "+{}", self.mode_char())
    }
}

/// User privilege level within an IRC channel.
///
/// Ordering reflects privilege: `Normal < Voiced < Operator < Admin`.
/// The derived `Ord` implementation respects variant declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum UserMode {
    /// Regular user with no special privileges.
    Normal,
    /// Can speak in moderated channels (+v).
    Voiced,
    /// Channel operator with management privileges (+o).
    Operator,
    /// Server administrator.
    Admin,
}

impl UserMode {
    /// Returns the IRC mode character for this user mode, if applicable.
    pub fn mode_char(&self) -> Option<char> {
        match self {
            Self::Voiced => Some('v'),
            Self::Operator => Some('o'),
            Self::Normal | Self::Admin => None,
        }
    }
}

impl fmt::Display for UserMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => f.write_str("Normal"),
            Self::Voiced => f.write_str("+v"),
            Self::Operator => f.write_str("+o"),
            Self::Admin => f.write_str("Admin"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ChannelMode construction ----

    #[test]
    fn channel_mode_invite_only() {
        let mode = ChannelMode::InviteOnly;
        assert_eq!(mode.mode_char(), 'i');
        assert_eq!(mode.to_string(), "+i");
    }

    #[test]
    fn channel_mode_moderated() {
        let mode = ChannelMode::Moderated;
        assert_eq!(mode.mode_char(), 'm');
        assert_eq!(mode.to_string(), "+m");
    }

    #[test]
    fn channel_mode_no_external_messages() {
        let mode = ChannelMode::NoExternalMessages;
        assert_eq!(mode.mode_char(), 'n');
        assert_eq!(mode.to_string(), "+n");
    }

    #[test]
    fn channel_mode_secret() {
        let mode = ChannelMode::Secret;
        assert_eq!(mode.mode_char(), 's');
        assert_eq!(mode.to_string(), "+s");
    }

    #[test]
    fn channel_mode_topic_protected() {
        let mode = ChannelMode::TopicProtected;
        assert_eq!(mode.mode_char(), 't');
        assert_eq!(mode.to_string(), "+t");
    }

    #[test]
    fn channel_mode_key_required() {
        let mode = ChannelMode::KeyRequired("secret".to_owned());
        assert_eq!(mode.mode_char(), 'k');
        assert_eq!(mode.to_string(), "+k");
    }

    #[test]
    fn channel_mode_user_limit() {
        let mode = ChannelMode::UserLimit(50);
        assert_eq!(mode.mode_char(), 'l');
        assert_eq!(mode.to_string(), "+l");
    }

    // ---- ChannelMode equality ----

    #[test]
    fn channel_mode_equality_simple() {
        assert_eq!(ChannelMode::InviteOnly, ChannelMode::InviteOnly);
        assert_ne!(ChannelMode::InviteOnly, ChannelMode::Moderated);
    }

    #[test]
    fn channel_mode_equality_with_data() {
        assert_eq!(
            ChannelMode::KeyRequired("a".to_owned()),
            ChannelMode::KeyRequired("a".to_owned())
        );
        assert_ne!(
            ChannelMode::KeyRequired("a".to_owned()),
            ChannelMode::KeyRequired("b".to_owned())
        );
    }

    #[test]
    fn channel_mode_user_limit_equality() {
        assert_eq!(ChannelMode::UserLimit(10), ChannelMode::UserLimit(10));
        assert_ne!(ChannelMode::UserLimit(10), ChannelMode::UserLimit(20));
    }

    // ---- ChannelMode clone ----

    #[test]
    fn channel_mode_clone() {
        let mode = ChannelMode::KeyRequired("pass".to_owned());
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
    }

    // ---- ChannelMode serde ----

    #[test]
    fn channel_mode_serde_roundtrip_simple() {
        let mode = ChannelMode::InviteOnly;
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: ChannelMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }

    #[test]
    fn channel_mode_serde_roundtrip_key_required() {
        let mode = ChannelMode::KeyRequired("mysecret".to_owned());
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: ChannelMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }

    #[test]
    fn channel_mode_serde_roundtrip_user_limit() {
        let mode = ChannelMode::UserLimit(100);
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: ChannelMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }

    #[test]
    fn channel_mode_serde_all_variants() {
        let modes = vec![
            ChannelMode::InviteOnly,
            ChannelMode::Moderated,
            ChannelMode::NoExternalMessages,
            ChannelMode::Secret,
            ChannelMode::TopicProtected,
            ChannelMode::KeyRequired("key".to_owned()),
            ChannelMode::UserLimit(42),
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: ChannelMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    // ---- UserMode construction ----

    #[test]
    fn user_mode_normal() {
        let mode = UserMode::Normal;
        assert_eq!(mode.mode_char(), None);
        assert_eq!(mode.to_string(), "Normal");
    }

    #[test]
    fn user_mode_voiced() {
        let mode = UserMode::Voiced;
        assert_eq!(mode.mode_char(), Some('v'));
        assert_eq!(mode.to_string(), "+v");
    }

    #[test]
    fn user_mode_operator() {
        let mode = UserMode::Operator;
        assert_eq!(mode.mode_char(), Some('o'));
        assert_eq!(mode.to_string(), "+o");
    }

    #[test]
    fn user_mode_admin() {
        let mode = UserMode::Admin;
        assert_eq!(mode.mode_char(), None);
        assert_eq!(mode.to_string(), "Admin");
    }

    // ---- UserMode ordering ----

    #[test]
    fn user_mode_ordering_normal_less_than_voiced() {
        assert!(UserMode::Normal < UserMode::Voiced);
    }

    #[test]
    fn user_mode_ordering_voiced_less_than_operator() {
        assert!(UserMode::Voiced < UserMode::Operator);
    }

    #[test]
    fn user_mode_ordering_operator_less_than_admin() {
        assert!(UserMode::Operator < UserMode::Admin);
    }

    #[test]
    fn user_mode_ordering_full_chain() {
        assert!(UserMode::Normal < UserMode::Voiced);
        assert!(UserMode::Voiced < UserMode::Operator);
        assert!(UserMode::Operator < UserMode::Admin);
        assert!(UserMode::Normal < UserMode::Admin);
    }

    #[test]
    fn user_mode_ordering_equal() {
        assert!(UserMode::Operator == UserMode::Operator);
        assert!(!(UserMode::Operator < UserMode::Operator));
    }

    // ---- UserMode equality ----

    #[test]
    fn user_mode_equality() {
        assert_eq!(UserMode::Normal, UserMode::Normal);
        assert_ne!(UserMode::Normal, UserMode::Voiced);
    }

    // ---- UserMode copy ----

    #[test]
    fn user_mode_copy() {
        let mode = UserMode::Operator;
        let copied = mode;
        assert_eq!(mode, copied); // mode still usable after copy
    }

    // ---- UserMode hash ----

    #[test]
    fn user_mode_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(UserMode::Normal);
        set.insert(UserMode::Voiced);
        set.insert(UserMode::Normal); // duplicate
        assert_eq!(set.len(), 2);
    }

    // ---- UserMode serde ----

    #[test]
    fn user_mode_serde_roundtrip() {
        let mode = UserMode::Operator;
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: UserMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }

    #[test]
    fn user_mode_serde_all_variants() {
        let modes = [
            UserMode::Normal,
            UserMode::Voiced,
            UserMode::Operator,
            UserMode::Admin,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: UserMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    // ---- UserMode BTreeMap (Ord usage) ----

    #[test]
    fn user_mode_usable_in_btreemap() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(UserMode::Admin, "admin");
        map.insert(UserMode::Normal, "normal");
        map.insert(UserMode::Operator, "op");
        map.insert(UserMode::Voiced, "voiced");

        let keys: Vec<_> = map.keys().collect();
        assert_eq!(
            keys,
            vec![
                &UserMode::Normal,
                &UserMode::Voiced,
                &UserMode::Operator,
                &UserMode::Admin,
            ]
        );
    }
}
