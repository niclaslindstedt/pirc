use std::collections::HashSet;
use std::sync::Arc;

use pirc_common::UserMode;
use pirc_common::{Nickname, UserError};
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_ERRONEUSNICKNAME, ERR_NEEDMOREPARAMS, ERR_NICKNAMEINUSE, ERR_NOMOTD,
    ERR_NONICKNAMEGIVEN, ERR_NOSUCHNICK, ERR_UMODEUNKNOWNFLAG, ERR_USERSDONTMATCH, RPL_AWAY,
    RPL_CREATED, RPL_ENDOFWHOIS, RPL_NOWAWAY, RPL_UMODEIS, RPL_UNAWAY, RPL_WELCOME, RPL_WHOISIDLE,
    RPL_WHOISOPERATOR, RPL_WHOISSERVER, RPL_WHOISUSER, RPL_YOURHOST,
};
use pirc_protocol::{Command, Message, PircSubcommand, Prefix};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::raft::cluster_command::ClusterCommand;
use crate::raft::rpc::RaftMessage;
use crate::raft::types::NodeId;

use crate::channel_registry::ChannelRegistry;
use crate::config::ServerConfig;
use crate::group_registry::GroupRegistry;
use crate::handler_channel::{
    handle_ban, handle_channel_mode, handle_invite, handle_join, handle_kick, handle_list,
    handle_names, handle_notice, handle_part, handle_privmsg, handle_topic,
    remove_user_from_all_channels,
};
use crate::handler_cluster::{
    self, ClusterContext,
};
use crate::handler_oper::{handle_die, handle_kill, handle_oper, handle_restart, handle_wallops};
use crate::handler_group;
use crate::handler_p2p::handle_p2p_relay;
use crate::handler_relay::handle_relay;
#[allow(unused_imports)] // Re-exported for test submodules that use `super::*`
pub(crate) use crate::handler_oper::{host_matches_mask, is_oper};
use crate::offline_store::OfflineMessageStore;
use crate::prekey_store::PreKeyBundleStore;
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
    /// An operator requested server shutdown (DIE/RESTART).
    Shutdown,
}

