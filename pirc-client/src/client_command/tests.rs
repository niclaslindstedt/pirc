use super::*;

// ── Helper ─────────────────────────────────────────────────

fn args(strs: &[&str]) -> Vec<String> {
    strs.iter().map(|s| (*s).to_owned()).collect()
}

// ── Channel commands ───────────────────────────────────────

#[test]
fn join_valid() {
    assert_eq!(
        ClientCommand::from_parsed("join", &args(&["#test"])),
        Ok(ClientCommand::Join("#test".into()))
    );
}

#[test]
fn join_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("join", &[]),
        Err(CommandError::MissingArgument {
            command: "join".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn join_no_hash_prefix() {
    assert_eq!(
        ClientCommand::from_parsed("join", &args(&["test"])),
        Err(CommandError::InvalidArgument {
            command: "join".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn part_with_reason() {
    assert_eq!(
        ClientCommand::from_parsed("part", &args(&["#chan", "Goodbye!"])),
        Ok(ClientCommand::Part("#chan".into(), Some("Goodbye!".into())))
    );
}

#[test]
fn part_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("part", &args(&["#chan"])),
        Ok(ClientCommand::Part("#chan".into(), None))
    );
}

#[test]
fn part_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("part", &[]),
        Err(CommandError::MissingArgument {
            command: "part".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn part_invalid_channel() {
    assert_eq!(
        ClientCommand::from_parsed("part", &args(&["nochannel"])),
        Err(CommandError::InvalidArgument {
            command: "part".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn topic_query() {
    assert_eq!(
        ClientCommand::from_parsed("topic", &args(&["#chan"])),
        Ok(ClientCommand::Topic("#chan".into(), None))
    );
}

#[test]
fn topic_set() {
    assert_eq!(
        ClientCommand::from_parsed("topic", &args(&["#chan", "New topic here"])),
        Ok(ClientCommand::Topic(
            "#chan".into(),
            Some("New topic here".into())
        ))
    );
}

#[test]
fn topic_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("topic", &[]),
        Err(CommandError::MissingArgument {
            command: "topic".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn topic_invalid_channel() {
    assert_eq!(
        ClientCommand::from_parsed("topic", &args(&["nochan"])),
        Err(CommandError::InvalidArgument {
            command: "topic".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn list_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("list", &[]),
        Ok(ClientCommand::List(None))
    );
}

#[test]
fn list_with_pattern() {
    assert_eq!(
        ClientCommand::from_parsed("list", &args(&["#rust*"])),
        Ok(ClientCommand::List(Some("#rust*".into())))
    );
}

#[test]
fn invite_valid() {
    assert_eq!(
        ClientCommand::from_parsed("invite", &args(&["nick", "#chan"])),
        Ok(ClientCommand::Invite("nick".into(), "#chan".into()))
    );
}

#[test]
fn invite_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("invite", &[]),
        Err(CommandError::MissingArgument {
            command: "invite".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn invite_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("invite", &args(&["nick"])),
        Err(CommandError::MissingArgument {
            command: "invite".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn invite_invalid_channel() {
    assert_eq!(
        ClientCommand::from_parsed("invite", &args(&["nick", "badchan"])),
        Err(CommandError::InvalidArgument {
            command: "invite".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn names_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("names", &[]),
        Ok(ClientCommand::Names(None))
    );
}

#[test]
fn names_with_channel() {
    assert_eq!(
        ClientCommand::from_parsed("names", &args(&["#test"])),
        Ok(ClientCommand::Names(Some("#test".into())))
    );
}

// ── Messaging commands ─────────────────────────────────────

#[test]
fn msg_valid() {
    assert_eq!(
        ClientCommand::from_parsed("msg", &args(&["nick", "hello world"])),
        Ok(ClientCommand::Msg("nick".into(), "hello world".into()))
    );
}

#[test]
fn privmsg_alias() {
    assert_eq!(
        ClientCommand::from_parsed("privmsg", &args(&["nick", "hi"])),
        Ok(ClientCommand::Msg("nick".into(), "hi".into()))
    );
}

#[test]
fn msg_missing_target() {
    assert_eq!(
        ClientCommand::from_parsed("msg", &[]),
        Err(CommandError::MissingArgument {
            command: "msg".into(),
            argument: "target".into(),
        })
    );
}

#[test]
fn msg_missing_message() {
    assert_eq!(
        ClientCommand::from_parsed("msg", &args(&["nick"])),
        Err(CommandError::MissingArgument {
            command: "msg".into(),
            argument: "message".into(),
        })
    );
}

#[test]
fn query_with_message() {
    assert_eq!(
        ClientCommand::from_parsed("query", &args(&["nick", "hey there"])),
        Ok(ClientCommand::Query(
            "nick".into(),
            Some("hey there".into())
        ))
    );
}

#[test]
fn query_without_message() {
    assert_eq!(
        ClientCommand::from_parsed("query", &args(&["nick"])),
        Ok(ClientCommand::Query("nick".into(), None))
    );
}

#[test]
fn query_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("query", &[]),
        Err(CommandError::MissingArgument {
            command: "query".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn me_action() {
    // Parser produces: ["waves", "at everyone"]
    assert_eq!(
        ClientCommand::from_parsed("me", &args(&["waves", "at everyone"])),
        Ok(ClientCommand::Me("waves at everyone".into()))
    );
}

#[test]
fn me_single_word() {
    assert_eq!(
        ClientCommand::from_parsed("me", &args(&["waves"])),
        Ok(ClientCommand::Me("waves".into()))
    );
}

#[test]
fn me_missing_text() {
    assert_eq!(
        ClientCommand::from_parsed("me", &[]),
        Err(CommandError::MissingArgument {
            command: "me".into(),
            argument: "action text".into(),
        })
    );
}

#[test]
fn notice_valid() {
    assert_eq!(
        ClientCommand::from_parsed("notice", &args(&["user", "This is a notice"])),
        Ok(ClientCommand::Notice(
            "user".into(),
            "This is a notice".into()
        ))
    );
}

#[test]
fn notice_missing_target() {
    assert_eq!(
        ClientCommand::from_parsed("notice", &[]),
        Err(CommandError::MissingArgument {
            command: "notice".into(),
            argument: "target".into(),
        })
    );
}

#[test]
fn notice_missing_message() {
    assert_eq!(
        ClientCommand::from_parsed("notice", &args(&["user"])),
        Err(CommandError::MissingArgument {
            command: "notice".into(),
            argument: "message".into(),
        })
    );
}

#[test]
fn ctcp_with_args() {
    // Parser: ["nick", "VERSION extra"]
    assert_eq!(
        ClientCommand::from_parsed("ctcp", &args(&["nick", "version extra"])),
        Ok(ClientCommand::Ctcp(
            "nick".into(),
            "VERSION".into(),
            Some("extra".into())
        ))
    );
}

#[test]
fn ctcp_without_args() {
    assert_eq!(
        ClientCommand::from_parsed("ctcp", &args(&["nick", "ping"])),
        Ok(ClientCommand::Ctcp("nick".into(), "PING".into(), None))
    );
}

#[test]
fn ctcp_missing_target() {
    assert_eq!(
        ClientCommand::from_parsed("ctcp", &[]),
        Err(CommandError::MissingArgument {
            command: "ctcp".into(),
            argument: "target".into(),
        })
    );
}

#[test]
fn ctcp_missing_command() {
    assert_eq!(
        ClientCommand::from_parsed("ctcp", &args(&["nick"])),
        Err(CommandError::MissingArgument {
            command: "ctcp".into(),
            argument: "command".into(),
        })
    );
}

// ── User commands ──────────────────────────────────────────

#[test]
fn nick_valid() {
    assert_eq!(
        ClientCommand::from_parsed("nick", &args(&["newnick"])),
        Ok(ClientCommand::Nick("newnick".into()))
    );
}

#[test]
fn nick_missing() {
    assert_eq!(
        ClientCommand::from_parsed("nick", &[]),
        Err(CommandError::MissingArgument {
            command: "nick".into(),
            argument: "nickname".into(),
        })
    );
}

#[test]
fn whois_valid() {
    assert_eq!(
        ClientCommand::from_parsed("whois", &args(&["someuser"])),
        Ok(ClientCommand::Whois("someuser".into()))
    );
}

#[test]
fn whois_missing() {
    assert_eq!(
        ClientCommand::from_parsed("whois", &[]),
        Err(CommandError::MissingArgument {
            command: "whois".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn away_with_reason() {
    assert_eq!(
        ClientCommand::from_parsed("away", &args(&["Gone", "for lunch"])),
        Ok(ClientCommand::Away(Some("Gone for lunch".into())))
    );
}

#[test]
fn away_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("away", &[]),
        Ok(ClientCommand::Away(None))
    );
}

#[test]
fn quit_with_reason() {
    assert_eq!(
        ClientCommand::from_parsed("quit", &args(&["Goodbye", "cruel world"])),
        Ok(ClientCommand::Quit(Some("Goodbye cruel world".into())))
    );
}

#[test]
fn quit_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("quit", &[]),
        Ok(ClientCommand::Quit(None))
    );
}

// ── Moderation commands ────────────────────────────────────

#[test]
fn kick_with_reason() {
    // Parser: ["#chan", "baduser You have been kicked"]
    assert_eq!(
        ClientCommand::from_parsed("kick", &args(&["#chan", "baduser You have been kicked"])),
        Ok(ClientCommand::Kick(
            "#chan".into(),
            "baduser".into(),
            Some("You have been kicked".into())
        ))
    );
}

#[test]
fn kick_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("kick", &args(&["#chan", "baduser"])),
        Ok(ClientCommand::Kick("#chan".into(), "baduser".into(), None))
    );
}

#[test]
fn kick_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("kick", &[]),
        Err(CommandError::MissingArgument {
            command: "kick".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn kick_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("kick", &args(&["#chan"])),
        Err(CommandError::MissingArgument {
            command: "kick".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn kick_invalid_channel() {
    assert_eq!(
        ClientCommand::from_parsed("kick", &args(&["badchan", "nick"])),
        Err(CommandError::InvalidArgument {
            command: "kick".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn ban_valid() {
    assert_eq!(
        ClientCommand::from_parsed("ban", &args(&["#chan", "*!*@bad.host"])),
        Ok(ClientCommand::Ban("#chan".into(), "*!*@bad.host".into()))
    );
}

#[test]
fn ban_missing_channel() {
    assert_eq!(
        ClientCommand::from_parsed("ban", &[]),
        Err(CommandError::MissingArgument {
            command: "ban".into(),
            argument: "channel".into(),
        })
    );
}

#[test]
fn ban_missing_mask() {
    assert_eq!(
        ClientCommand::from_parsed("ban", &args(&["#chan"])),
        Err(CommandError::MissingArgument {
            command: "ban".into(),
            argument: "mask".into(),
        })
    );
}

#[test]
fn ban_invalid_channel() {
    assert_eq!(
        ClientCommand::from_parsed("ban", &args(&["nochan", "mask"])),
        Err(CommandError::InvalidArgument {
            command: "ban".into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        })
    );
}

#[test]
fn mode_with_modestring() {
    assert_eq!(
        ClientCommand::from_parsed("mode", &args(&["#chan", "+o user"])),
        Ok(ClientCommand::Mode("#chan".into(), Some("+o user".into())))
    );
}

#[test]
fn mode_query() {
    assert_eq!(
        ClientCommand::from_parsed("mode", &args(&["#chan"])),
        Ok(ClientCommand::Mode("#chan".into(), None))
    );
}

#[test]
fn mode_missing_target() {
    assert_eq!(
        ClientCommand::from_parsed("mode", &[]),
        Err(CommandError::MissingArgument {
            command: "mode".into(),
            argument: "target".into(),
        })
    );
}

// ── Operator commands ──────────────────────────────────────

#[test]
fn oper_valid() {
    assert_eq!(
        ClientCommand::from_parsed("oper", &args(&["admin", "secret"])),
        Ok(ClientCommand::Oper("admin".into(), "secret".into()))
    );
}

#[test]
fn oper_missing_name() {
    assert_eq!(
        ClientCommand::from_parsed("oper", &[]),
        Err(CommandError::MissingArgument {
            command: "oper".into(),
            argument: "name".into(),
        })
    );
}

#[test]
fn oper_missing_password() {
    assert_eq!(
        ClientCommand::from_parsed("oper", &args(&["admin"])),
        Err(CommandError::MissingArgument {
            command: "oper".into(),
            argument: "password".into(),
        })
    );
}

#[test]
fn kill_valid() {
    assert_eq!(
        ClientCommand::from_parsed("kill", &args(&["baduser", "Spamming"])),
        Ok(ClientCommand::Kill("baduser".into(), "Spamming".into()))
    );
}

#[test]
fn kill_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("kill", &[]),
        Err(CommandError::MissingArgument {
            command: "kill".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn kill_missing_reason() {
    assert_eq!(
        ClientCommand::from_parsed("kill", &args(&["baduser"])),
        Err(CommandError::MissingArgument {
            command: "kill".into(),
            argument: "reason".into(),
        })
    );
}

#[test]
fn die_with_reason() {
    assert_eq!(
        ClientCommand::from_parsed("die", &args(&["shutting down"])),
        Ok(ClientCommand::Die(Some("shutting down".into())))
    );
}

#[test]
fn die_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("die", &[]),
        Ok(ClientCommand::Die(None))
    );
}

#[test]
fn restart_with_reason() {
    assert_eq!(
        ClientCommand::from_parsed("restart", &args(&["updating"])),
        Ok(ClientCommand::Restart(Some("updating".into())))
    );
}

#[test]
fn restart_without_reason() {
    assert_eq!(
        ClientCommand::from_parsed("restart", &[]),
        Ok(ClientCommand::Restart(None))
    );
}

// ── pirc-specific commands ─────────────────────────────────

#[test]
fn cluster_valid() {
    assert_eq!(
        ClientCommand::from_parsed("cluster", &args(&["status"])),
        Ok(ClientCommand::Cluster("status".into(), vec![]))
    );
}

#[test]
fn cluster_with_extra_args() {
    assert_eq!(
        ClientCommand::from_parsed("cluster", &args(&["add", "node1.example.com"])),
        Ok(ClientCommand::Cluster(
            "add".into(),
            vec!["node1.example.com".into()]
        ))
    );
}

#[test]
fn cluster_missing_subcommand() {
    assert_eq!(
        ClientCommand::from_parsed("cluster", &[]),
        Err(CommandError::MissingArgument {
            command: "cluster".into(),
            argument: "subcommand".into(),
        })
    );
}

#[test]
fn invite_key_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("invite-key", &[]),
        Ok(ClientCommand::InviteKey(vec![]))
    );
}

#[test]
fn invite_key_with_args() {
    assert_eq!(
        ClientCommand::from_parsed("invite-key", &args(&["generate"])),
        Ok(ClientCommand::InviteKey(vec!["generate".into()]))
    );
}

#[test]
fn network_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("network", &[]),
        Ok(ClientCommand::Network(vec![]))
    );
}

#[test]
fn network_with_args() {
    assert_eq!(
        ClientCommand::from_parsed("network", &args(&["status", "details here"])),
        Ok(ClientCommand::Network(vec![
            "status".into(),
            "details here".into()
        ]))
    );
}

// ── Meta commands ──────────────────────────────────────────

#[test]
fn help_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("help", &[]),
        Ok(ClientCommand::Help(None))
    );
}

#[test]
fn help_with_topic() {
    assert_eq!(
        ClientCommand::from_parsed("help", &args(&["join"])),
        Ok(ClientCommand::Help(Some("join".into())))
    );
}

// ── Unknown commands ───────────────────────────────────────

#[test]
fn unknown_command() {
    assert_eq!(
        ClientCommand::from_parsed("foobar", &args(&["arg1", "arg2"])),
        Ok(ClientCommand::Unknown(
            "foobar".into(),
            vec!["arg1".into(), "arg2".into()]
        ))
    );
}

#[test]
fn unknown_command_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("xyz", &[]),
        Ok(ClientCommand::Unknown("xyz".into(), vec![]))
    );
}

// ── Case insensitivity ─────────────────────────────────────
// The parser already lowercases command names, but we test that
// from_parsed works correctly with lowercase input.

#[test]
fn join_lowercase_name() {
    assert_eq!(
        ClientCommand::from_parsed("join", &args(&["#test"])),
        Ok(ClientCommand::Join("#test".into()))
    );
}

// ── Error display ──────────────────────────────────────────

#[test]
fn missing_argument_display() {
    let err = CommandError::MissingArgument {
        command: "join".into(),
        argument: "channel".into(),
    };
    assert_eq!(err.to_string(), "join: missing required argument: channel");
}

#[test]
fn invalid_argument_display() {
    let err = CommandError::InvalidArgument {
        command: "join".into(),
        argument: "channel".into(),
        reason: "channel name must start with #".into(),
    };
    assert_eq!(
        err.to_string(),
        "join: invalid channel: channel name must start with #"
    );
}

// ── Edge cases ─────────────────────────────────────────────

#[test]
fn msg_to_channel() {
    assert_eq!(
        ClientCommand::from_parsed("msg", &args(&["#channel", "hello everyone"])),
        Ok(ClientCommand::Msg(
            "#channel".into(),
            "hello everyone".into()
        ))
    );
}

#[test]
fn ctcp_command_uppercased() {
    assert_eq!(
        ClientCommand::from_parsed("ctcp", &args(&["nick", "version"])),
        Ok(ClientCommand::Ctcp("nick".into(), "VERSION".into(), None))
    );
}

#[test]
fn mode_on_user() {
    assert_eq!(
        ClientCommand::from_parsed("mode", &args(&["mynick", "+i"])),
        Ok(ClientCommand::Mode("mynick".into(), Some("+i".into())))
    );
}

#[test]
fn empty_command_name_is_unknown() {
    assert_eq!(
        ClientCommand::from_parsed("", &[]),
        Ok(ClientCommand::Unknown(String::new(), vec![]))
    );
}

#[test]
fn quit_single_word_reason() {
    assert_eq!(
        ClientCommand::from_parsed("quit", &args(&["Bye"])),
        Ok(ClientCommand::Quit(Some("Bye".into())))
    );
}

#[test]
fn away_single_word() {
    assert_eq!(
        ClientCommand::from_parsed("away", &args(&["brb"])),
        Ok(ClientCommand::Away(Some("brb".into())))
    );
}

#[test]
fn kick_extracts_nick_from_trailing() {
    // Simulates: /kick #chan nick
    // Parser gives: ["#chan", "nick"]
    assert_eq!(
        ClientCommand::from_parsed("kick", &args(&["#chan", "nick"])),
        Ok(ClientCommand::Kick("#chan".into(), "nick".into(), None))
    );
}

#[test]
fn ban_extracts_mask_from_trailing() {
    // /ban #chan *!*@host extra
    // Parser gives: ["#chan", "*!*@host extra"]
    // ban takes only the first word as mask
    assert_eq!(
        ClientCommand::from_parsed("ban", &args(&["#chan", "*!*@host extra"])),
        Ok(ClientCommand::Ban("#chan".into(), "*!*@host".into()))
    );
}

#[test]
fn oper_extracts_password_from_trailing() {
    // /oper admin secret extra
    // Parser gives: ["admin", "secret extra"]
    // oper takes only the first word as password
    assert_eq!(
        ClientCommand::from_parsed("oper", &args(&["admin", "secret extra"])),
        Ok(ClientCommand::Oper("admin".into(), "secret".into()))
    );
}

#[test]
fn invite_extracts_channel_from_trailing() {
    // /invite nick #chan extra
    // Parser gives: ["nick", "#chan extra"]
    // invite takes only the first word as channel
    assert_eq!(
        ClientCommand::from_parsed("invite", &args(&["nick", "#chan extra"])),
        Ok(ClientCommand::Invite("nick".into(), "#chan".into()))
    );
}

// ════════════════════════════════════════════════════════════════
// to_message() conversion tests
// ════════════════════════════════════════════════════════════════

use pirc_protocol::{Command, Message, PircSubcommand};

// ── Helper: assert message validates ──────────────────────────

fn assert_valid(msg: &Message) {
    msg.validate().unwrap_or_else(|e| {
        panic!("Message failed validation: {e}: {msg:?}");
    });
}

// ── Channel commands → Message ────────────────────────────────

#[test]
fn to_message_join() {
    let cmd = ClientCommand::Join("#test".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Join);
    assert_eq!(msg.params, vec!["#test"]);
    assert_valid(&msg);
}

#[test]
fn to_message_part_with_reason() {
    let cmd = ClientCommand::Part("#chan".into(), Some("Goodbye!".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Part);
    assert_eq!(msg.params, vec!["#chan", "Goodbye!"]);
    assert_valid(&msg);
}

#[test]
fn to_message_part_without_reason() {
    let cmd = ClientCommand::Part("#chan".into(), None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Part);
    assert_eq!(msg.params, vec!["#chan"]);
    assert_valid(&msg);
}

#[test]
fn to_message_topic_query() {
    let cmd = ClientCommand::Topic("#chan".into(), None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Topic);
    assert_eq!(msg.params, vec!["#chan"]);
    assert_valid(&msg);
}

#[test]
fn to_message_topic_set() {
    let cmd = ClientCommand::Topic("#chan".into(), Some("New topic".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Topic);
    assert_eq!(msg.params, vec!["#chan", "New topic"]);
    assert_valid(&msg);
}

#[test]
fn to_message_list_no_pattern() {
    let cmd = ClientCommand::List(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::List);
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_list_with_pattern() {
    let cmd = ClientCommand::List(Some("#rust*".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::List);
    assert_eq!(msg.params, vec!["#rust*"]);
}

#[test]
fn to_message_invite() {
    let cmd = ClientCommand::Invite("nick".into(), "#chan".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Invite);
    assert_eq!(msg.params, vec!["nick", "#chan"]);
    assert_valid(&msg);
}

#[test]
fn to_message_names_no_channel() {
    let cmd = ClientCommand::Names(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Names);
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_names_with_channel() {
    let cmd = ClientCommand::Names(Some("#test".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Names);
    assert_eq!(msg.params, vec!["#test"]);
}

// ── Messaging commands → Message ──────────────────────────────

#[test]
fn to_message_msg() {
    let cmd = ClientCommand::Msg("nick".into(), "hello world".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params, vec!["nick", "hello world"]);
    assert_valid(&msg);
}

#[test]
fn to_message_msg_to_channel() {
    let cmd = ClientCommand::Msg("#channel".into(), "hello everyone".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params, vec!["#channel", "hello everyone"]);
    assert_valid(&msg);
}

#[test]
fn to_message_query_with_message() {
    let cmd = ClientCommand::Query("nick".into(), Some("hey there".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params, vec!["nick", "hey there"]);
    assert_valid(&msg);
}

#[test]
fn to_message_query_without_message_returns_none() {
    let cmd = ClientCommand::Query("nick".into(), None);
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn to_message_me_with_context() {
    let cmd = ClientCommand::Me("waves at everyone".into());
    let msg = cmd.to_message(Some("#channel")).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params[0], "#channel");
    assert_eq!(msg.params[1], "\x01ACTION waves at everyone\x01");
    assert_valid(&msg);
}

#[test]
fn to_message_me_ctcp_action_wrapping() {
    let cmd = ClientCommand::Me("dances".into());
    let msg = cmd.to_message(Some("#test")).unwrap();
    let body = &msg.params[1];
    assert!(body.starts_with('\x01'));
    assert!(body.ends_with('\x01'));
    assert!(body.contains("ACTION dances"));
}

#[test]
fn to_message_me_without_context_returns_none() {
    let cmd = ClientCommand::Me("waves".into());
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn to_message_notice() {
    let cmd = ClientCommand::Notice("user".into(), "This is a notice".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Notice);
    assert_eq!(msg.params, vec!["user", "This is a notice"]);
    assert_valid(&msg);
}

#[test]
fn to_message_ctcp_without_args() {
    let cmd = ClientCommand::Ctcp("nick".into(), "PING".into(), None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params[0], "nick");
    assert_eq!(msg.params[1], "\x01PING\x01");
    assert_valid(&msg);
}

#[test]
fn to_message_ctcp_with_args() {
    let cmd = ClientCommand::Ctcp("nick".into(), "VERSION".into(), Some("extra".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Privmsg);
    assert_eq!(msg.params[0], "nick");
    assert_eq!(msg.params[1], "\x01VERSION extra\x01");
    assert_valid(&msg);
}

// ── User commands → Message ───────────────────────────────────

#[test]
fn to_message_nick() {
    let cmd = ClientCommand::Nick("newnick".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Nick);
    assert_eq!(msg.params, vec!["newnick"]);
    assert_valid(&msg);
}

#[test]
fn to_message_whois() {
    let cmd = ClientCommand::Whois("someuser".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Whois);
    assert_eq!(msg.params, vec!["someuser"]);
    assert_valid(&msg);
}

#[test]
fn to_message_away_with_reason() {
    let cmd = ClientCommand::Away(Some("Gone for lunch".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Away);
    assert_eq!(msg.params, vec!["Gone for lunch"]);
}

#[test]
fn to_message_away_without_reason() {
    let cmd = ClientCommand::Away(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Away);
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_quit_with_reason() {
    let cmd = ClientCommand::Quit(Some("Goodbye cruel world".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Quit);
    assert_eq!(msg.params, vec!["Goodbye cruel world"]);
}

#[test]
fn to_message_quit_without_reason() {
    let cmd = ClientCommand::Quit(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Quit);
    assert!(msg.params.is_empty());
}

// ── Moderation commands → Message ─────────────────────────────

#[test]
fn to_message_kick_with_reason() {
    let cmd = ClientCommand::Kick("#chan".into(), "baduser".into(), Some("Spamming".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Kick);
    assert_eq!(msg.params, vec!["#chan", "baduser", "Spamming"]);
    assert_valid(&msg);
}

#[test]
fn to_message_kick_without_reason() {
    let cmd = ClientCommand::Kick("#chan".into(), "baduser".into(), None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Kick);
    assert_eq!(msg.params, vec!["#chan", "baduser"]);
    assert_valid(&msg);
}

#[test]
fn to_message_ban() {
    let cmd = ClientCommand::Ban("#chan".into(), "*!*@bad.host".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Ban);
    assert_eq!(msg.params, vec!["#chan", "*!*@bad.host"]);
    assert_valid(&msg);
}

#[test]
fn to_message_mode_with_modestring() {
    let cmd = ClientCommand::Mode("#chan".into(), Some("+o user".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Mode);
    assert_eq!(msg.params, vec!["#chan", "+o user"]);
    assert_valid(&msg);
}

#[test]
fn to_message_mode_query() {
    let cmd = ClientCommand::Mode("#chan".into(), None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Mode);
    assert_eq!(msg.params, vec!["#chan"]);
    assert_valid(&msg);
}

// ── Operator commands → Message ───────────────────────────────

#[test]
fn to_message_oper() {
    let cmd = ClientCommand::Oper("admin".into(), "secret".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Oper);
    assert_eq!(msg.params, vec!["admin", "secret"]);
    assert_valid(&msg);
}

#[test]
fn to_message_kill() {
    let cmd = ClientCommand::Kill("baduser".into(), "Spamming".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Kill);
    assert_eq!(msg.params, vec!["baduser", "Spamming"]);
    assert_valid(&msg);
}

#[test]
fn to_message_die_with_reason() {
    let cmd = ClientCommand::Die(Some("shutting down".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Die);
    assert_eq!(msg.params, vec!["shutting down"]);
}

#[test]
fn to_message_die_without_reason() {
    let cmd = ClientCommand::Die(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Die);
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_restart_with_reason() {
    let cmd = ClientCommand::Restart(Some("updating".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Restart);
    assert_eq!(msg.params, vec!["updating"]);
}

#[test]
fn to_message_restart_without_reason() {
    let cmd = ClientCommand::Restart(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Restart);
    assert!(msg.params.is_empty());
}

// ── pirc-specific commands → Message ──────────────────────────

#[test]
fn to_message_cluster_join() {
    let cmd = ClientCommand::Cluster("join".into(), vec!["invite-key-123".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterJoin));
    assert_eq!(msg.params, vec!["invite-key-123"]);
}

#[test]
fn to_message_cluster_sync() {
    let cmd = ClientCommand::Cluster("sync".into(), vec![]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterSync));
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_cluster_heartbeat() {
    let cmd = ClientCommand::Cluster("heartbeat".into(), vec!["server1".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterHeartbeat));
    assert_eq!(msg.params, vec!["server1"]);
}

#[test]
fn to_message_cluster_migrate() {
    let cmd = ClientCommand::Cluster("migrate".into(), vec!["user1".into(), "server2".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterMigrate));
    assert_eq!(msg.params, vec!["user1", "server2"]);
}

#[test]
fn to_message_cluster_raft() {
    let cmd = ClientCommand::Cluster("raft".into(), vec!["vote".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterRaft));
    assert_eq!(msg.params, vec!["vote"]);
}

#[test]
fn to_message_cluster_welcome() {
    let cmd = ClientCommand::Cluster("welcome".into(), vec!["server-id".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterWelcome));
    assert_eq!(msg.params, vec!["server-id"]);
}

#[test]
fn to_message_cluster_case_insensitive() {
    let cmd = ClientCommand::Cluster("JOIN".into(), vec![]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::ClusterJoin));
}

#[test]
fn to_message_invite_key() {
    let cmd = ClientCommand::InviteKey(vec!["generate".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
    assert_eq!(msg.params, vec!["generate"]);
}

#[test]
fn to_message_invite_key_no_args() {
    let cmd = ClientCommand::InviteKey(vec![]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::KeyExchange));
    assert!(msg.params.is_empty());
}

#[test]
fn to_message_network() {
    let cmd = ClientCommand::Network(vec!["status".into()]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::Cap));
    assert_eq!(msg.params, vec!["status"]);
}

#[test]
fn to_message_network_no_args() {
    let cmd = ClientCommand::Network(vec![]);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.command, Command::Pirc(PircSubcommand::Cap));
    assert!(msg.params.is_empty());
}

// ── Meta commands → None ──────────────────────────────────────

#[test]
fn to_message_help_returns_none() {
    let cmd = ClientCommand::Help(None);
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn to_message_help_with_topic_returns_none() {
    let cmd = ClientCommand::Help(Some("join".into()));
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn to_message_unknown_returns_none() {
    let cmd = ClientCommand::Unknown("foobar".into(), vec!["arg1".into()]);
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn to_message_unknown_no_args_returns_none() {
    let cmd = ClientCommand::Unknown("xyz".into(), vec![]);
    assert!(cmd.to_message(None).is_none());
}

// ── Validation tests ──────────────────────────────────────────

#[test]
fn to_message_all_producing_commands_validate() {
    // Test that all commands that produce a Message pass validate().
    let commands: Vec<(ClientCommand, Option<&str>)> = vec![
        (ClientCommand::Join("#test".into()), None),
        (ClientCommand::Part("#chan".into(), None), None),
        (
            ClientCommand::Part("#chan".into(), Some("bye".into())),
            None,
        ),
        (ClientCommand::Topic("#chan".into(), None), None),
        (
            ClientCommand::Topic("#chan".into(), Some("hi".into())),
            None,
        ),
        (ClientCommand::Invite("nick".into(), "#chan".into()), None),
        (ClientCommand::Msg("nick".into(), "hi".into()), None),
        (ClientCommand::Query("nick".into(), Some("hi".into())), None),
        (ClientCommand::Me("waves".into()), Some("#chan")),
        (ClientCommand::Notice("user".into(), "hi".into()), None),
        (
            ClientCommand::Ctcp("nick".into(), "PING".into(), None),
            None,
        ),
        (ClientCommand::Nick("newnick".into()), None),
        (ClientCommand::Whois("user".into()), None),
        (
            ClientCommand::Kick("#chan".into(), "nick".into(), None),
            None,
        ),
        (ClientCommand::Ban("#chan".into(), "mask".into()), None),
        (ClientCommand::Mode("#chan".into(), None), None),
        (ClientCommand::Oper("admin".into(), "pass".into()), None),
        (ClientCommand::Kill("nick".into(), "reason".into()), None),
    ];

    for (cmd, ctx) in &commands {
        let msg = cmd
            .to_message(*ctx)
            .unwrap_or_else(|| panic!("Expected Some(Message) for {cmd:?}"));
        assert_valid(&msg);
    }
}

// ── Wire format tests ─────────────────────────────────────────

#[test]
fn to_message_join_wire_format() {
    let cmd = ClientCommand::Join("#test".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "JOIN #test");
}

#[test]
fn to_message_part_wire_format() {
    let cmd = ClientCommand::Part("#chan".into(), Some("Leaving now".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "PART #chan :Leaving now");
}

#[test]
fn to_message_privmsg_wire_format() {
    let cmd = ClientCommand::Msg("nick".into(), "hello world".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "PRIVMSG nick :hello world");
}

#[test]
fn to_message_me_wire_format() {
    let cmd = ClientCommand::Me("dances".into());
    let msg = cmd.to_message(Some("#test")).unwrap();
    assert_eq!(msg.to_string(), "PRIVMSG #test :\x01ACTION dances\x01");
}

#[test]
fn to_message_nick_wire_format() {
    let cmd = ClientCommand::Nick("newnick".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "NICK newnick");
}

#[test]
fn to_message_quit_wire_format() {
    let cmd = ClientCommand::Quit(Some("Bye".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "QUIT Bye");
}

#[test]
fn to_message_quit_no_reason_wire_format() {
    let cmd = ClientCommand::Quit(None);
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "QUIT");
}

#[test]
fn to_message_notice_wire_format() {
    let cmd = ClientCommand::Notice("user".into(), "important msg".into());
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "NOTICE user :important msg");
}

#[test]
fn to_message_kick_wire_format() {
    let cmd = ClientCommand::Kick("#chan".into(), "user".into(), Some("bad behavior".into()));
    let msg = cmd.to_message(None).unwrap();
    assert_eq!(msg.to_string(), "KICK #chan user :bad behavior");
}

#[test]
fn to_message_no_prefix() {
    // All client-originated messages should have no prefix.
    let cmd = ClientCommand::Join("#test".into());
    let msg = cmd.to_message(None).unwrap();
    assert!(msg.prefix.is_none());
}

// ── Connection commands ───────────────────────────────────

#[test]
fn reconnect_parses() {
    assert_eq!(
        ClientCommand::from_parsed("reconnect", &[]),
        Ok(ClientCommand::Reconnect)
    );
}

#[test]
fn disconnect_parses() {
    assert_eq!(
        ClientCommand::from_parsed("disconnect", &[]),
        Ok(ClientCommand::Disconnect)
    );
}

#[test]
fn reconnect_to_message_is_none() {
    assert!(ClientCommand::Reconnect.to_message(None).is_none());
}

#[test]
fn disconnect_to_message_is_none() {
    assert!(ClientCommand::Disconnect.to_message(None).is_none());
}

// ── Encryption commands ───────────────────────────────────

#[test]
fn encryption_status_parses() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["status"])),
        Ok(ClientCommand::Encryption(EncryptionSubcommand::Status))
    );
}

#[test]
fn encryption_status_case_insensitive() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["STATUS"])),
        Ok(ClientCommand::Encryption(EncryptionSubcommand::Status))
    );
}

#[test]
fn encryption_reset_parses() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["reset", "alice"])),
        Ok(ClientCommand::Encryption(EncryptionSubcommand::Reset(
            "alice".into()
        )))
    );
}

#[test]
fn encryption_reset_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["reset"])),
        Err(CommandError::MissingArgument {
            command: "encryption reset".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn encryption_info_parses() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["info", "bob"])),
        Ok(ClientCommand::Encryption(EncryptionSubcommand::Info(
            "bob".into()
        )))
    );
}

#[test]
fn encryption_info_missing_nick() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["info"])),
        Err(CommandError::MissingArgument {
            command: "encryption info".into(),
            argument: "nick".into(),
        })
    );
}

#[test]
fn encryption_missing_subcommand() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &[]),
        Err(CommandError::MissingArgument {
            command: "encryption".into(),
            argument: "subcommand".into(),
        })
    );
}

#[test]
fn encryption_unknown_subcommand() {
    assert_eq!(
        ClientCommand::from_parsed("encryption", &args(&["bogus"])),
        Err(CommandError::InvalidArgument {
            command: "encryption".into(),
            argument: "subcommand".into(),
            reason: "unknown subcommand 'bogus' (expected: status, reset, info)".into(),
        })
    );
}

#[test]
fn fingerprint_no_args() {
    assert_eq!(
        ClientCommand::from_parsed("fingerprint", &[]),
        Ok(ClientCommand::Fingerprint(None))
    );
}

#[test]
fn fingerprint_with_nick() {
    assert_eq!(
        ClientCommand::from_parsed("fingerprint", &args(&["alice"])),
        Ok(ClientCommand::Fingerprint(Some("alice".into())))
    );
}

#[test]
fn encryption_to_message_is_none() {
    let cmd = ClientCommand::Encryption(EncryptionSubcommand::Status);
    assert!(cmd.to_message(None).is_none());
}

#[test]
fn fingerprint_to_message_is_none() {
    let cmd = ClientCommand::Fingerprint(None);
    assert!(cmd.to_message(None).is_none());
}
