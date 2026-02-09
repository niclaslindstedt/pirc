use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Error type for invalid nickname input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NicknameError {
    #[error("nickname must not be empty")]
    Empty,
    #[error("nickname must not exceed 30 characters (got {0})")]
    TooLong(usize),
    #[error("nickname must start with a letter or one of: _ \\ ` [ ] {{ }} ^ | (got '{0}')")]
    InvalidFirstChar(char),
    #[error("nickname contains invalid character '{0}' at position {1}")]
    InvalidChar(char, usize),
}

/// A validated IRC-style nickname.
///
/// Nicknames are 1–30 characters. The first character must be a letter or one of
/// `_`, `\`, `` ` ``, `[`, `]`, `{`, `}`, `^`, or `|`.
/// Subsequent characters may also include digits and hyphens.
///
/// Comparison and hashing are case-insensitive (IRC convention), but the original
/// casing is preserved for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Nickname(String);

impl Nickname {
    /// Create a new `Nickname` after validating the input.
    ///
    /// # Errors
    ///
    /// Returns `NicknameError` if the input is empty, too long, or contains
    /// characters that are not allowed by the IRC nickname rules.
    pub fn new(s: &str) -> Result<Self, NicknameError> {
        validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

/// Returns `true` if `c` is a valid first character for a nickname.
fn is_valid_first_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '\\' | '`' | '[' | ']' | '{' | '}' | '^' | '|')
}

/// Returns `true` if `c` is a valid subsequent character for a nickname.
fn is_valid_subsequent_char(c: char) -> bool {
    is_valid_first_char(c) || c.is_ascii_digit() || c == '-'
}

fn validate(s: &str) -> Result<(), NicknameError> {
    if s.is_empty() {
        return Err(NicknameError::Empty);
    }
    if s.len() > 30 {
        return Err(NicknameError::TooLong(s.len()));
    }

    let mut chars = s.chars().enumerate();

    // First character
    if let Some((_, first)) = chars.next() {
        if !is_valid_first_char(first) {
            return Err(NicknameError::InvalidFirstChar(first));
        }
    }

    // Subsequent characters
    for (i, c) in chars {
        if !is_valid_subsequent_char(c) {
            return Err(NicknameError::InvalidChar(c, i));
        }
    }

    Ok(())
}

impl fmt::Display for Nickname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Nickname {
    type Err = NicknameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl AsRef<str> for Nickname {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Nickname {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Nickname {}

impl std::hash::Hash for Nickname {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state);
    }
}

impl From<Nickname> for String {
    fn from(nick: Nickname) -> Self {
        nick.0
    }
}

impl TryFrom<String> for Nickname {
    type Error = NicknameError;

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
    fn simple_alpha_nickname() {
        let nick = Nickname::new("alice").unwrap();
        assert_eq!(nick.as_ref(), "alice");
    }

    #[test]
    fn single_letter_nickname() {
        assert!(Nickname::new("A").is_ok());
    }

    #[test]
    fn max_length_nickname() {
        let s = "a".repeat(30);
        assert!(Nickname::new(&s).is_ok());
    }

    #[test]
    fn nickname_with_digits() {
        assert!(Nickname::new("nick123").is_ok());
    }

    #[test]
    fn nickname_with_hyphen() {
        assert!(Nickname::new("nick-name").is_ok());
    }

    #[test]
    fn nickname_with_underscore_start() {
        assert!(Nickname::new("_nick").is_ok());
    }

    #[test]
    fn nickname_starting_with_backslash() {
        assert!(Nickname::new("\\nick").is_ok());
    }

    #[test]
    fn nickname_starting_with_backtick() {
        assert!(Nickname::new("`nick").is_ok());
    }

    #[test]
    fn nickname_starting_with_brackets() {
        assert!(Nickname::new("[nick").is_ok());
        assert!(Nickname::new("]nick").is_ok());
        assert!(Nickname::new("{nick").is_ok());
        assert!(Nickname::new("}nick").is_ok());
    }

    #[test]
    fn nickname_starting_with_caret() {
        assert!(Nickname::new("^nick").is_ok());
    }

    #[test]
    fn nickname_starting_with_pipe() {
        assert!(Nickname::new("|nick").is_ok());
    }

    #[test]
    fn nickname_with_all_valid_special_chars() {
        assert!(Nickname::new("a_\\`[]{}^|-9").is_ok());
    }

    // ---- Construction failure cases ----