/// Handle a single parsed message from a client connection.
///
/// For pre-registration clients, only NICK, USER, PING, PONG, and QUIT are processed.
/// Once both NICK and USER have been received, the client is registered in the
/// `UserRegistry` and the welcome burst is sent.
///
/// Returns [`HandleResult::Quit`] when the client sends QUIT, signalling the
/// connection loop to stop reading and clean up.
#[allow(clippy::too_many_arguments)]
pub fn handle_message(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
    config: &ServerConfig,
    cluster_ctx: Option<&ClusterContext>,
    prekey_store: &Arc<PreKeyBundleStore>,
    offline_store: &Arc<OfflineMessageStore>,
    group_registry: &Arc<GroupRegistry>,
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
                handle_quit(msg, connection_id, registry, channels, group_registry, sender, state);
                return HandleResult::Quit;
            }
            Command::Nick => handle_nick_change(msg, connection_id, registry, sender),
            Command::User => handle_user(msg, sender, state),
            Command::Whois => handle_whois(msg, connection_id, registry, sender),
            Command::Away => handle_away(msg, connection_id, registry, sender),
            Command::Mode => {
                // Route to channel or user mode handler based on target.
                if msg
                    .params
                    .first()
                    .is_some_and(|t| t.starts_with('#') || t.starts_with('&'))
                {
                    handle_channel_mode(msg, connection_id, registry, channels, sender)
                } else {
                    handle_user_mode(msg, connection_id, registry, sender)
                }
            }
            Command::Join => handle_join(msg, connection_id, registry, channels, sender, config.limits.max_channels_per_user),
            Command::Part => handle_part(msg, connection_id, registry, channels, sender),
            Command::Topic => handle_topic(msg, connection_id, registry, channels, sender),
            Command::Kick => handle_kick(msg, connection_id, registry, channels, sender),
            Command::Invite => handle_invite(msg, connection_id, registry, channels, sender),
            Command::Ban => handle_ban(msg, connection_id, registry, channels, sender),
            Command::Privmsg => handle_privmsg(msg, connection_id, registry, channels, sender),
            Command::Notice => handle_notice(msg, connection_id, registry, channels, sender),
            Command::List => handle_list(msg, connection_id, registry, channels, sender),
            Command::Names => handle_names(msg, connection_id, registry, channels, sender),
            Command::Oper => handle_oper(msg, connection_id, registry, sender, config),
            Command::Kill => handle_kill(msg, connection_id, registry, channels, sender),
            Command::Die => {
                if handle_die(msg, connection_id, registry, sender) {
                    return HandleResult::Shutdown;
                }
            }
            Command::Restart => {
                if handle_restart(msg, connection_id, registry, sender) {
                    return HandleResult::Shutdown;
                }
            }
            Command::Wallops => handle_wallops(msg, connection_id, registry, sender),
            Command::Motd => send_motd(sender, &get_nick(connection_id, registry), config),
            Command::Ping => handle_ping(msg, sender),
            Command::Pirc(PircSubcommand::ClusterRaft) => {
                debug!(conn_id = connection_id, "received PIRC CLUSTER RAFT on client connection");
            }
            Command::Pirc(PircSubcommand::InviteKeyGenerate) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_invite_key_generate(
                        msg, connection_id, registry, sender, ctx,
                    );
                }
            }
            Command::Pirc(PircSubcommand::InviteKeyList) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_invite_key_list(connection_id, registry, sender, ctx);
                }
            }
            Command::Pirc(PircSubcommand::InviteKeyRevoke) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_invite_key_revoke(
                        msg, connection_id, registry, sender, ctx,
                    );
                }
            }
            Command::Pirc(PircSubcommand::ClusterStatus) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_cluster_status(connection_id, registry, sender, ctx);
                }
            }
            Command::Pirc(PircSubcommand::ClusterMembers) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_cluster_members(connection_id, registry, sender, ctx);
                }
            }
            Command::Pirc(PircSubcommand::NetworkInfo) => {
                if let Some(ctx) = cluster_ctx {
                    handler_cluster::handle_network_info(connection_id, registry, sender, ctx);
                }
            }
            Command::Pirc(PircSubcommand::KeyExchange) => {
                // If the target is "*", this is a self-directed bundle upload.
                if msg.params.first().is_some_and(|t| t == "*") {
                    maybe_store_prekey_bundle(msg, connection_id, registry, sender, prekey_store);
                } else {
                    handle_key_exchange(
                        msg,
                        connection_id,
                        registry,
                        sender,
                        prekey_store,
                        offline_store,
                    );
                }
            }
            Command::Pirc(
                ref sub @ (PircSubcommand::Encrypted
                | PircSubcommand::KeyExchangeAck
                | PircSubcommand::KeyExchangeComplete
                | PircSubcommand::Fingerprint),
            ) => {
                handle_relay(sub, msg, connection_id, registry, sender, offline_store);
            }
            Command::Pirc(
                ref sub @ (PircSubcommand::P2pOffer
                | PircSubcommand::P2pAnswer
                | PircSubcommand::P2pIce
                | PircSubcommand::P2pEstablished
                | PircSubcommand::P2pFailed),
            ) => {
                handle_p2p_relay(sub, msg, connection_id, registry, sender);
            }
            Command::Pirc(PircSubcommand::GroupCreate) => {
                handler_group::handle_group_create(
                    msg, connection_id, registry, group_registry, sender,
                );
            }
            Command::Pirc(PircSubcommand::GroupInvite) => {
                handler_group::handle_group_invite(
                    msg, connection_id, registry, group_registry, sender,
                );
            }
            Command::Pirc(PircSubcommand::GroupJoin) => {
                handler_group::handle_group_join(
                    msg, connection_id, registry, group_registry, sender,
                );
            }
            Command::Pirc(PircSubcommand::GroupLeave) => {
                handler_group::handle_group_leave(
                    msg, connection_id, registry, group_registry, sender,
                );
            }
            Command::Pirc(PircSubcommand::GroupMessage) => {
                handler_group::handle_group_message_relay(
                    msg, connection_id, registry, group_registry, sender, offline_store,
                );
            }
            Command::Pirc(
                ref sub @ (PircSubcommand::GroupKeyExchange
                | PircSubcommand::GroupP2pOffer
                | PircSubcommand::GroupP2pAnswer
                | PircSubcommand::GroupP2pIce),
            ) => {
                handler_group::handle_group_signaling_relay(
                    sub, msg, connection_id, registry, group_registry, sender,
                );
            }
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
        complete_registration(connection_id, registry, sender, state, config, offline_store);
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
        &[
            &requestor_nick,
            &nick,
            &target.username,
            &target.hostname,
            "*",
        ],
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
        send_numeric(sender, ERR_UMODEUNKNOWNFLAG, &[&nick], "Unknown MODE flag");
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
    offline_store: &Arc<OfflineMessageStore>,
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

    // Deliver any offline messages queued while this user was disconnected.
    deliver_offline_messages(&nick, sender, offline_store);
}

