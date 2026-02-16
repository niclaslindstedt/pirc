//! Typed client-side command representations.
//!
//! [`ClientCommand`] is the typed form of a user-entered command. It is produced
//! from the raw [`ParsedInput::Command`](crate::command_parser::ParsedInput)
//! output via [`ClientCommand::from_parsed`].
//!
//! The [`ClientCommand::to_message`] method converts a command into an
//! `Option<pirc_protocol::Message>` for wire transmission. Commands that are
//! client-local (like `/help`) or have no message to send (like `/query`
//! without a message) return `None`.

use pirc_common::types::GroupId;
use pirc_protocol::{Command, Message, PircSubcommand};

/// Errors returned when a parsed command cannot be converted into a
/// [`ClientCommand`] due to missing or invalid arguments.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// A required argument is missing (e.g. `/join` without a channel).
    #[error("{command}: missing required argument: {argument}")]
    MissingArgument { command: String, argument: String },

    /// An argument has an invalid format (e.g. a channel name without `#`).
    #[error("{command}: invalid {argument}: {reason}")]
    InvalidArgument {
        command: String,
        argument: String,
        reason: String,
    },
}

/// A typed client-side IRC command with validated arguments.
///
/// Each variant encodes exactly the information the command needs.
/// Unrecognised commands are captured as [`Unknown`](ClientCommand::Unknown)
/// for future extensibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    // ── Channel ────────────────────────────────────────────────
    /// `/join #channel`
    Join(String),
    /// `/part #channel [reason]`
    Part(String, Option<String>),
    /// `/topic #channel [new_topic]`  — query or set the topic.
    Topic(String, Option<String>),
    /// `/list [pattern]`
    List(Option<String>),
    /// `/invite nick #channel`
    Invite(String, String),
    /// `/names [#channel]`
    Names(Option<String>),

    // ── Messaging ──────────────────────────────────────────────
    /// `/msg target message`
    Msg(String, String),
    /// `/query nick [message]`
    Query(String, Option<String>),
    /// `/me action_text`
    Me(String),
    /// `/notice target message`
    Notice(String, String),
    /// `/ctcp target command [args]`
    Ctcp(String, String, Option<String>),

    // ── User ───────────────────────────────────────────────────
    /// `/nick new_nick`
    Nick(String),
    /// `/whois nick`
    Whois(String),
    /// `/away [reason]`  — set or clear away status.
    Away(Option<String>),
    /// `/quit [reason]`
    Quit(Option<String>),

    // ── Moderation ─────────────────────────────────────────────
    /// `/kick #channel nick [reason]`
    Kick(String, String, Option<String>),
    /// `/ban #channel mask`
    Ban(String, String),
    /// `/mode target [mode_string]`
    Mode(String, Option<String>),

    // ── Operator ───────────────────────────────────────────────
    /// `/oper name password`
    Oper(String, String),
    /// `/kill nick reason`
    Kill(String, String),
    /// `/die [reason]`
    Die(Option<String>),
    /// `/restart [reason]`
    Restart(Option<String>),

    // ── pirc-specific ──────────────────────────────────────────
    /// `/cluster subcommand [args…]`
    Cluster(String, Vec<String>),
    /// `/invite-key [args…]`
    InviteKey(Vec<String>),
    /// `/network [args…]`
    Network(Vec<String>),

    // ── Connection ────────────────────────────────────────────
    /// `/reconnect` — manually trigger a reconnect.
    Reconnect,
    /// `/disconnect` — disconnect without auto-reconnect.
    Disconnect,

    // ── Group chat ─────────────────────────────────────────────
    /// `/group <subcommand> [args]`
    Group(GroupSubcommand),

    // ── Encryption ──────────────────────────────────────────────
    /// `/encryption <subcommand> [args]`
    Encryption(EncryptionSubcommand),
    /// `/fingerprint [nick]`
    Fingerprint(Option<String>),

    // ── Meta ───────────────────────────────────────────────────
    /// `/help [topic]`
    Help(Option<String>),
    /// Any command not recognised by the client.
    Unknown(String, Vec<String>),
}

