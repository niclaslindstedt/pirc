//! Integration tests for pirc-common.
//!
//! Verifies that all public types are accessible from the crate root and that
//! cross-type interactions (types ↔ errors, serde round-trips, Result alias)
//! work correctly.

use pirc_common::{
    ChannelError, ChannelMode, ChannelName, Nickname, PircError, Result, ServerId, UserError,
    UserId, UserMode,
};

// ---- Crate-root re-exports ----

#[test]
fn all_types_importable_from_crate_root() {
    // If this compiles, the re-exports are wired up correctly.
    let _nick = Nickname::new("alice").unwrap();
    let _chan = ChannelName::new("#general").unwrap();
    let _sid = ServerId::new(1);
    let _uid = UserId::new(1);
    let _cm = ChannelMode::InviteOnly;
    let _um = UserMode::Normal;
}

#[test]
fn error_types_importable_from_crate_root() {
    let _ce = ChannelError::NotFound {
        channel: "#test".into(),
    };
    let _ue = UserError::NotOperator;
    let _pe = PircError::ProtocolError {
        message: "test".into(),
    };
}

// ---- Types ↔ errors interplay ----

#[test]
fn invalid_nickname_produces_error() {
    let err = Nickname::new("").unwrap_err();
    assert_eq!(err.to_string(), "nickname must not be empty");
}

#[test]
fn invalid_nickname_starting_with_digit() {
    let err = Nickname::new("123bad").unwrap_err();
    assert!(err.to_string().contains("must start with a letter"));
}

#[test]
fn invalid_channel_name_produces_error() {
    let err = ChannelName::new("nochannel").unwrap_err();
    assert!(err.to_string().contains("must start with '#'"));
}

#[test]
fn invalid_channel_name_empty() {
    let err = ChannelName::new("").unwrap_err();
    assert_eq!(err.to_string(), "channel name must not be empty");
}

// ---- Error conversion chain ----

#[test]
fn channel_error_converts_to_pirc_error() {
    let channel_err = ChannelError::InvalidName {
        name: "bad".into(),
        reason: "missing prefix".into(),
    };
    let pirc_err: PircError = channel_err.into();
    assert!(matches!(pirc_err, PircError::ChannelError(_)));
}

#[test]
fn user_error_converts_to_pirc_error() {
    let user_err = UserError::InvalidNick {
        nick: "123".into(),
        reason: "digit start".into(),
    };
    let pirc_err: PircError = user_err.into();
    assert!(matches!(pirc_err, PircError::UserError(_)));
}

// ---- Result<T> alias with ? operator ----

fn create_nickname(s: &str) -> Result<Nickname> {
    let nick = Nickname::new(s).map_err(|e| UserError::InvalidNick {
        nick: s.into(),
        reason: e.to_string(),
    })?;
    Ok(nick)
}

#[test]
fn result_alias_with_question_mark_ok() {
    let nick = create_nickname("alice").unwrap();
    assert_eq!(nick.as_ref(), "alice");
}

#[test]
fn result_alias_with_question_mark_err() {
    let result = create_nickname("123bad");
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PircError::UserError(UserError::InvalidNick { .. })
    ));
}

fn channel_operation() -> Result<ChannelName> {
    Err(ChannelError::NotFound {
        channel: "#gone".into(),
    })?
}

#[test]
fn result_alias_propagates_channel_error() {
    let result = channel_operation();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PircError::ChannelError(ChannelError::NotFound { .. })
    ));
}

// ---- Serde round-trips ----

#[test]
fn nickname_serde_roundtrip() {
    let nick = Nickname::new("Alice").unwrap();
    let json = serde_json::to_string(&nick).unwrap();
    let deserialized: Nickname = serde_json::from_str(&json).unwrap();
    assert_eq!(nick, deserialized);
}

#[test]
fn channel_name_serde_roundtrip() {
    let chan = ChannelName::new("#general").unwrap();
    let json = serde_json::to_string(&chan).unwrap();
    let deserialized: ChannelName = serde_json::from_str(&json).unwrap();
    assert_eq!(chan, deserialized);
}

#[test]
fn server_id_serde_roundtrip() {
    let id = ServerId::new(42);
    let json = serde_json::to_string(&id).unwrap();
    let deserialized: ServerId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, deserialized);
}

#[test]
fn user_id_serde_roundtrip() {
    let id = UserId::new(99);
    let json = serde_json::to_string(&id).unwrap();
    let deserialized: UserId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, deserialized);
}

#[test]
fn channel_mode_serde_roundtrip() {
    let modes = vec![
        ChannelMode::InviteOnly,
        ChannelMode::Moderated,
        ChannelMode::NoExternalMessages,
        ChannelMode::Secret,
        ChannelMode::TopicProtected,
        ChannelMode::KeyRequired("secret123".to_owned()),
        ChannelMode::UserLimit(50),
    ];
    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: ChannelMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }
}

#[test]
fn user_mode_serde_roundtrip() {
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

// ---- Cross-type interactions ----

#[test]
fn nickname_case_insensitive_equality() {
    let a = Nickname::new("Alice").unwrap();
    let b = Nickname::new("alice").unwrap();
    assert_eq!(a, b);
}

#[test]
fn channel_name_case_insensitive_equality() {
    let a = ChannelName::new("#General").unwrap();
    let b = ChannelName::new("#general").unwrap();
    assert_eq!(a, b);
}

#[test]
fn user_mode_ordering() {
    assert!(UserMode::Normal < UserMode::Voiced);
    assert!(UserMode::Voiced < UserMode::Operator);
    assert!(UserMode::Operator < UserMode::Admin);
}

#[test]
fn server_and_user_id_are_distinct_types() {
    let sid = ServerId::new(1);
    let uid = UserId::new(1);
    // Both display the same number, but are different types.
    assert_eq!(sid.to_string(), "1");
    assert_eq!(uid.to_string(), "1");
    assert_eq!(format!("{sid:?}"), "ServerId(1)");
    assert_eq!(format!("{uid:?}"), "UserId(1)");
}

#[test]
fn channel_mode_display() {
    assert_eq!(ChannelMode::InviteOnly.to_string(), "+i");
    assert_eq!(ChannelMode::KeyRequired("x".into()).to_string(), "+k");
    assert_eq!(ChannelMode::UserLimit(10).to_string(), "+l");
}
