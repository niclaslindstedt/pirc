use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use pirc_common::{ChannelMode, ChannelName, Nickname};

/// Per-channel privilege level for a member.
///
/// Represents the status a user holds within a specific channel.
/// Ordering reflects privilege: `Normal < Voiced < Operator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberStatus {
    /// Regular channel member with no special privileges.
    Normal,
    /// Can speak in moderated channels (+v).
    Voiced,
    /// Channel operator with management privileges (+o).
    Operator,
}

impl MemberStatus {
    /// Returns the IRC prefix character for this status, if any.
    pub fn prefix_char(&self) -> Option<char> {
        match self {
            Self::Normal => None,
            Self::Voiced => Some('+'),
            Self::Operator => Some('@'),
        }
    }

    /// Returns the IRC mode character for this status, if any.
    pub fn mode_char(&self) -> Option<char> {
        match self {
            Self::Normal => None,
            Self::Voiced => Some('v'),
            Self::Operator => Some('o'),
        }
    }
}

/// An entry in a channel's ban list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanEntry {
    /// The ban mask (e.g. `*!*@host`).
    pub mask: String,
    /// Who set the ban (nick or server name).
    pub who_set: String,
    /// Unix timestamp when the ban was set.
    pub timestamp: u64,
}

/// A single IRC channel's state.
///
/// Holds all information the server tracks for a channel: name, topic,
/// modes, member list, bans, invites, and channel metadata.
pub struct Channel {
    /// Validated channel name (e.g. `#general`).
    pub name: ChannelName,
    /// Channel topic: (text, who_set, timestamp). `None` if no topic is set.
    pub topic: Option<(String, String, u64)>,
    /// Active channel modes (flag-type modes only; key and limit are separate fields).
    pub modes: HashSet<ChannelMode>,
    /// Members currently in the channel, mapped to their per-channel status.
    pub members: HashMap<Nickname, MemberStatus>,
    /// Ban list entries (+b).
    pub ban_list: Vec<BanEntry>,
    /// Invited nicknames (for +i channels).
    pub invite_list: HashSet<Nickname>,
    /// Channel key for +k mode. `None` if no key is set.
    pub key: Option<String>,
    /// Maximum user count for +l mode. `None` if no limit is set.
    pub user_limit: Option<u32>,
    /// Unix timestamp when the channel was created.
    pub created_at: u64,
}