    #[test]
    fn empty_nickname_fails() {
        assert_eq!(Nickname::new(""), Err(NicknameError::Empty));
    }

    #[test]
    fn too_long_nickname_fails() {
        let s = "a".repeat(31);
        assert_eq!(Nickname::new(&s), Err(NicknameError::TooLong(31)));
    }

    #[test]
    fn starts_with_digit_fails() {
        assert_eq!(
            Nickname::new("123invalid"),
            Err(NicknameError::InvalidFirstChar('1'))
        );
    }

    #[test]
    fn starts_with_hyphen_fails() {
        assert_eq!(
            Nickname::new("-nick"),
            Err(NicknameError::InvalidFirstChar('-'))
        );
    }

    #[test]
    fn contains_space_fails() {
        assert_eq!(
            Nickname::new("nick name"),
            Err(NicknameError::InvalidChar(' ', 4))
        );
    }

    #[test]
    fn contains_at_sign_fails() {
        assert_eq!(
            Nickname::new("nick@name"),
            Err(NicknameError::InvalidChar('@', 4))
        );
    }

    #[test]
    fn contains_exclamation_fails() {
        assert_eq!(
            Nickname::new("nick!"),
            Err(NicknameError::InvalidChar('!', 4))
        );
    }

    // ---- Case-insensitive equality ----

    #[test]
    fn case_insensitive_equality() {
        let a = Nickname::new("Alice").unwrap();
        let b = Nickname::new("alice").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn case_insensitive_mixed() {
        let a = Nickname::new("NiCkNaMe").unwrap();
        let b = Nickname::new("nickname").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn case_preserving_display() {
        let nick = Nickname::new("Alice").unwrap();
        assert_eq!(nick.to_string(), "Alice");
    }

    // ---- Hash consistency ----

    #[test]
    fn case_insensitive_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let hash_of = |nick: &Nickname| {
            let mut h = DefaultHasher::new();
            nick.hash(&mut h);
            h.finish()
        };

        let a = Nickname::new("Alice").unwrap();
        let b = Nickname::new("alice").unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    // ---- Display ----

    #[test]
    fn display_shows_original_casing() {
        let nick = Nickname::new("MyNick").unwrap();
        assert_eq!(format!("{nick}"), "MyNick");
    }

    // ---- FromStr ----

    #[test]
    fn from_str_valid() {
        let nick: Nickname = "validnick".parse().unwrap();
        assert_eq!(nick.as_ref(), "validnick");
    }

    #[test]
    fn from_str_invalid() {
        let result = "".parse::<Nickname>();
        assert!(result.is_err());
    }

    // ---- AsRef<str> ----

    #[test]
    fn as_ref_str() {
        let nick = Nickname::new("TestNick").unwrap();
        let s: &str = nick.as_ref();
        assert_eq!(s, "TestNick");
    }

    // ---- Serde round-trip ----

    #[test]
    fn serde_roundtrip() {
        let nick = Nickname::new("Alice").unwrap();
        let json = serde_json::to_string(&nick).unwrap();
        assert_eq!(json, "\"Alice\"");
        let deserialized: Nickname = serde_json::from_str(&json).unwrap();
        assert_eq!(nick, deserialized);
    }

    #[test]
    fn serde_invalid_input_fails() {
        let result = serde_json::from_str::<Nickname>("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_invalid_chars_fails() {
        let result = serde_json::from_str::<Nickname>("\"123bad\"");
        assert!(result.is_err());
    }

    // ---- Clone ----

    #[test]
    fn clone_works() {
        let nick = Nickname::new("alice").unwrap();
        let cloned = nick.clone();
        assert_eq!(nick, cloned);
    }

    // ---- Into<String> ----

    #[test]
    fn into_string() {
        let nick = Nickname::new("alice").unwrap();
        let s: String = nick.into();
        assert_eq!(s, "alice");
    }

    // ---- TryFrom<String> ----

    #[test]
    fn try_from_string_valid() {
        let nick = Nickname::try_from("alice".to_owned()).unwrap();
        assert_eq!(nick.as_ref(), "alice");
    }

    #[test]
    fn try_from_string_invalid() {
        assert!(Nickname::try_from(String::new()).is_err());
    }

    // ---- HashMap usage ----

    #[test]
    fn usable_in_hashmap() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let nick = Nickname::new("Alice").unwrap();
        map.insert(nick, 42);

        let lookup = Nickname::new("alice").unwrap();
        assert_eq!(map.get(&lookup), Some(&42));
    }
}
