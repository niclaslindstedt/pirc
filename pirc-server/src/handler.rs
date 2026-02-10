use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::{Nickname, UserError};
use pirc_common::UserMode;
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_ERRONEUSNICKNAME, ERR_NEEDMOREPARAMS, ERR_NICKNAMEINUSE,
    ERR_NOMOTD, ERR_NONICKNAMEGIVEN, ERR_NOSUCHNICK, ERR_UMODEUNKNOWNFLAG, ERR_USERSDONTMATCH,
    RPL_AWAY, RPL_CREATED, RPL_ENDOFWHOIS, RPL_NOWAWAY, RPL_UMODEIS, RPL_UNAWAY, RPL_WELCOME,
    RPL_WHOISOPERATOR, RPL_WHOISIDLE, RPL_WHOISSERVER, RPL_WHOISUSER, RPL_YOURHOST,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::warn;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::handler_channel::{handle_ban, handle_channel_mode, handle_invite, handle_join, handle_kick, handle_notice, handle_part, handle_privmsg, handle_topic, remove_user_from_all_channels};
use crate::registry::UserRegistry;
use crate::user::UserSession;

pub(crate) const SERVER_NAME: &str = "pircd";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tracks partial client state before NICK + USER are both received.
pub struct PreRegistrationState {
    pub nick: Option<Nickname>,
    pub username: Option<String>,
    pub realname: Option<String>,
    pub hostname: String,
    pub registered: bool,
}

impl PreRegistrationState {
    pub fn new(hostname: String) -> Self {
        Self {
            nick: None,
            username: None,
            realname: None,
            hostname,
            registered: false,
        }
    }

    fn is_ready(&self) -> bool {
        self.nick.is_some() && self.username.is_some()
    }
}

/// Result of handling a message, indicating whether the connection should close.
pub enum HandleResult {
    /// Continue processing messages from this connection.
    Continue,
    /// The client sent QUIT; the connection should be closed.
    Quit,
}

/// Handle a single parsed message from a client connection.
///
/// For pre-registration clients, only NICK, USER, PING, PONG, and QUIT are processed.
/// Once both NICK and USER have been received, the client is registered in the
/// `UserRegistry` and the welcome burst is sent.
///
/// Returns [`HandleResult::Quit`] when the client sends QUIT, signalling the
/// connection loop to stop reading and clean up.
pub fn handle_message(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
    config: &ServerConfig,
) -> HandleResult {
    if state.registered {
        // Update idle tracking for non-PING/PONG commands.
        if !matches!(msg.command, Command::Ping | Command::Pong) {
            if let Some(session_arc) = registry.get_by_connection(connection_id) {
                let mut session = session_arc.write().expect("session lock poisoned");
                session.last_active = Instant::now();
            }
        }

        // Post-registration command dispatch.
        match &msg.command {
            Command::Quit => {
                handle_quit(msg, connection_id, registry, channels, sender, state);
                return HandleResult::Quit;
            }
            Command::Nick => handle_nick_change(msg, connection_id, registry, sender),
            Command::User => handle_user(msg, sender, state),
            Command::Whois => handle_whois(msg, connection_id, registry, sender),
            Command::Away => handle_away(msg, connection_id, registry, sender),
            Command::Mode => {
                // Route to channel or user mode handler based on target.
                if msg.params.first().is_some_and(|t| t.starts_with('#') || t.starts_with('&')) {
                    handle_channel_mode(msg, connection_id, registry, channels, sender)
                } else {
                    handle_user_mode(msg, connection_id, registry, sender)
                }
            }
            Command::Join => handle_join(msg, connection_id, registry, channels, sender),
            Command::Part => handle_part(msg, connection_id, registry, channels, sender),
            Command::Topic => handle_topic(msg, connection_id, registry, channels, sender),
            Command::Kick => handle_kick(msg, connection_id, registry, channels, sender),
            Command::Invite => handle_invite(msg, connection_id, registry, channels, sender),
            Command::Ban => handle_ban(msg, connection_id, registry, channels, sender),
            Command::Privmsg => handle_privmsg(msg, connection_id, registry, channels, sender),
            Command::Notice => handle_notice(msg, connection_id, registry, channels, sender),
            Command::Ping => handle_ping(msg, sender),
            // PONG and other commands are silently absorbed.
            _ => {}
        }
        return HandleResult::Continue;
    }

    // Pre-registration command dispatch.
    match &msg.command {
        Command::Quit => return HandleResult::Quit,
        Command::Nick => handle_nick(msg, registry, sender, state),
        Command::User => handle_user(msg, sender, state),
        Command::Ping => {
            handle_ping(msg, sender);
            return HandleResult::Continue;
        }
        // PONG and other unhandled commands are ignored pre-registration.
        _ => return HandleResult::Continue,
    }

    // After processing NICK or USER, check if registration can complete.
    if state.is_ready() {
        complete_registration(connection_id, registry, sender, state, config);
    }
    HandleResult::Continue
}

