use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::{ChannelMode, ChannelName, Nickname, UserError};
use pirc_common::UserMode;
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_BADCHANNELKEY, ERR_BANNEDCHANNEL, ERR_CHANNELISFULL,
    ERR_ERRONEUSNICKNAME, ERR_INVITEONLYCHAN, ERR_NEEDMOREPARAMS, ERR_NICKNAMEINUSE, ERR_NOMOTD,
    ERR_NONICKNAMEGIVEN, ERR_NOSUCHCHANNEL, ERR_NOSUCHNICK, ERR_NOTONCHANNEL, ERR_UMODEUNKNOWNFLAG,
    ERR_USERSDONTMATCH, RPL_AWAY, RPL_CREATED, RPL_ENDOFNAMES, RPL_ENDOFWHOIS, RPL_NAMREPLY,
    RPL_NOTOPIC, RPL_NOWAWAY, RPL_TOPIC, RPL_TOPICWHOTIME, RPL_UMODEIS, RPL_UNAWAY, RPL_WELCOME,
    RPL_WHOISOPERATOR, RPL_WHOISIDLE, RPL_WHOISSERVER, RPL_WHOISUSER, RPL_YOURHOST,
};
use pirc_protocol::{Command, Message, Prefix};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::warn;

use crate::channel::MemberStatus;
use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::registry::UserRegistry;
use crate::user::UserSession;

const SERVER_NAME: &str = "pircd";
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
            Command::Mode => handle_user_mode(msg, connection_id, registry, sender),
            Command::Join => handle_join(msg, connection_id, registry, channels, sender),
            Command::Part => handle_part(msg, connection_id, registry, channels, sender),
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

/// Handle the JOIN command from a registered user.
///
/// Supports comma-separated channel names: `JOIN #chan1,#chan2 [key1,key2]`
/// Creates channels on first join, grants +o to first user, enforces mode
/// restrictions (+i, +k, +l, +b), broadcasts JOIN to channel members,
/// and sends topic + NAMES to the joining user.
fn handle_join(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };
    let nick_str = nick.to_string();

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick_str, "JOIN"],
            "Not enough parameters",
        );
        return;
    }

    let channel_names: Vec<&str> = msg.params[0].split(',').collect();
    let keys: Vec<&str> = if msg.params.len() > 1 {
        msg.params[1].split(',').collect()
    } else {
        Vec::new()
    };

    for (i, chan_str) in channel_names.iter().enumerate() {
        let chan_str = chan_str.trim();
        if chan_str.is_empty() {
            continue;
        }

        // Validate channel name.
        let Ok(chan_name) = ChannelName::new(chan_str) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        let key = keys.get(i).copied();

        // Get or create the channel.
        let channel_arc = channels.get_or_create(chan_name.clone());
        let is_new;
        {
            let mut channel = channel_arc.write().expect("channel lock poisoned");

            // Check if already a member.
            if channel.members.contains_key(&nick) {
                // Silently ignore duplicate JOIN per IRC convention.
                continue;
            }

            is_new = channel.members.is_empty();

            // Enforce channel mode restrictions (skip for new channels).
            if !is_new {
                // +b: check ban list
                let user_mask = format!("{}!{}@{}", nick_str, username, hostname);
                if is_banned(&channel.ban_list, &user_mask) {
                    send_numeric(
                        sender,
                        ERR_BANNEDCHANNEL,
                        &[&nick_str, chan_str],
                        "Cannot join channel (+b)",
                    );
                    continue;
                }

                // +i: invite only
                if channel.modes.contains(&ChannelMode::InviteOnly)
                    && !channel.invite_list.contains(&nick)
                {
                    send_numeric(
                        sender,
                        ERR_INVITEONLYCHAN,
                        &[&nick_str, chan_str],
                        "Cannot join channel (+i)",
                    );
                    continue;
                }

                // +k: key required
                if let Some(ref chan_key) = channel.key {
                    match key {
                        Some(provided) if provided == chan_key => {}
                        _ => {
                            send_numeric(
                                sender,
                                ERR_BADCHANNELKEY,
                                &[&nick_str, chan_str],
                                "Cannot join channel (+k)",
                            );
                            continue;
                        }
                    }
                }

                // +l: user limit
                if let Some(limit) = channel.user_limit {
                    if channel.members.len() as u32 >= limit {
                        send_numeric(
                            sender,
                            ERR_CHANNELISFULL,
                            &[&nick_str, chan_str],
                            "Cannot join channel (+l)",
                        );
                        continue;
                    }
                }
            }

            // Add user to channel. First user gets +o.
            let status = if is_new {
                MemberStatus::Operator
            } else {
                MemberStatus::Normal
            };
            channel.members.insert(nick.clone(), status);

            // Remove from invite list if present (invite consumed).
            channel.invite_list.remove(&nick);
        }

        // Build the JOIN message with user prefix.
        let join_msg = Message::builder(Command::Join)
            .prefix(Prefix::User {
                nick: nick.clone(),
                user: username.clone(),
                host: hostname.clone(),
            })
            .param(chan_name.as_ref())
            .build();

        // Broadcast JOIN to all channel members (including the joining user).
        broadcast_to_channel(&channel_arc, &join_msg, None, registry);

        // Send topic to joining user.
        {
            let channel = channel_arc.read().expect("channel lock poisoned");
            match &channel.topic {
                Some((text, who, timestamp)) => {
                    send_numeric(
                        sender,
                        RPL_TOPIC,
                        &[&nick_str, chan_name.as_ref()],
                        text,
                    );
                    send_numeric(
                        sender,
                        RPL_TOPICWHOTIME,
                        &[&nick_str, chan_name.as_ref(), who, &timestamp.to_string()],
                        "",
                    );
                }
                None => {
                    send_numeric(
                        sender,
                        RPL_NOTOPIC,
                        &[&nick_str, chan_name.as_ref()],
                        "No topic is set",
                    );
                }
            }
        }

        // Send NAMES list.
        send_names_reply(sender, &nick_str, &chan_name, &channel_arc);
    }
}

