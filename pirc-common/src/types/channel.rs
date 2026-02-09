use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Error type for invalid channel name input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelNameError {
    #[error("channel name must not be empty")]
    Empty,
    #[error("channel name must start with '#' (got '{0}')")]
    MissingPrefix(char),
    #[error("channel name must be at least 2 characters (including '#' prefix)")]
    TooShort,
    #[error("channel name must not exceed 50 characters (got {0})")]
    TooLong(usize),
    #[error("channel name contains invalid character '{0}' at position {1}")]
    InvalidChar(char, usize),
}

/// A validated IRC-style channel name.
///
/// Channel names must start with `#`, be 2–50 characters long (including the prefix),
/// and must not contain spaces, control characters, or commas.
///
/// Comparison and hashing are case-insensitive (IRC convention), but the original
/// casing is preserved for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChannelName(String);

impl ChannelName {
    /// Create a new `ChannelName` after validating the input.
    ///
    /// # Errors
    ///
    /// Returns `ChannelNameError` if the input is empty, missing the `#` prefix,
    /// too short, too long, or contains forbidden characters.
    pub fn new(s: &str) -> Result<Self, ChannelNameError> {
        validate(s)?;
        Ok(Self(s.to_owned()))
    }

    /// Returns the channel name without the `#` prefix.
    pub fn name_without_prefix(&self) -> &str {
        &self.0[1..]
    }
}

fn validate(s: &str) -> Result<(), ChannelNameError> {
    if s.is_empty() {
        return Err(ChannelNameError::Empty);
    }

    let first = s.chars().next().expect("non-empty string");
    if first != '#' {
        return Err(ChannelNameError::MissingPrefix(first));
    }

    if s.len() < 2 {
        return Err(ChannelNameError::TooShort);
    }

    if s.len() > 50 {
        return Err(ChannelNameError::TooLong(s.len()));
    }

    for (i, c) in s.chars().enumerate().skip(1) {
        if c == ' ' || c == ',' || c.is_control() {
            return Err(ChannelNameError::InvalidChar(c, i));
        }
    }

    Ok(())
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ChannelName {
    type Err = ChannelNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl AsRef<str> for ChannelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq for ChannelName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for ChannelName {}

impl std::hash::Hash for ChannelName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state);
    }
}

impl From<ChannelName> for String {
    fn from(channel: ChannelName) -> Self {
        channel.0
    }
}