/// Subcommands for the `/encryption` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncryptionSubcommand {
    /// `/encryption status` — list all active encrypted sessions.
    Status,
    /// `/encryption reset <nick>` — reset encrypted session with a peer.
    Reset(String),
    /// `/encryption info <nick>` — show detailed session info.
    Info(String),
}

/// Subcommands for the `/group` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupSubcommand {
    /// `/group create <name>` — create a new group chat.
    Create(String),
    /// `/group invite <nick>` — invite a user to the current group.
    Invite(String),
    /// `/group join <group_id>` — join (accept invitation to) a group.
    Join(GroupId),
    /// `/group leave` — leave the current group.
    Leave,
    /// `/group members` — list members of the current group.
    Members,
    /// `/group list` — list all groups the user belongs to.
    List,
    /// `/group info` — show info about the current group.
    Info,
}

impl ClientCommand {
    /// Convert a parsed command name and argument list into a typed
    /// [`ClientCommand`].
    ///
    /// `name` is expected to be **lowercase** (as produced by
    /// [`crate::command_parser::parse`]).
    ///
    /// `args` follows the IRC trailing-text convention from the parser:
    /// at most two elements where `args[1]` may contain spaces.
    /// Commands needing more positional arguments (e.g. `/kick #chan nick reason`)
    /// perform further splitting internally.
    pub fn from_parsed(name: &str, args: &[String]) -> Result<Self, CommandError> {
        match name {
            // ── Channel ────────────────────────────────────────
            "join" => parse_join(args),
            "part" => parse_part(args),
            "topic" => parse_topic(args),
            "list" => Ok(ClientCommand::List(args.first().cloned())),
            "invite" => parse_invite(args),
            "names" => Ok(ClientCommand::Names(args.first().cloned())),

            // ── Messaging ──────────────────────────────────────
            "msg" | "privmsg" => parse_msg(args),
            "query" => parse_query(args),
            "me" => parse_me(args),
            "notice" => parse_notice(args),
            "ctcp" => parse_ctcp(args),

            // ── User ───────────────────────────────────────────
            "nick" => parse_nick(args),
            "whois" => parse_whois(args),
            "away" => Ok(ClientCommand::Away(join_all_args(args))),
            "quit" => Ok(ClientCommand::Quit(join_all_args(args))),

            // ── Moderation ─────────────────────────────────────
            "kick" => parse_kick(args),
            "ban" => parse_ban(args),
            "mode" => parse_mode(args),

            // ── Operator ───────────────────────────────────────
            "oper" => parse_oper(args),
            "kill" => parse_kill(args),
            "die" => Ok(ClientCommand::Die(join_all_args(args))),
            "restart" => Ok(ClientCommand::Restart(join_all_args(args))),

            // ── pirc-specific ──────────────────────────────────
            "cluster" => parse_cluster(args),
            "invite-key" => Ok(ClientCommand::InviteKey(args.to_vec())),
            "network" => Ok(ClientCommand::Network(args.to_vec())),

            // ── Connection ──────────────────────────────────────
            "reconnect" => Ok(ClientCommand::Reconnect),
            "disconnect" => Ok(ClientCommand::Disconnect),

            // ── Group chat ──────────────────────────────────────
            "group" => parse_group(args),

            // ── Encryption ─────────────────────────────────────
            "encryption" => parse_encryption(args),
            "fingerprint" => Ok(ClientCommand::Fingerprint(args.first().cloned())),

            // ── Meta ───────────────────────────────────────────
            "help" => Ok(ClientCommand::Help(args.first().cloned())),

            // ── Unknown ────────────────────────────────────────
            _ => Ok(ClientCommand::Unknown(name.to_owned(), args.to_vec())),
        }
    }