impl Channel {
    /// Create a new empty channel with the given name.
    ///
    /// The channel starts with no topic, no modes, no members, and the
    /// creation timestamp set to the current time.
    pub fn new(name: ChannelName) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            name,
            topic: None,
            modes: HashSet::new(),
            members: HashMap::new(),
            ban_list: Vec::new(),
            invite_list: HashSet::new(),
            key: None,
            user_limit: None,
            created_at,
        }
    }

    /// Returns the number of members in the channel.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Returns `true` if the channel has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_name(s: &str) -> ChannelName {
        ChannelName::new(s).unwrap()
    }

    fn nick(s: &str) -> Nickname {
        Nickname::new(s).unwrap()
    }

    // ---- MemberStatus ----

    #[test]
    fn member_status_ordering() {
        assert!(MemberStatus::Normal < MemberStatus::Voiced);
        assert!(MemberStatus::Voiced < MemberStatus::Operator);
        assert!(MemberStatus::Normal < MemberStatus::Operator);
    }

    #[test]
    fn member_status_prefix_char() {
        assert_eq!(MemberStatus::Normal.prefix_char(), None);
        assert_eq!(MemberStatus::Voiced.prefix_char(), Some('+'));
        assert_eq!(MemberStatus::Operator.prefix_char(), Some('@'));
    }

    #[test]
    fn member_status_mode_char() {
        assert_eq!(MemberStatus::Normal.mode_char(), None);
        assert_eq!(MemberStatus::Voiced.mode_char(), Some('v'));
        assert_eq!(MemberStatus::Operator.mode_char(), Some('o'));
    }

    #[test]
    fn member_status_equality() {
        assert_eq!(MemberStatus::Normal, MemberStatus::Normal);
        assert_ne!(MemberStatus::Normal, MemberStatus::Voiced);
    }

    #[test]
    fn member_status_copy() {
        let status = MemberStatus::Operator;
        let copied = status;
        assert_eq!(status, copied);
    }

    #[test]
    fn member_status_hash() {
        let mut set = HashSet::new();
        set.insert(MemberStatus::Normal);
        set.insert(MemberStatus::Voiced);
        set.insert(MemberStatus::Normal); // duplicate
        assert_eq!(set.len(), 2);
    }

    // ---- BanEntry ----

    #[test]
    fn ban_entry_creation() {
        let entry = BanEntry {
            mask: "*!*@evil.host".to_owned(),
            who_set: "ChanOp".to_owned(),
            timestamp: 1700000000,
        };
        assert_eq!(entry.mask, "*!*@evil.host");
        assert_eq!(entry.who_set, "ChanOp");
        assert_eq!(entry.timestamp, 1700000000);
    }

    #[test]
    fn ban_entry_equality() {
        let a = BanEntry {
            mask: "*!*@host".to_owned(),
            who_set: "op".to_owned(),
            timestamp: 100,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // ---- Channel ----

    #[test]
    fn new_channel_defaults() {
        let ch = Channel::new(channel_name("#test"));
        assert_eq!(ch.name, channel_name("#test"));
        assert!(ch.topic.is_none());
        assert!(ch.modes.is_empty());
        assert!(ch.members.is_empty());
        assert!(ch.ban_list.is_empty());
        assert!(ch.invite_list.is_empty());
        assert!(ch.key.is_none());
        assert!(ch.user_limit.is_none());
        assert!(ch.created_at > 0);
    }

    #[test]
    fn channel_member_count() {
        let mut ch = Channel::new(channel_name("#test"));
        assert_eq!(ch.member_count(), 0);
        assert!(ch.is_empty());

        ch.members.insert(nick("Alice"), MemberStatus::Operator);
        assert_eq!(ch.member_count(), 1);
        assert!(!ch.is_empty());

        ch.members.insert(nick("Bob"), MemberStatus::Normal);
        assert_eq!(ch.member_count(), 2);
    }

    #[test]
    fn channel_topic() {
        let mut ch = Channel::new(channel_name("#test"));
        ch.topic = Some(("Welcome!".to_owned(), "Alice".to_owned(), 1700000000));

        let (text, who, ts) = ch.topic.as_ref().unwrap();
        assert_eq!(text, "Welcome!");
        assert_eq!(who, "Alice");
        assert_eq!(*ts, 1700000000);
    }

    #[test]
    fn channel_modes() {
        let mut ch = Channel::new(channel_name("#test"));
        ch.modes.insert(ChannelMode::InviteOnly);
        ch.modes.insert(ChannelMode::Moderated);
        assert!(ch.modes.contains(&ChannelMode::InviteOnly));
        assert!(ch.modes.contains(&ChannelMode::Moderated));
        assert!(!ch.modes.contains(&ChannelMode::Secret));
    }

    #[test]
    fn channel_key_and_limit() {
        let mut ch = Channel::new(channel_name("#test"));
        ch.key = Some("secret123".to_owned());
        ch.user_limit = Some(50);
        assert_eq!(ch.key.as_deref(), Some("secret123"));
        assert_eq!(ch.user_limit, Some(50));
    }

    #[test]
    fn channel_ban_list() {
        let mut ch = Channel::new(channel_name("#test"));
        ch.ban_list.push(BanEntry {
            mask: "*!*@bad.host".to_owned(),
            who_set: "Op".to_owned(),
            timestamp: 100,
        });
        assert_eq!(ch.ban_list.len(), 1);
        assert_eq!(ch.ban_list[0].mask, "*!*@bad.host");
    }

    #[test]
    fn channel_invite_list() {
        let mut ch = Channel::new(channel_name("#test"));
        ch.invite_list.insert(nick("Alice"));
        ch.invite_list.insert(nick("Bob"));
        assert!(ch.invite_list.contains(&nick("Alice")));
        assert!(ch.invite_list.contains(&nick("alice"))); // case-insensitive
        assert_eq!(ch.invite_list.len(), 2);
    }
}