impl TryFrom<String> for ChannelName {
    type Error = ChannelNameError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        validate(&s)?;
        Ok(Self(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Construction success cases ----

    #[test]
    fn simple_channel_name() {
        let ch = ChannelName::new("#general").unwrap();
        assert_eq!(ch.as_ref(), "#general");
    }

    #[test]
    fn minimal_channel_name() {
        let ch = ChannelName::new("#a").unwrap();
        assert_eq!(ch.as_ref(), "#a");
    }

    #[test]
    fn max_length_channel_name() {
        let s = format!("#{}", "a".repeat(49));
        assert_eq!(s.len(), 50);
        assert!(ChannelName::new(&s).is_ok());
    }

    #[test]
    fn channel_with_digits() {
        assert!(ChannelName::new("#channel123").is_ok());
    }

    #[test]
    fn channel_with_hyphens_and_underscores() {
        assert!(ChannelName::new("#my-channel_name").is_ok());
    }

    #[test]
    fn channel_with_dots() {
        assert!(ChannelName::new("#irc.channel").is_ok());
    }

    #[test]
    fn channel_with_special_chars() {
        assert!(ChannelName::new("#chan[test]").is_ok());
    }

    // ---- Construction failure cases ----

    #[test]
    fn empty_channel_name_fails() {
        assert_eq!(ChannelName::new(""), Err(ChannelNameError::Empty));
    }

    #[test]
    fn missing_prefix_fails() {
        assert_eq!(
            ChannelName::new("general"),
            Err(ChannelNameError::MissingPrefix('g'))
        );
    }

    #[test]
    fn just_prefix_fails() {
        assert_eq!(ChannelName::new("#"), Err(ChannelNameError::TooShort));
    }

    #[test]
    fn too_long_channel_name_fails() {
        let s = format!("#{}", "a".repeat(50));
        assert_eq!(s.len(), 51);
        assert_eq!(ChannelName::new(&s), Err(ChannelNameError::TooLong(51)));
    }

    #[test]
    fn contains_space_fails() {
        assert_eq!(
            ChannelName::new("#has space"),
            Err(ChannelNameError::InvalidChar(' ', 4))
        );
    }

    #[test]
    fn contains_comma_fails() {
        assert_eq!(
            ChannelName::new("#has,comma"),
            Err(ChannelNameError::InvalidChar(',', 4))
        );
    }

    #[test]
    fn contains_control_char_fails() {
        assert_eq!(
            ChannelName::new("#has\x07bell"),
            Err(ChannelNameError::InvalidChar('\x07', 4))
        );
    }

    #[test]
    fn contains_null_byte_fails() {
        assert_eq!(
            ChannelName::new("#has\0null"),
            Err(ChannelNameError::InvalidChar('\0', 4))
        );
    }

    #[test]
    fn wrong_prefix_fails() {
        assert_eq!(
            ChannelName::new("&general"),
            Err(ChannelNameError::MissingPrefix('&'))
        );
    }

    // ---- Case-insensitive equality ----

    #[test]
    fn case_insensitive_equality() {
        let a = ChannelName::new("#General").unwrap();
        let b = ChannelName::new("#general").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn case_insensitive_mixed() {
        let a = ChannelName::new("#ChAnNeL").unwrap();
        let b = ChannelName::new("#channel").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn case_preserving_display() {
        let ch = ChannelName::new("#General").unwrap();
        assert_eq!(ch.to_string(), "#General");
    }

    // ---- Hash consistency ----

    #[test]
    fn case_insensitive_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let hash_of = |ch: &ChannelName| {
            let mut h = DefaultHasher::new();
            ch.hash(&mut h);
            h.finish()
        };

        let a = ChannelName::new("#General").unwrap();
        let b = ChannelName::new("#general").unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    // ---- Display ----

    #[test]
    fn display_shows_full_name_with_prefix() {
        let ch = ChannelName::new("#mychannel").unwrap();
        assert_eq!(format!("{ch}"), "#mychannel");
    }

    // ---- FromStr ----

    #[test]
    fn from_str_valid() {
        let ch: ChannelName = "#valid".parse().unwrap();
        assert_eq!(ch.as_ref(), "#valid");
    }

    #[test]
    fn from_str_invalid() {
        let result = "nochannel".parse::<ChannelName>();
        assert!(result.is_err());
    }

    // ---- Display / FromStr round-trip ----

    #[test]
    fn display_fromstr_roundtrip() {
        let original = ChannelName::new("#RoundTrip").unwrap();
        let displayed = original.to_string();
        let parsed: ChannelName = displayed.parse().unwrap();
        assert_eq!(original, parsed);
        assert_eq!(parsed.as_ref(), "#RoundTrip");
    }

    // ---- AsRef<str> ----

    #[test]
    fn as_ref_str() {
        let ch = ChannelName::new("#test").unwrap();
        let s: &str = ch.as_ref();
        assert_eq!(s, "#test");
    }

    // ---- name_without_prefix ----

    #[test]
    fn name_without_prefix_returns_name() {
        let ch = ChannelName::new("#general").unwrap();
        assert_eq!(ch.name_without_prefix(), "general");
    }

    #[test]
    fn name_without_prefix_preserves_case() {
        let ch = ChannelName::new("#General").unwrap();
        assert_eq!(ch.name_without_prefix(), "General");
    }

    // ---- Serde round-trip ----

    #[test]
    fn serde_roundtrip() {
        let ch = ChannelName::new("#general").unwrap();
        let json = serde_json::to_string(&ch).unwrap();
        assert_eq!(json, "\"#general\"");
        let deserialized: ChannelName = serde_json::from_str(&json).unwrap();
        assert_eq!(ch, deserialized);
    }

    #[test]
    fn serde_invalid_input_fails() {
        let result = serde_json::from_str::<ChannelName>("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_missing_prefix_fails() {
        let result = serde_json::from_str::<ChannelName>("\"general\"");
        assert!(result.is_err());
    }

    // ---- Clone ----

    #[test]
    fn clone_works() {
        let ch = ChannelName::new("#general").unwrap();
        let cloned = ch.clone();
        assert_eq!(ch, cloned);
    }

    // ---- Into<String> ----

    #[test]
    fn into_string() {
        let ch = ChannelName::new("#general").unwrap();
        let s: String = ch.into();
        assert_eq!(s, "#general");
    }

    // ---- TryFrom<String> ----

    #[test]
    fn try_from_string_valid() {
        let ch = ChannelName::try_from("#general".to_owned()).unwrap();
        assert_eq!(ch.as_ref(), "#general");
    }

    #[test]
    fn try_from_string_invalid() {
        assert!(ChannelName::try_from(String::new()).is_err());
    }

    // ---- HashMap usage ----

    #[test]
    fn usable_in_hashmap() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let ch = ChannelName::new("#General").unwrap();
        map.insert(ch, 42);

        let lookup = ChannelName::new("#general").unwrap();
        assert_eq!(map.get(&lookup), Some(&42));
    }
}