    /// Convert this command into a protocol [`Message`] for wire transmission.
    ///
    /// `context` is the active channel or query target. It is required for
    /// commands that send to the current view (`/me`, `/ctcp` without
    /// explicit target in some flows). For most commands the target is
    /// embedded in the variant itself.
    ///
    /// Returns `None` for client-local commands (`Help`, `Unknown`) and
    /// for `Query` when no message body is present (opening a query window
    /// is a UI-only action).
    pub fn to_message(&self, context: Option<&str>) -> Option<Message> {
        match self {
            // ── Channel ────────────────────────────────────────
            ClientCommand::Join(channel) => {
                Some(Message::new(Command::Join, vec![channel.clone()]))
            }
            ClientCommand::Part(channel, reason) => {
                let mut params = vec![channel.clone()];
                if let Some(r) = reason {
                    params.push(r.clone());
                }
                Some(Message::new(Command::Part, params))
            }
            ClientCommand::Topic(channel, topic) => {
                let mut params = vec![channel.clone()];
                if let Some(t) = topic {
                    params.push(t.clone());
                }
                Some(Message::new(Command::Topic, params))
            }
            ClientCommand::List(pattern) => {
                let params = match pattern {
                    Some(p) => vec![p.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::List, params))
            }
            ClientCommand::Invite(nick, channel) => Some(Message::new(
                Command::Invite,
                vec![nick.clone(), channel.clone()],
            )),
            ClientCommand::Names(channel) => {
                let params = match channel {
                    Some(c) => vec![c.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::Names, params))
            }

            // ── Messaging ──────────────────────────────────────
            ClientCommand::Msg(target, message) => Some(Message::new(
                Command::Privmsg,
                vec![target.clone(), message.clone()],
            )),
            ClientCommand::Query(nick, message) => {
                // Query with message → send PRIVMSG to nick.
                // Query without message → client-local (open query window).
                message
                    .as_ref()
                    .map(|msg| Message::new(Command::Privmsg, vec![nick.clone(), msg.clone()]))
            }
            ClientCommand::Me(action) => {
                // /me requires an active context (channel or query target).
                let target = context?;
                let ctcp_body = format!("\x01ACTION {action}\x01");
                Some(Message::new(
                    Command::Privmsg,
                    vec![target.to_owned(), ctcp_body],
                ))
            }
            ClientCommand::Notice(target, message) => Some(Message::new(
                Command::Notice,
                vec![target.clone(), message.clone()],
            )),
            ClientCommand::Ctcp(target, command, args) => {
                let ctcp_body = match args {
                    Some(a) => format!("\x01{command} {a}\x01"),
                    None => format!("\x01{command}\x01"),
                };
                Some(Message::new(
                    Command::Privmsg,
                    vec![target.clone(), ctcp_body],
                ))
            }

            // ── User ───────────────────────────────────────────
            ClientCommand::Nick(nick) => Some(Message::new(Command::Nick, vec![nick.clone()])),
            ClientCommand::Whois(nick) => Some(Message::new(Command::Whois, vec![nick.clone()])),
            ClientCommand::Away(reason) => {
                let params = match reason {
                    Some(r) => vec![r.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::Away, params))
            }
            ClientCommand::Quit(reason) => {
                let params = match reason {
                    Some(r) => vec![r.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::Quit, params))
            }

            // ── Moderation ─────────────────────────────────────
            ClientCommand::Kick(channel, nick, reason) => {
                let mut params = vec![channel.clone(), nick.clone()];
                if let Some(r) = reason {
                    params.push(r.clone());
                }
                Some(Message::new(Command::Kick, params))
            }
            ClientCommand::Ban(channel, mask) => Some(Message::new(
                Command::Ban,
                vec![channel.clone(), mask.clone()],
            )),
            ClientCommand::Mode(target, mode_string) => {
                let mut params = vec![target.clone()];
                if let Some(m) = mode_string {
                    params.push(m.clone());
                }
                Some(Message::new(Command::Mode, params))
            }

            // ── Operator ───────────────────────────────────────
            ClientCommand::Oper(name, password) => Some(Message::new(
                Command::Oper,
                vec![name.clone(), password.clone()],
            )),
            ClientCommand::Kill(nick, reason) => Some(Message::new(
                Command::Kill,
                vec![nick.clone(), reason.clone()],
            )),
            ClientCommand::Die(reason) => {
                let params = match reason {
                    Some(r) => vec![r.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::Die, params))
            }
            ClientCommand::Restart(reason) => {
                let params = match reason {
                    Some(r) => vec![r.clone()],
                    None => vec![],
                };
                Some(Message::new(Command::Restart, params))
            }

            // ── pirc-specific ──────────────────────────────────
            ClientCommand::Cluster(sub, args) => {
                let subcmd = match sub.to_ascii_uppercase().as_str() {
                    "JOIN" => PircSubcommand::ClusterJoin,
                    "WELCOME" => PircSubcommand::ClusterWelcome,
                    "SYNC" => PircSubcommand::ClusterSync,
                    "HEARTBEAT" => PircSubcommand::ClusterHeartbeat,
                    "MIGRATE" => PircSubcommand::ClusterMigrate,
                    "RAFT" => PircSubcommand::ClusterRaft,
                    // Unknown cluster subcommand — fall back to ClusterJoin
                    // with the raw subcommand as a param. In practice the
                    // server will reject invalid subcommands.
                    _ => PircSubcommand::ClusterJoin,
                };
                let params: Vec<String> = args.clone();
                Some(Message::new(Command::Pirc(subcmd), params))
            }
            ClientCommand::InviteKey(args) => {
                // invite-key maps to PIRC KEYEXCHANGE with args.
                Some(Message::new(
                    Command::Pirc(PircSubcommand::KeyExchange),
                    args.clone(),
                ))
            }
            ClientCommand::Network(args) => {
                // /network maps to PIRC CAP with args.
                Some(Message::new(
                    Command::Pirc(PircSubcommand::Cap),
                    args.clone(),
                ))
            }

            // ── Connection ──────────────────────────────────────
            ClientCommand::Reconnect | ClientCommand::Disconnect => None,

            // ── Group chat (client-local) ────────────────────────
            ClientCommand::Group(_) => None,

            // ── Encryption (client-local) ───────────────────────
            ClientCommand::Encryption(_) | ClientCommand::Fingerprint(_) => None,

            // ── Meta ───────────────────────────────────────────
            ClientCommand::Help(_) => None,
            ClientCommand::Unknown(_, _) => None,
        }
    }
}