fn handle_nick(
    msg: &Message,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    if msg.params.is_empty() {
        send_numeric(sender, ERR_NONICKNAMEGIVEN, &["*"], "No nickname given");
        return;
    }

    let nick_str = &msg.params[0];
    let Ok(nick) = Nickname::new(nick_str) else {
        send_numeric(
            sender,
            ERR_ERRONEUSNICKNAME,
            &[nick_str],
            "Erroneous nickname",
        );
        return;
    };

    if registry.nick_in_use(&nick) {
        send_numeric(
            sender,
            ERR_NICKNAMEINUSE,
            &[nick.as_ref()],
            "Nickname is already in use",
        );
        return;
    }

    state.nick = Some(nick);
}

/// Handle a NICK command from an already-registered user.
///
/// Validates the new nickname, checks for collisions, atomically updates
/// the registry, and sends the NICK confirmation with the old prefix.
fn handle_nick_change(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    if msg.params.is_empty() {
        send_numeric(sender, ERR_NONICKNAMEGIVEN, &["*"], "No nickname given");
        return;
    }

    let nick_str = &msg.params[0];
    let Ok(new_nick) = Nickname::new(nick_str) else {
        send_numeric(
            sender,
            ERR_ERRONEUSNICKNAME,
            &[nick_str],
            "Erroneous nickname",
        );
        return;
    };

    // Look up the current session to get old nick and user/host info.
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (old_nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };

    // Attempt the nick change in the registry.
    match registry.change_nick(&old_nick, new_nick.clone()) {
        Ok(()) => {
            // Send NICK confirmation with old prefix: :oldnick!user@host NICK newnick
            let nick_msg = Message::builder(Command::Nick)
                .prefix(Prefix::User {
                    nick: old_nick,
                    user: username,
                    host: hostname,
                })
                .param(new_nick.as_ref())
                .build();
            let _ = sender.send(nick_msg);

            // Update last_active timestamp.
            let mut session = session_arc.write().expect("session lock poisoned");
            session.last_active = Instant::now();
        }
        Err(UserError::NickInUse { .. }) => {
            let current_nick = {
                let session = session_arc.read().expect("session lock poisoned");
                session.nickname.clone()
            };
            send_numeric(
                sender,
                ERR_NICKNAMEINUSE,
                &[current_nick.as_ref(), new_nick.as_ref()],
                "Nickname is already in use",
            );
        }
        Err(_) => {}
    }
}