/// Deliver queued offline messages to a user who just connected.
///
/// Messages are sorted so that key exchange messages come before encrypted
/// messages, ensuring the recipient can establish encryption sessions before
/// attempting decryption.
fn deliver_offline_messages(
    nick: &Nickname,
    sender: &mpsc::UnboundedSender<Message>,
    offline_store: &Arc<OfflineMessageStore>,
) {
    let mut messages = offline_store.take_messages(nick);

    if messages.is_empty() {
        return;
    }

    // Sort: key exchange messages first, then everything else in original order.
    // Use a stable sort so relative ordering within each group is preserved.
    messages.sort_by_key(|msg| match &msg.command {
        Command::Pirc(PircSubcommand::KeyExchange) => 0,
        Command::Pirc(PircSubcommand::KeyExchangeAck) => 1,
        Command::Pirc(PircSubcommand::KeyExchangeComplete) => 2,
        _ => 3,
    });

    let count = messages.len();
    for msg in messages {
        let _ = sender.send(msg);
    }

    debug!(nick = %nick, count, "delivered offline messages");
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
/// Broadcasts QUIT to all channel members, removes the user from all channels,
/// cleans up empty channels, removes the user from the registry, and sends
/// an ERROR closing link message.
fn handle_quit(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    channels: &Arc<ChannelRegistry>,
    group_registry: &Arc<GroupRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    state: &mut PreRegistrationState,
) {
    let quit_message = msg.params.first().map_or("Client Quit", |s| s.as_str());

    let (hostname, nickname, username) =
        if let Some(session_arc) = registry.get_by_connection(connection_id) {
            let session = session_arc.read().expect("session lock poisoned");
            (
                session.hostname.clone(),
                Some(session.nickname.clone()),
                Some(session.username.clone()),
            )
        } else {
            (state.hostname.clone(), None, None)
        };

    // Build the QUIT message with the user's prefix.
    if let (Some(ref nick), Some(ref user)) = (&nickname, &username) {
        let quit_msg = Message::builder(Command::Quit)
            .prefix(Prefix::User {
                nick: nick.clone(),
                user: user.clone(),
                host: hostname.clone(),
            })
            .trailing(quit_message)
            .build();

        // Broadcast QUIT to all channel members (deduplicated) then remove from channels.
        broadcast_quit_and_remove(nick, &quit_msg, channels, registry);

        // Remove user from all groups, broadcasting GROUP LEAVE to remaining members.
        handler_group::remove_user_from_all_groups(nick, user, &hostname, registry, group_registry);
    } else if let Some(ref nick) = nickname {
        // No user info available, just remove silently.
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

/// Broadcast a QUIT message to all unique recipients across all channels the user
/// is in, then remove the user from all channels and clean up empty ones.
pub(crate) fn broadcast_quit_and_remove(
    nick: &Nickname,
    quit_msg: &Message,
    channels: &Arc<ChannelRegistry>,
    registry: &Arc<UserRegistry>,
) {
    use std::collections::HashSet;

    let channel_list = channels.list_all();
    let mut notified: HashSet<Nickname> = HashSet::new();

    for (chan_name, channel_arc) in &channel_list {
        let members: Vec<Nickname> = {
            let channel = channel_arc.read().expect("channel lock poisoned");
            if !channel.members.contains_key(nick) {
                continue;
            }
            channel.members.keys().cloned().collect()
        };

        // Send QUIT to each member we haven't notified yet.
        for member_nick in &members {
            if member_nick == nick {
                continue;
            }
            if notified.insert(member_nick.clone()) {
                if let Some(session_arc) = registry.get_by_nick(member_nick) {
                    let session = session_arc.read().expect("session lock poisoned");
                    let _ = session.sender.send(quit_msg.clone());
                }
            }
        }

        // Remove user from channel.
        {
            let mut channel = channel_arc.write().expect("channel lock poisoned");
            channel.members.remove(nick);
        }
        channels.remove_if_empty(chan_name);
    }
}

/// Get a user's nick by connection ID, returning "*" if not found.
fn get_nick(connection_id: u64, registry: &Arc<UserRegistry>) -> String {
    if let Some(s) = registry.get_by_connection(connection_id) {
        let session = s.read().expect("session lock poisoned");
        session.nickname.to_string()
    } else {
        "*".to_owned()
    }
}

/// Route an inbound PIRC CLUSTER RAFT message to the Raft driver.
///
/// Deserializes the protocol message into a [`RaftMessage`] and forwards it
/// to the Raft driver's inbound channel. The `from` parameter identifies which
/// peer sent this message.
///
/// Returns `true` if the message was successfully forwarded, `false` otherwise.
pub fn handle_cluster_raft(
    msg: &Message,
    from: NodeId,
    inbound_tx: &mpsc::UnboundedSender<(NodeId, RaftMessage<ClusterCommand>)>,
) -> bool {
    match RaftMessage::<ClusterCommand>::from_protocol_message(msg) {
        Ok(raft_msg) => {
            if inbound_tx.send((from, raft_msg)).is_err() {
                warn!(%from, "raft inbound channel closed");
                return false;
            }
            true
        }
        Err(e) => {
            warn!(%from, error = %e, "failed to deserialize raft message");
            false
        }
    }
}

/// Handle a `PIRC KEYEXCHANGE <target> [data]` message.
///
/// The wire format is: `PIRC KEYEXCHANGE <target> <base64-data>`
///
/// The server decodes the base64 data to determine the message type:
/// - **`RequestBundle`**: The sender is requesting the target's pre-key bundle.
///   The server looks it up in the [`PreKeyBundleStore`] and sends it back.
/// - **Other variants** (`Bundle`, `InitMessage`, `Complete`): Relayed directly
///   to the target user. The server does not inspect or decrypt these.
///
/// If no data parameter is present, the command is treated as a `RequestBundle`.
fn handle_key_exchange(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    prekey_store: &Arc<PreKeyBundleStore>,
    offline_store: &Arc<OfflineMessageStore>,
) {
    // Get the sender's nickname.
    let sender_nick = match registry.get_by_connection(connection_id) {
        Some(session_arc) => {
            let session = session_arc.read().expect("session lock poisoned");
            session.nickname.clone()
        }
        None => return,
    };

    // params[0] = target nick, params[1] = base64-encoded key exchange data (optional for RequestBundle)
    if msg.params.is_empty() {
        send_numeric(
            sender,
            ERR_NEEDMOREPARAMS,
            &[sender_nick.as_ref(), "KEYEXCHANGE"],
            "Not enough parameters",
        );
        return;
    }

    let target_str = &msg.params[0];
    let Ok(target_nick) = Nickname::new(target_str) else {
        send_numeric(
            sender,
            ERR_NOSUCHNICK,
            &[sender_nick.as_ref(), target_str],
            "No such nick/channel",
        );
        return;
    };

    // Determine message type: if there's data, decode it; otherwise treat as RequestBundle.
    let is_request_bundle = if msg.params.len() < 2 {
        true
    } else {
        // Try to decode to check the message type tag.
        match pirc_crypto::protocol::decode_from_wire(&msg.params[1]) {
            Ok(bytes) if !bytes.is_empty() && bytes[0] == 0 => true, // TAG_REQUEST_BUNDLE = 0
            _ => false,
        }
    };

    // Maximum base64 chars per chunk when delivering a stored bundle.
    const BUNDLE_CHUNK_SIZE: usize = 400;

    if is_request_bundle {
        // Look up the target's pre-key bundle in the store.
        if let Some(bundle_data) = prekey_store.get_bundle(&target_nick) {
            // Send the bundle back to the requester.
            // If it is too large for a single IRC message, split into chunks.
            let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_data);
            if encoded.len() > BUNDLE_CHUNK_SIZE {
                let raw = encoded.as_bytes();
                let chunks: Vec<&str> = raw
                    .chunks(BUNDLE_CHUNK_SIZE)
                    .map(|c| std::str::from_utf8(c).expect("base64 is valid UTF-8"))
                    .collect();
                let total = chunks.len();
                for (i, chunk) in chunks.iter().enumerate() {
                    let n = i + 1;
                    let reply = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
                        .prefix(Prefix::server(SERVER_NAME))
                        .param(sender_nick.as_ref())
                        .param(&format!("{n}/{total}"))
                        .param(chunk)
                        .build();
                    let _ = sender.send(reply);
                }
            } else {
                let reply = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
                    .prefix(Prefix::server(SERVER_NAME))
                    .param(sender_nick.as_ref())
                    .param(&encoded)
                    .build();
                let _ = sender.send(reply);
            }
        } else {
            // Target has no bundle stored.
            let notice = Message::builder(Command::Notice)
                .prefix(Prefix::server(SERVER_NAME))
                .param(sender_nick.as_ref())
                .trailing(&format!(
                    "No pre-key bundle available for {target_nick}"
                ))
                .build();
            let _ = sender.send(notice);
        }
    } else {
        // Relay the key exchange message to the target user.
        // Forward all params after the target (supports both single-param and
        // chunked n/total + chunk_data forms).
        let sender_session = registry
            .get_by_connection(connection_id)
            .expect("sender session must exist");
        let (username, hostname) = {
            let s = sender_session.read().expect("session lock poisoned");
            (s.username.clone(), s.hostname.clone())
        };
        let mut relay_builder = Message::builder(Command::Pirc(PircSubcommand::KeyExchange))
            .prefix(Prefix::User {
                nick: sender_nick.clone(),
                user: username,
                host: hostname,
            })
            .param(target_nick.as_ref());
        for param in msg.params.iter().skip(1) {
            relay_builder = relay_builder.param(param);
        }
        let relay = relay_builder.build();

        if let Some(session_arc) = registry.get_by_nick(&target_nick) {
            let session = session_arc.read().expect("session lock poisoned");
            let _ = session.sender.send(relay);
        } else {
            // Target is offline — queue the message for delivery on reconnect.
            offline_store.queue_message(&target_nick, relay);
            let notice = Message::builder(Command::Notice)
                .prefix(Prefix::server(SERVER_NAME))
                .param(sender_nick.as_ref())
                .trailing(&format!(
                    "{target_nick} is offline. Message will be delivered when they reconnect."
                ))
                .build();
            let _ = sender.send(notice);
        }
    }
}

/// Parse a chunk header of the form `"<n>/<total>"`.
///
/// Returns `Some((n, total))` where both are 1-based and `n <= total`, or
/// `None` if the string is not a valid header.
fn parse_chunk_header(s: &str) -> Option<(usize, usize)> {
    let (n_str, total_str) = s.split_once('/')?;
    let n = n_str.parse::<usize>().ok()?;
    let total = total_str.parse::<usize>().ok()?;
    if n >= 1 && total >= 1 && n <= total {
        Some((n, total))
    } else {
        None
    }
}

/// Handle `PIRC KEYEXCHANGE` for bundle registration (storing a user's pre-key
/// bundle on the server). Called when a user sends their own bundle to the
/// server for storage.
///
/// Wire formats:
/// - Single message: `PIRC KEYEXCHANGE * <base64-bundle-data>`
/// - Chunked:        `PIRC KEYEXCHANGE * <n>/<total> <base64-chunk>`
///
/// When target is `*` (self), the server stores the bundle data in the
/// [`PreKeyBundleStore`] keyed by the sender's nickname.
pub(crate) fn maybe_store_prekey_bundle(
    msg: &Message,
    connection_id: u64,
    registry: &Arc<UserRegistry>,
    sender: &mpsc::UnboundedSender<Message>,
    prekey_store: &Arc<PreKeyBundleStore>,
) {
    // Check if this is a self-directed bundle upload (target = "*")
    if msg.params.len() < 2 {
        return;
    }

    if msg.params[0] != "*" {
        return;
    }

    let sender_nick = match registry.get_by_connection(connection_id) {
        Some(session_arc) => {
            let session = session_arc.read().expect("session lock poisoned");
            session.nickname.clone()
        }
        None => return,
    };

    // Determine the full base64-encoded payload, handling chunked uploads.
    let encoded: String = if msg.params.len() >= 3 {
        if let Some((n, total)) = parse_chunk_header(&msg.params[1]) {
            // Chunked form: accumulate and wait for completion.
            match prekey_store.add_bundle_chunk(&sender_nick, &msg.params[2], n, total) {
                Some(assembled) => assembled,
                None => return, // more chunks expected
            }
        } else {
            msg.params[1].clone()
        }
    } else {
        msg.params[1].clone()
    };

    // Decode and validate that this is a Bundle message.
    let Ok(data) = pirc_crypto::protocol::decode_from_wire(&encoded) else {
        let notice = Message::builder(Command::Notice)
            .prefix(Prefix::server(SERVER_NAME))
            .param(sender_nick.as_ref())
            .trailing("Invalid key exchange data encoding")
            .build();
        let _ = sender.send(notice);
        return;
    };

    // Verify this is a Bundle message (tag byte 1).
    if data.is_empty() || data[0] != 1 {
        let notice = Message::builder(Command::Notice)
            .prefix(Prefix::server(SERVER_NAME))
            .param(sender_nick.as_ref())
            .trailing("Expected a Bundle message for registration")
            .build();
        let _ = sender.send(notice);
        return;
    }

    prekey_store.store_bundle(&sender_nick, data);

    let ack = Message::builder(Command::Notice)
        .prefix(Prefix::server(SERVER_NAME))
        .param(sender_nick.as_ref())
        .trailing("Pre-key bundle registered")
        .build();
    let _ = sender.send(ack);
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
#[path = "channel_mode_query_tests.rs"]
mod channel_mode_query_tests;

#[cfg(test)]
#[path = "channel_mode_set_tests.rs"]
mod channel_mode_set_tests;

#[cfg(test)]
#[path = "channel_mode_tests.rs"]
mod channel_mode_tests;

#[cfg(test)]
#[path = "ban_invite_tests.rs"]
mod ban_invite_tests;

#[cfg(test)]
#[path = "privmsg_notice_tests.rs"]
mod privmsg_notice_tests;

#[cfg(test)]
#[path = "list_names_tests.rs"]
mod list_names_tests;

#[cfg(test)]
#[path = "oper_tests.rs"]
mod oper_tests;

#[cfg(test)]
#[path = "ctcp_tests.rs"]
mod ctcp_tests;

#[cfg(test)]
#[path = "keyexchange_tests.rs"]
mod keyexchange_tests;

#[cfg(test)]
#[path = "relay_tests.rs"]
mod relay_tests;

#[cfg(test)]
#[path = "p2p_relay_tests.rs"]
mod p2p_relay_tests;

#[cfg(test)]
#[path = "offline_delivery_tests.rs"]
mod offline_delivery_tests;