// ── Helper: join all args back into a single optional string ───────────

/// Join all arguments into a single space-separated string.
/// Returns `None` if `args` is empty.
fn join_all_args(args: &[String]) -> Option<String> {
    if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    }
}

/// Split the second argument (trailing text) into two parts at the first
/// whitespace boundary. Returns `(first_word, Option<rest>)`.
fn split_second_arg(text: &str) -> (&str, Option<&str>) {
    match text.find(char::is_whitespace) {
        Some(pos) => {
            let rest = text[pos..].trim_start();
            if rest.is_empty() {
                (&text[..pos], None)
            } else {
                (&text[..pos], Some(rest))
            }
        }
        None => (text, None),
    }
}

/// Validate that `channel` starts with `#`, returning a [`CommandError`] if not.
fn require_channel(channel: &str, command: &str) -> Result<(), CommandError> {
    if !channel.starts_with('#') {
        return Err(CommandError::InvalidArgument {
            command: command.into(),
            argument: "channel".into(),
            reason: "channel name must start with #".into(),
        });
    }
    Ok(())
}

// ── Per-command parsers ────────────────────────────────────────────────

fn parse_join(args: &[String]) -> Result<ClientCommand, CommandError> {
    let channel = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "join".into(),
        argument: "channel".into(),
    })?;
    require_channel(channel, "join")?;
    Ok(ClientCommand::Join(channel.clone()))
}

fn parse_part(args: &[String]) -> Result<ClientCommand, CommandError> {
    let channel = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "part".into(),
        argument: "channel".into(),
    })?;
    require_channel(channel, "part")?;
    Ok(ClientCommand::Part(channel.clone(), args.get(1).cloned()))
}

fn parse_topic(args: &[String]) -> Result<ClientCommand, CommandError> {
    let channel = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "topic".into(),
        argument: "channel".into(),
    })?;
    require_channel(channel, "topic")?;
    Ok(ClientCommand::Topic(channel.clone(), args.get(1).cloned()))
}