/// Handle a WHOIS query from a registered user.
///
/// Looks up the target nick in the registry and sends the standard WHOIS
/// reply sequence: RPL_WHOISUSER, RPL_WHOISSERVER, optional RPL_WHOISOPERATOR,
/// optional RPL_AWAY, RPL_WHOISIDLE, and RPL_ENDOFWHOIS.
fn handle_whois(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    // Get the requestor's nick for reply addressing.
    let requestor_nick = match registry.get_by_connection(connection_id) {
        Some(session_arc) => {
            let session = session_arc.read().expect("session lock poisoned");
            session.nickname.to_string()
        }
        None => return,
    };

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NONICKNAMEGIVEN,
            &[&requestor_nick],
            "No nickname given",
        );
        return;
    }

    let target_nick_str = &msg.params[0];
    let target_nick = match Nickname::new(target_nick_str) {
        Ok(n) => n,
        Err(_) => {
            send_numeric(
                sender,
                ERR_NOSUCHNICK,
                &[&requestor_nick, target_nick_str],
                "No such nick/channel",
            );
            send_numeric(
                sender,
                RPL_ENDOFWHOIS,
                &[&requestor_nick, target_nick_str],
                "End of /WHOIS list",
            );
            return;
        }
    };

    let Some(target_session_arc) = registry.get_by_nick(&target_nick) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[&requestor_nick, target_nick_str],
            "No such nick/channel",
        );
        send_numeric(
            sender,
            RPL_ENDOFWHOIS,
            &[&requestor_nick, target_nick_str],
            "End of /WHOIS list",
        );
        return;
    };

    let target = target_session_arc.read().expect("session lock poisoned");
    let nick = target.nickname.to_string();

    // RPL_WHOISUSER (311): <requestor> <nick> <user> <host> * :<realname>
    send_numeric(
        sender,
        RPL_WHOISUSER,
        &[&requestor_nick, &nick, &target.username, &target.hostname, "*"],
        &target.realname,
    );

    // RPL_WHOISSERVER (312): <requestor> <nick> <server> :<server info>
    send_numeric(
        sender,
        RPL_WHOISSERVER,
        &[&requestor_nick, &nick, SERVER_NAME],
        &format!("{SERVER_NAME} {SERVER_VERSION}"),
    );

    // RPL_WHOISOPERATOR (313): if target is an operator
    if target.modes.contains(&UserMode::Operator) {
        send_numeric(
            sender,
            RPL_WHOISOPERATOR,
            &[&requestor_nick, &nick],
            "is an IRC operator",
        );
    }

    // RPL_AWAY (301): if target is away
    if let Some(ref away_msg) = target.away_message {
        send_numeric(sender, RPL_AWAY, &[&requestor_nick, &nick], away_msg);
    }

    // RPL_WHOISIDLE (317): <requestor> <nick> <idle_secs> <signon> :seconds idle, signon time
    let idle_secs = target.last_active.elapsed().as_secs();
    let signon = target.signon_time;
    send_numeric(
        sender,
        RPL_WHOISIDLE,
        &[
            &requestor_nick,
            &nick,
            &idle_secs.to_string(),
            &signon.to_string(),
        ],
        "seconds idle, signon time",
    );

    // RPL_ENDOFWHOIS (318)
    send_numeric(
        sender,
        RPL_ENDOFWHOIS,
        &[&requestor_nick, &nick],
        "End of /WHOIS list",
    );
}

/// Handle the AWAY command from a registered user.
///
/// `AWAY :message` sets the user as away. `AWAY` (no params) clears away status.
fn handle_away(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.to_string()
    };

    if msg.params.is_empty() {
        // Clear away status.
        {
            let mut session = session_arc.write().expect("session lock poisoned");
            session.away_message = None;
        }
        send_numeric(
            sender,
            RPL_UNAWAY,
            &[&nick],
            "You are no longer marked as being away",
        );
    } else {
        // Set away with message.
        let away_msg = msg.params[0].clone();
        {
            let mut session = session_arc.write().expect("session lock poisoned");
            session.away_message = Some(away_msg);
        }
        send_numeric(
            sender,
            RPL_NOWAWAY,
            &[&nick],
            "You have been marked as being away",
        );
    }
}

