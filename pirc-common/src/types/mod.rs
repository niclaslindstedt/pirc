//! Validated IRC types: nicknames, channel names, identifiers, modes, and groups.

pub mod channel;
pub mod group;
pub mod identifiers;
pub mod mode;
pub mod nickname;

pub use channel::ChannelName;
pub use channel::ChannelNameError;
pub use group::{GroupInfo, GroupMember, GroupMembership};
pub use identifiers::GroupId;
pub use identifiers::ServerId;
pub use identifiers::UserId;
pub use mode::ChannelMode;
pub use mode::GroupMemberRole;
pub use mode::UserMode;
pub use nickname::Nickname;
pub use nickname::NicknameError;