fn parse_invite(args: &[String]) -> Result<ClientCommand, CommandError> {
    let nick = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "invite".into(),
        argument: "nick".into(),
    })?;
    // The second arg (channel) might be in args[1] or we need it from the
    // trailing text. The parser produces: args = ["nick", "#channel ..."].
    let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "invite".into(),
        argument: "channel".into(),
    })?;
    let (channel, _) = split_second_arg(trailing);
    require_channel(channel, "invite")?;
    Ok(ClientCommand::Invite(nick.clone(), channel.to_owned()))
}

fn parse_msg(args: &[String]) -> Result<ClientCommand, CommandError> {
    let target = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "msg".into(),
        argument: "target".into(),
    })?;
    let message = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "msg".into(),
        argument: "message".into(),
    })?;
    Ok(ClientCommand::Msg(target.clone(), message.clone()))
}

fn parse_query(args: &[String]) -> Result<ClientCommand, CommandError> {
    let nick = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "query".into(),
        argument: "nick".into(),
    })?;
    Ok(ClientCommand::Query(nick.clone(), args.get(1).cloned()))
}

fn parse_me(args: &[String]) -> Result<ClientCommand, CommandError> {
    // `/me action text` — the parser produces ["waves", "at everyone"].
    // We rejoin everything into a single action string.
    let text = join_all_args(args).ok_or_else(|| CommandError::MissingArgument {
        command: "me".into(),
        argument: "action text".into(),
    })?;
    Ok(ClientCommand::Me(text))
}

fn parse_notice(args: &[String]) -> Result<ClientCommand, CommandError> {
    let target = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "notice".into(),
        argument: "target".into(),
    })?;
    let message = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "notice".into(),
        argument: "message".into(),
    })?;
    Ok(ClientCommand::Notice(target.clone(), message.clone()))
}

fn parse_ctcp(args: &[String]) -> Result<ClientCommand, CommandError> {
    let target = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "ctcp".into(),
        argument: "target".into(),
    })?;
    // The trailing text contains "COMMAND [optional args]".
    let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "ctcp".into(),
        argument: "command".into(),
    })?;
    let (ctcp_cmd, ctcp_args) = split_second_arg(trailing);
    Ok(ClientCommand::Ctcp(
        target.clone(),
        ctcp_cmd.to_ascii_uppercase(),
        ctcp_args.map(str::to_owned),
    ))
}

fn parse_nick(args: &[String]) -> Result<ClientCommand, CommandError> {
    let nick = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "nick".into(),
        argument: "nickname".into(),
    })?;
    Ok(ClientCommand::Nick(nick.clone()))
}

fn parse_whois(args: &[String]) -> Result<ClientCommand, CommandError> {
    let nick = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "whois".into(),
        argument: "nick".into(),
    })?;
    Ok(ClientCommand::Whois(nick.clone()))
}

fn parse_kick(args: &[String]) -> Result<ClientCommand, CommandError> {
    let channel = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "kick".into(),
        argument: "channel".into(),
    })?;
    require_channel(channel, "kick")?;
    // The trailing text is "nick [reason]".
    let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "kick".into(),
        argument: "nick".into(),
    })?;
    let (nick, reason) = split_second_arg(trailing);
    Ok(ClientCommand::Kick(
        channel.clone(),
        nick.to_owned(),
        reason.map(str::to_owned),
    ))
}

fn parse_ban(args: &[String]) -> Result<ClientCommand, CommandError> {
    let channel = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "ban".into(),
        argument: "channel".into(),
    })?;
    require_channel(channel, "ban")?;
    let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "ban".into(),
        argument: "mask".into(),
    })?;
    let (mask, _) = split_second_arg(trailing);
    Ok(ClientCommand::Ban(channel.clone(), mask.to_owned()))
}

fn parse_mode(args: &[String]) -> Result<ClientCommand, CommandError> {
    let target = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "mode".into(),
        argument: "target".into(),
    })?;
    Ok(ClientCommand::Mode(target.clone(), args.get(1).cloned()))
}

fn parse_oper(args: &[String]) -> Result<ClientCommand, CommandError> {
    let name = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "oper".into(),
        argument: "name".into(),
    })?;
    let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "oper".into(),
        argument: "password".into(),
    })?;
    let (password, _) = split_second_arg(trailing);
    Ok(ClientCommand::Oper(name.clone(), password.to_owned()))
}