/// Handle the MODE command targeting a user nick.
///
/// - `MODE <own-nick>` → RPL_UMODEIS with current modes
/// - `MODE <own-nick> <modestring>` → apply mode changes
/// - `MODE <other-nick>` → ERR_USERSDONTMATCH
fn handle_user_mode(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let nick = {
        let session = session_arc.read().expect("session lock poisoned");
        session.nickname.to_string()
    };

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick, "MODE"],
            "Not enough parameters",
        );
        return;
    }

    let target = &msg.params[0];

    // Check if target is this user (case-insensitive).
    let is_self = {
        let session = session_arc.read().expect("session lock poisoned");
        target.eq_ignore_ascii_case(session.nickname.as_ref())
    };

    if !is_self {
        send_numeric(
            sender,
            ERR_USERSDONTMATCH,
            &[&nick],
            "Cannot change mode for other users",
        );
        return;
    }

    if msg.params.len() < 2 {
        // Mode query: return current modes.
        let mode_string = {
            let session = session_arc.read().expect("session lock poisoned");
            format_user_modes(&session.modes)
        };
        send_numeric(sender, RPL_UMODEIS, &[&nick, &mode_string], "");
        return;
    }

    // Mode set: parse and apply the mode string.
    let modestring = &msg.params[1];
    let mut adding = true;
    let mut unknown = false;

    for ch in modestring.chars() {
        match ch {
            '+' => adding = true,
            '-' => adding = false,
            'o' => {
                // Cannot self-promote to operator via MODE; can only remove.
                if !adding {
                    let mut session = session_arc.write().expect("session lock poisoned");
                    session.modes.remove(&UserMode::Operator);
                }
            }
            'v' => {
                let mut session = session_arc.write().expect("session lock poisoned");
                if adding {
                    session.modes.insert(UserMode::Voiced);
                } else {
                    session.modes.remove(&UserMode::Voiced);
                }
            }
            _ => {
                unknown = true;
            }
        }
    }

    if unknown {
        send_numeric(
            sender,
            ERR_UMODEUNKNOWNFLAG,
            &[&nick],
            "Unknown MODE flag",
        );
    }

    // Confirm current modes after changes.
    let mode_string = {
        let session = session_arc.read().expect("session lock poisoned");
        format_user_modes(&session.modes)
    };
    send_numeric(sender, RPL_UMODEIS, &[&nick, &mode_string], "");
}

/// Format user modes as a mode string like `+ov` or `+`.
fn format_user_modes(modes: &HashSet<UserMode>) -> String {
    let mut chars: Vec<char> = modes.iter().filter_map(|m| m.mode_char()).collect();
    chars.sort();
    format!("+{}", chars.into_iter().collect::<String>())
}

fn handle_user(
    msg: &Message,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    if state.registered {
        send_numeric(
            sender,
            ERR_ALREADYREGISTERED,
            &["*"],
            "You may not reregister",
        );
        return;
    }

    // USER <username> <mode> <unused> :<realname>
    if msg.params.len() < 4 {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &["USER"],
            "Not enough parameters",
        );
        return;
    }

    state.username = Some(msg.params[0].clone());
    state.realname = Some(msg.params[3].clone());
}

fn complete_registration(
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
    config: &ServerConfig,
) {
    let nick = state.nick.clone().expect("nick set before registration");
    let username = state
        .username
        .clone()
        .expect("username set before registration");
    let realname = state
        .realname
        .clone()
        .expect("realname set before registration");

    let now = Instant::now();
    let signon_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let session = UserSession {
        connection_id,
        nickname: nick.clone(),
        username: username.clone(),
        realname: realname.clone(),
        hostname: state.hostname.clone(),
        modes: HashSet::new(),
        away_message: None,
        connected_at: now,
        signon_time,
        last_active: now,
        registered: true,
        sender: sender.clone(),
    };

    if let Err(e) = registry.register(session) {
        warn!(connection_id, "registration failed: {e}");
        send_numeric(
            sender,
            ERR_NICKNAMEINUSE,
            &[nick.as_ref()],
            "Nickname is already in use",
        );
        state.nick = None;
        return;
    }

    state.registered = true;

    let nick_str = nick.as_ref();
    let user_host = format!("{username}@{}", state.hostname);

    // RPL_WELCOME (001)
    send_numeric(
        sender,
        RPL_WELCOME,
        &[nick_str],
        &format!("Welcome to the pirc network, {nick_str}!{user_host}"),
    );

    // RPL_YOURHOST (002)
    send_numeric(
        sender,
        RPL_YOURHOST,
        &[nick_str],
        &format!("Your host is {SERVER_NAME}, running version {SERVER_VERSION}"),
    );

    // RPL_CREATED (003)
    send_numeric(
        sender,
        RPL_CREATED,
        &[nick_str],
        &format!("This server was created {SERVER_NAME}"),
    );

    // MOTD or ERR_NOMOTD
    send_motd(sender, nick_str, config);
}