/// Handle the PART command from a registered user.
///
/// Supports comma-separated channel names: `PART #chan1,#chan2 [:reason]`
/// Broadcasts PART to channel members, removes user, and cleans up empty channels.
fn handle_part(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
) {
    let Some(session_arc) = registry.get_by_connection(connection_id) else {
        return;
    };

    let (nick, username, hostname) = {
        let session = session_arc.read().expect("session lock poisoned");
        (
            session.nickname.clone(),
            session.username.clone(),
            session.hostname.clone(),
        )
    };
    let nick_str = nick.to_string();

    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[&nick_str, "PART"],
            "Not enough parameters",
        );
        return;
    }

    let channel_names: Vec<&str> = msg.params[0].split(',').collect();
    let reason = msg.params.get(1).map(String::as_str);

    for chan_str in channel_names {
        let chan_str = chan_str.trim();
        if chan_str.is_empty() {
            continue;
        }

        // Validate channel name.
        let Ok(chan_name) = ChannelName::new(chan_str) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        let Some(channel_arc) = channels.get(&chan_name) else {
            send_numeric(
                sender,
                ERR_NOSUCHCHANNEL,
                &[&nick_str, chan_str],
                "No such channel",
            );
            continue;
        };

        // Check membership and remove.
        {
            let channel = channel_arc.read().expect("channel lock poisoned");
            if !channel.members.contains_key(&nick) {
                send_numeric(
                    sender,
                    ERR_NOTONCHANNEL,
                    &[&nick_str, chan_str],
                    "You're not on that channel",
                );
                continue;
            }
        }

        // Build the PART message with user prefix.
        let mut part_builder = Message::builder(Command::Part)
            .prefix(Prefix::User {
                nick: nick.clone(),
                user: username.clone(),
                host: hostname.clone(),
            })
            .param(chan_name.as_ref());
        if let Some(reason) = reason {
            part_builder = part_builder.trailing(reason);
        }
        let part_msg = part_builder.build();

        // Broadcast PART to all channel members (including the parting user).
        broadcast_to_channel(&channel_arc, &part_msg, None, registry);

        // Remove user from channel.
        {
            let mut channel = channel_arc.write().expect("channel lock poisoned");
            channel.members.remove(&nick);
        }

        // Clean up empty channel.
        channels.remove_if_empty(&chan_name);
    }
}