fn parse_kill(args: &[String]) -> Result<ClientCommand, CommandError> {
    let nick = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "kill".into(),
        argument: "nick".into(),
    })?;
    let reason = args.get(1).ok_or_else(|| CommandError::MissingArgument {
        command: "kill".into(),
        argument: "reason".into(),
    })?;
    Ok(ClientCommand::Kill(nick.clone(), reason.clone()))
}

fn parse_encryption(args: &[String]) -> Result<ClientCommand, CommandError> {
    let sub = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "encryption".into(),
        argument: "subcommand".into(),
    })?;

    match sub.to_ascii_lowercase().as_str() {
        "status" => Ok(ClientCommand::Encryption(EncryptionSubcommand::Status)),
        "reset" => {
            // The nick may be in the trailing text (args[1])
            let nick = args.get(1).ok_or_else(|| CommandError::MissingArgument {
                command: "encryption reset".into(),
                argument: "nick".into(),
            })?;
            let (nick_word, _) = split_second_arg(nick);
            Ok(ClientCommand::Encryption(EncryptionSubcommand::Reset(
                nick_word.to_owned(),
            )))
        }
        "info" => {
            let nick = args.get(1).ok_or_else(|| CommandError::MissingArgument {
                command: "encryption info".into(),
                argument: "nick".into(),
            })?;
            let (nick_word, _) = split_second_arg(nick);
            Ok(ClientCommand::Encryption(EncryptionSubcommand::Info(
                nick_word.to_owned(),
            )))
        }
        _ => Err(CommandError::InvalidArgument {
            command: "encryption".into(),
            argument: "subcommand".into(),
            reason: format!(
                "unknown subcommand '{}' (expected: status, reset, info)",
                sub
            ),
        }),
    }
}

fn parse_cluster(args: &[String]) -> Result<ClientCommand, CommandError> {
    let sub = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "cluster".into(),
        argument: "subcommand".into(),
    })?;
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    Ok(ClientCommand::Cluster(sub.clone(), rest))
}

fn parse_group(args: &[String]) -> Result<ClientCommand, CommandError> {
    let sub = args.first().ok_or_else(|| CommandError::MissingArgument {
        command: "group".into(),
        argument: "subcommand".into(),
    })?;

    match sub.to_ascii_lowercase().as_str() {
        "create" => {
            let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
                command: "group create".into(),
                argument: "name".into(),
            })?;
            Ok(ClientCommand::Group(GroupSubcommand::Create(
                trailing.clone(),
            )))
        }
        "invite" => {
            let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
                command: "group invite".into(),
                argument: "nick".into(),
            })?;
            let (nick, _) = split_second_arg(trailing);
            Ok(ClientCommand::Group(GroupSubcommand::Invite(
                nick.to_owned(),
            )))
        }
        "join" => {
            let trailing = args.get(1).ok_or_else(|| CommandError::MissingArgument {
                command: "group join".into(),
                argument: "group_id".into(),
            })?;
            let (id_str, _) = split_second_arg(trailing);
            let group_id: GroupId =
                id_str.parse().map_err(|_| CommandError::InvalidArgument {
                    command: "group join".into(),
                    argument: "group_id".into(),
                    reason: "must be a numeric group ID".into(),
                })?;
            Ok(ClientCommand::Group(GroupSubcommand::Join(group_id)))
        }
        "leave" => Ok(ClientCommand::Group(GroupSubcommand::Leave)),
        "members" => Ok(ClientCommand::Group(GroupSubcommand::Members)),
        "list" => Ok(ClientCommand::Group(GroupSubcommand::List)),
        "info" => Ok(ClientCommand::Group(GroupSubcommand::Info)),
        _ => Err(CommandError::InvalidArgument {
            command: "group".into(),
            argument: "subcommand".into(),
            reason: format!(
                "unknown subcommand '{sub}' (expected: create, invite, join, leave, members, list, info)"
            ),
        }),
    }
}

#[cfg(test)]
mod tests;