fn send_motd(sender: &mpsc::UnboundedSender<Message>, nick: &str, config: &ServerConfig) {
    let file_motd;
    let motd = if let Some(ref text) = config.motd.text {
        Some(text.as_str())
    } else if let Some(ref path) = config.motd.path {
        file_motd = std::fs::read_to_string(path).ok();
        file_motd.as_deref()
    } else {
        None
    };

    match motd {
        Some(text) => {
            send_numeric(
                sender,
                pirc_protocol::numeric::RPL_MOTDSTART,
                &[nick],
                &format!("- {SERVER_NAME} Message of the day -"),
            );
            for line in text.lines() {
                send_numeric(
                    sender,
                    pirc_protocol::numeric::RPL_MOTD,
                    &[nick],
                    &format!("- {line}"),
                );
            }
            send_numeric(
                sender,
                pirc_protocol::numeric::RPL_ENDOFMOTD,
                &[nick],
                "End of /MOTD command",
            );
        }
        None => {
            send_numeric(sender, ERR_NOMOTD, &[nick], "MOTD File is missing");
        }
    }
}

fn handle_ping(msg: &Message, sender: &mpsc::UnboundedSender<Message>) {
    if let Some(token) = msg.params.first() {
        let pong = Message::builder(Command::Pong)
            .prefix(Prefix::server(SERVER_NAME))
            .param(token)
            .build();
        let _ = sender.send(pong);
    }
}

/// Handle the QUIT command from a registered user.
///
/// Removes the user from all channels, cleans up empty channels,
/// removes the user from the registry, and sends an ERROR closing link message.
fn handle_quit(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    let quit_message = msg.params.first().map_or("Client Quit", |s| s.as_str());

    let (hostname, nickname) = if let Some(session_arc) = registry.get_by_connection(connection_id)
    {
        let session = session_arc.read().expect("session lock poisoned");
        (session.hostname.clone(), Some(session.nickname.clone()))
    } else {
        (state.hostname.clone(), None)
    };

    // Remove user from all channels.
    if let Some(ref nick) = nickname {
        remove_user_from_all_channels(nick, channels);
    }

    // Remove user from registry.
    if state.registered {
        registry.remove_by_connection(connection_id);
        state.registered = false;
    }

    // Send ERROR closing link.
    let error_msg = Message::builder(Command::Error)
        .trailing(&format!("Closing Link: {hostname} (Quit: {quit_message})"))
        .build();
    let _ = sender.send(error_msg);
}

pub(crate) fn send_numeric(
    sender: &mpsc::UnboundedSender<Message>,
    code: u16,
    params: &[&str],
    trailing: &str,
) {
    let mut builder = Message::builder(Command::Numeric(code)).prefix(Prefix::server(SERVER_NAME));
    for p in params {
        builder = builder.param(p);
    }
    builder = builder.trailing(trailing);
    let _ = sender.send(builder.build());
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "whois_tests.rs"]
mod whois_tests;

#[cfg(test)]
#[path = "quit_ping_tests.rs"]
mod quit_ping_tests;

#[cfg(test)]
#[path = "join_part_tests.rs"]
mod join_part_tests;

#[cfg(test)]
#[path = "topic_tests.rs"]
mod topic_tests;

#[cfg(test)]
#[path = "kick_tests.rs"]
mod kick_tests;

#[cfg(test)]
#[path = "channel_mode_tests.rs"]
mod channel_mode_tests;

#[cfg(test)]
#[path = "ban_invite_tests.rs"]
mod ban_invite_tests;

#[cfg(test)]
#[path = "privmsg_notice_tests.rs"]
mod privmsg_notice_tests;