/// Broadcast a message to all members of a channel.
///
/// If `exclude` is `Some(nick)`, that nick will not receive the message.
fn broadcast_to_channel(
    channel_arc: &Arc<std::sync::RwLock<crate::channel::Channel>>,
    msg: &Message,
    exclude: Option<&Nickname>,
    registry: &Arc<UserRegistry>,
) {
    let member_nicks: Vec<Nickname> = {
        let channel = channel_arc.read().expect("channel lock poisoned");
        channel.members.keys().cloned().collect()
    };

    for member_nick in &member_nicks {
        if exclude.is_some_and(|e| e == member_nick) {
            continue;
        }
        if let Some(session_arc) = registry.get_by_nick(member_nick) {
            let session = session_arc.read().expect("session lock poisoned");
            let _ = session.sender.send(msg.clone());
        }
    }
}

/// Send RPL_NAMREPLY + RPL_ENDOFNAMES to a user for a channel.
fn send_names_reply(
    sender: &mpsc::UnboundedSender<Message>,
    nick: &str,
    chan_name: &ChannelName,
    channel_arc: &Arc<std::sync::RwLock<crate::channel::Channel>>,
) {
    let names_str = {
        let channel = channel_arc.read().expect("channel lock poisoned");
        let mut names: Vec<String> = channel
            .members
            .iter()
            .map(|(member_nick, status)| {
                match status.prefix_char() {
                    Some(prefix) => format!("{}{}", prefix, member_nick.as_ref()),
                    None => member_nick.to_string(),
                }
            })
            .collect();
        names.sort();
        names.join(" ")
    };

    // RPL_NAMREPLY: = means public channel
    send_numeric(
        sender,
        RPL_NAMREPLY,
        &[nick, "=", chan_name.as_ref()],
        &names_str,
    );

    send_numeric(
        sender,
        RPL_ENDOFNAMES,
        &[nick, chan_name.as_ref()],
        "End of /NAMES list",
    );
}

/// Check if a user mask matches any ban entry.
fn is_banned(ban_list: &[crate::channel::BanEntry], user_mask: &str) -> bool {
    ban_list.iter().any(|ban| matches_ban_mask(&ban.mask, user_mask))
}

/// Simple glob-style ban mask matching.
///
/// Supports `*` as a wildcard matching any sequence of characters.
fn matches_ban_mask(mask: &str, target: &str) -> bool {
    let mask_lower = mask.to_ascii_lowercase();
    let target_lower = target.to_ascii_lowercase();
    glob_match(&mask_lower, &target_lower)
}

/// Simple glob matching: `*` matches any sequence, `?` matches any single char.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut px = 0;
    let mut tx = 0;
    let mut star_px = usize::MAX;
    let mut star_tx = 0;
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();

    while tx < tb.len() {
        if px < pb.len() && (pb[px] == b'?' || pb[px] == tb[tx]) {
            px += 1;
            tx += 1;
        } else if px < pb.len() && pb[px] == b'*' {
            star_px = px;
            star_tx = tx;
            px += 1;
        } else if star_px != usize::MAX {
            px = star_px + 1;
            star_tx += 1;
            tx = star_tx;
        } else {
            return false;
        }
    }
    while px < pb.len() && pb[px] == b'*' {
        px += 1;
    }
    px == pb.len()
}

/// Remove a user from all channels they are in, cleaning up empty channels.
fn remove_user_from_all_channels(nick: &Nickname, channels: &Arc<ChannelRegistry>) {
    let channel_list = channels.list();
    for (chan_name, _, _) in channel_list {
        if let Some(channel_arc) = channels.get(&chan_name) {
            let mut channel = channel_arc.write().expect("channel lock poisoned");
            channel.members.remove(nick);
        }
        channels.remove_if_empty(&chan_name);
    }
}

fn send_numeric(
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
#[path = "quit_ping_tests.rs"]
mod quit_ping_tests;

#[cfg(test)]
#[path = "join_part_tests.rs"]
mod join_part_tests;
