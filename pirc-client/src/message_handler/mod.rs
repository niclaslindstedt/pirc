use pirc_protocol::{Command, Message, Prefix};

use crate::tui::buffer_manager::BufferId;
use crate::tui::message_buffer::{BufferLine, LineType};

/// Extract the sender nick from a message prefix.
fn sender_nick(msg: &Message) -> Option<String> {
    match &msg.prefix {
        Some(Prefix::User { nick, .. }) => Some(nick.as_ref().to_string()),
        _ => None,
    }
}

/// An action to be performed by the app after handling a message.
pub enum HandlerAction {
    /// Push a line to a specific buffer.
    PushLine { target: BufferId, line: BufferLine },
    /// Ensure a channel buffer exists (open it) before pushing lines.
    OpenChannel(String),
    /// Update our nick in the connection manager and view.
    UpdateNick(String),
}

/// Route an inbound server message to appropriate buffer actions.
///
/// Returns a list of actions for the caller to execute. This keeps the handler
/// pure and testable — it doesn't directly mutate buffers or connection state.
///
/// `our_nick` is the current nick of the client, used to determine whether a
/// PRIVMSG is directed at us (query) vs. a channel, and whether a NICK change
/// is ours.
pub fn route_message(msg: &Message, our_nick: &str, ts: &str) -> Vec<HandlerAction> {
    match &msg.command {
        Command::Privmsg => route_privmsg(msg, our_nick, ts),
        Command::Notice => route_notice(msg, our_nick, ts),
        Command::Join => route_join(msg, ts),
        Command::Part => route_part(msg, ts),
        Command::Quit => route_quit(msg, ts),
        Command::Kick => route_kick(msg, our_nick, ts),
        Command::Nick => route_nick(msg, our_nick, ts),
        Command::Topic => route_topic(msg, ts),
        Command::Error => route_error(msg, ts),
        Command::Numeric(code) => route_numeric(*code, msg, ts),
        _ => vec![],
    }
}

fn route_privmsg(msg: &Message, our_nick: &str, ts: &str) -> Vec<HandlerAction> {
    let target = match msg.params.first() {
        Some(t) => t,
        None => return vec![],
    };
    let sender = sender_nick(msg).unwrap_or_default();
    let content = msg.params.get(1).map(|s| s.as_str()).unwrap_or("");

    if target.starts_with('#') || target.starts_with('&') {
        // Channel message — ensure buffer exists, then push
        vec![
            HandlerAction::OpenChannel(target.clone()),
            HandlerAction::PushLine {
                target: BufferId::Channel(target.clone()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: Some(sender),
                    content: content.to_string(),
                    line_type: LineType::Message,
                },
            },
        ]
    } else if target.eq_ignore_ascii_case(our_nick) {
        // Private message directed at us — create query buffer for sender
        vec![HandlerAction::PushLine {
            target: BufferId::Query(sender.clone()),
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: Some(sender),
                content: content.to_string(),
                line_type: LineType::Message,
            },
        }]
    } else {
        // PRIVMSG to some other target (shouldn't happen normally)
        vec![]
    }
}

fn route_notice(msg: &Message, _our_nick: &str, ts: &str) -> Vec<HandlerAction> {
    let target = match msg.params.first() {
        Some(t) => t,
        None => return vec![],
    };
    let sender = sender_nick(msg);
    let content = msg.params.get(1).map(|s| s.as_str()).unwrap_or("");

    // Server notices (pre-registration or to *, AUTH, etc.) → status buffer
    if target == "*" || target.eq_ignore_ascii_case("AUTH") {
        return vec![HandlerAction::PushLine {
            target: BufferId::Status,
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: None,
                content: content.to_string(),
                line_type: LineType::Notice,
            },
        }];
    }

    if target.starts_with('#') || target.starts_with('&') {
        // Channel notice
        vec![
            HandlerAction::OpenChannel(target.clone()),
            HandlerAction::PushLine {
                target: BufferId::Channel(target.clone()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender,
                    content: content.to_string(),
                    line_type: LineType::Notice,
                },
            },
        ]
    } else {
        // Private notice — goes to status buffer (standard IRC behavior)
        let display = match &sender {
            Some(nick) => format!("-{nick}- {content}"),
            None => content.to_string(),
        };
        vec![HandlerAction::PushLine {
            target: BufferId::Status,
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: None,
                content: display,
                line_type: LineType::Notice,
            },
        }]
    }
}

fn route_join(msg: &Message, ts: &str) -> Vec<HandlerAction> {
    let channel = match msg.params.first() {
        Some(c) => c,
        None => return vec![],
    };
    let nick = sender_nick(msg).unwrap_or_default();

    vec![
        HandlerAction::OpenChannel(channel.clone()),
        HandlerAction::PushLine {
            target: BufferId::Channel(channel.clone()),
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: None,
                content: format!("{nick} has joined {channel}"),
                line_type: LineType::Join,
            },
        },
    ]
}

fn route_part(msg: &Message, ts: &str) -> Vec<HandlerAction> {
    let channel = match msg.params.first() {
        Some(c) => c,
        None => return vec![],
    };
    let nick = sender_nick(msg).unwrap_or_default();
    let reason = msg.params.get(1).map(|s| s.as_str()).unwrap_or("");

    let content = if reason.is_empty() {
        format!("{nick} has left {channel}")
    } else {
        format!("{nick} has left {channel} ({reason})")
    };

    vec![HandlerAction::PushLine {
        target: BufferId::Channel(channel.clone()),
        line: BufferLine {
            timestamp: ts.to_string(),
            sender: None,
            content,
            line_type: LineType::Part,
        },
    }]
}

fn route_quit(msg: &Message, ts: &str) -> Vec<HandlerAction> {
    let nick = sender_nick(msg).unwrap_or_default();
    let reason = msg.params.first().map(|s| s.as_str()).unwrap_or("");

    let content = if reason.is_empty() {
        format!("{nick} has quit")
    } else {
        format!("{nick} has quit ({reason})")
    };

    // QUIT messages should go to all shared channels, but we don't track
    // channel membership yet. For now, push to status buffer. When channel
    // membership tracking is added, this can be improved.
    vec![HandlerAction::PushLine {
        target: BufferId::Status,
        line: BufferLine {
            timestamp: ts.to_string(),
            sender: None,
            content,
            line_type: LineType::Quit,
        },
    }]
}

fn route_kick(msg: &Message, our_nick: &str, ts: &str) -> Vec<HandlerAction> {
    let channel = match msg.params.first() {
        Some(c) => c,
        None => return vec![],
    };
    let kicked_nick = match msg.params.get(1) {
        Some(n) => n,
        None => return vec![],
    };
    let kicker = sender_nick(msg).unwrap_or_default();
    let reason = msg.params.get(2).map(|s| s.as_str()).unwrap_or("");

    let content = if kicked_nick.eq_ignore_ascii_case(our_nick) {
        if reason.is_empty() {
            format!("You have been kicked from {channel} by {kicker}")
        } else {
            format!("You have been kicked from {channel} by {kicker} ({reason})")
        }
    } else if reason.is_empty() {
        format!("{kicked_nick} has been kicked from {channel} by {kicker}")
    } else {
        format!("{kicked_nick} has been kicked from {channel} by {kicker} ({reason})")
    };

    vec![HandlerAction::PushLine {
        target: BufferId::Channel(channel.clone()),
        line: BufferLine {
            timestamp: ts.to_string(),
            sender: None,
            content,
            line_type: LineType::Kick,
        },
    }]
}

fn route_nick(msg: &Message, our_nick: &str, ts: &str) -> Vec<HandlerAction> {
    let old_nick = sender_nick(msg).unwrap_or_default();
    let new_nick = match msg.params.first() {
        Some(n) => n,
        None => return vec![],
    };

    let mut actions = Vec::new();

    if old_nick.eq_ignore_ascii_case(our_nick) {
        actions.push(HandlerAction::UpdateNick(new_nick.clone()));
        actions.push(HandlerAction::PushLine {
            target: BufferId::Status,
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: None,
                content: format!("You are now known as {new_nick}"),
                line_type: LineType::System,
            },
        });
    } else {
        // Someone else changed their nick — show in status buffer for now.
        // When channel membership tracking is added, push to shared channels.
        actions.push(HandlerAction::PushLine {
            target: BufferId::Status,
            line: BufferLine {
                timestamp: ts.to_string(),
                sender: None,
                content: format!("{old_nick} is now known as {new_nick}"),
                line_type: LineType::System,
            },
        });
    }

    actions
}

fn route_topic(msg: &Message, ts: &str) -> Vec<HandlerAction> {
    let channel = match msg.params.first() {
        Some(c) => c,
        None => return vec![],
    };
    let topic = msg.params.get(1).map(|s| s.as_str()).unwrap_or("");
    let nick = sender_nick(msg).unwrap_or_default();

    let content = if topic.is_empty() {
        format!("{nick} has cleared the topic for {channel}")
    } else {
        format!("{nick} has changed the topic to: {topic}")
    };

    vec![HandlerAction::PushLine {
        target: BufferId::Channel(channel.clone()),
        line: BufferLine {
            timestamp: ts.to_string(),
            sender: None,
            content,
            line_type: LineType::Topic,
        },
    }]
}

fn route_error(msg: &Message, ts: &str) -> Vec<HandlerAction> {
    let content = msg
        .params
        .first()
        .map(|s| s.as_str())
        .unwrap_or("Unknown error");

    vec![HandlerAction::PushLine {
        target: BufferId::Status,
        line: BufferLine {
            timestamp: ts.to_string(),
            sender: None,
            content: format!("ERROR: {content}"),
            line_type: LineType::Error,
        },
    }]
}

fn route_numeric(code: u16, msg: &Message, ts: &str) -> Vec<HandlerAction> {
    match code {
        // RPL_TOPIC (332): <channel> :<topic>
        332 => {
            let channel = match msg.params.get(1) {
                Some(c) => c,
                None => return vec![],
            };
            let topic = msg.params.get(2).map(|s| s.as_str()).unwrap_or("");
            vec![HandlerAction::PushLine {
                target: BufferId::Channel(channel.clone()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: format!("Topic: {topic}"),
                    line_type: LineType::Topic,
                },
            }]
        }

        // RPL_NOTOPIC (331): <channel> :No topic is set
        331 => {
            let channel = match msg.params.get(1) {
                Some(c) => c,
                None => return vec![],
            };
            vec![HandlerAction::PushLine {
                target: BufferId::Channel(channel.clone()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: "No topic is set".to_string(),
                    line_type: LineType::Topic,
                },
            }]
        }

        // RPL_NAMREPLY (353): <nick> <symbol> <channel> :<names>
        353 => {
            let channel = match msg.params.get(2) {
                Some(c) => c,
                None => return vec![],
            };
            let names = msg.params.get(3).map(|s| s.as_str()).unwrap_or("");
            vec![HandlerAction::PushLine {
                target: BufferId::Channel(channel.clone()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: format!("Users: {names}"),
                    line_type: LineType::System,
                },
            }]
        }

        // RPL_ENDOFNAMES (366): <nick> <channel> :End of /NAMES list
        366 => {
            // Silently consume — the names list is already displayed
            vec![]
        }

        // ERR_NOSUCHNICK (401): <nick> <target> :No such nick/channel
        401 => {
            let target = msg.params.get(1).map(|s| s.as_str()).unwrap_or("?");
            let text = msg
                .params
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("No such nick/channel");
            vec![HandlerAction::PushLine {
                target: BufferId::Status,
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: format!("{target}: {text}"),
                    line_type: LineType::Error,
                },
            }]
        }

        // ERR_NOSUCHCHANNEL (403): <nick> <channel> :No such channel
        403 => {
            let channel = msg.params.get(1).map(|s| s.as_str()).unwrap_or("?");
            let text = msg
                .params
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("No such channel");
            vec![HandlerAction::PushLine {
                target: BufferId::Status,
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: format!("{channel}: {text}"),
                    line_type: LineType::Error,
                },
            }]
        }

        // ERR_CANNOTSENDTOCHAN (404): <nick> <channel> :Cannot send to channel
        404 => {
            let channel = msg.params.get(1).map(|s| s.as_str()).unwrap_or("?");
            let text = msg
                .params
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("Cannot send to channel");
            vec![HandlerAction::PushLine {
                target: BufferId::Channel(channel.to_string()),
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: text.to_string(),
                    line_type: LineType::Error,
                },
            }]
        }

        // ERR_NOTONCHANNEL (442): <nick> <channel> :You're not on that channel
        442 => {
            let channel = msg.params.get(1).map(|s| s.as_str()).unwrap_or("?");
            let text = msg
                .params
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("You're not on that channel");
            vec![HandlerAction::PushLine {
                target: BufferId::Status,
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: format!("{channel}: {text}"),
                    line_type: LineType::Error,
                },
            }]
        }

        // Other errors (400-599) → status buffer
        code if code >= 400 => {
            // Format: skip the first param (our nick), join the rest
            let text: String = if msg.params.len() > 1 {
                msg.params[1..].join(" ")
            } else {
                format!("Error {code}")
            };
            vec![HandlerAction::PushLine {
                target: BufferId::Status,
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: text,
                    line_type: LineType::Error,
                },
            }]
        }

        // Other informational numerics → status buffer
        _ => {
            let text: String = if msg.params.len() > 1 {
                msg.params[1..].join(" ")
            } else {
                format!("{code}")
            };
            vec![HandlerAction::PushLine {
                target: BufferId::Status,
                line: BufferLine {
                    timestamp: ts.to_string(),
                    sender: None,
                    content: text,
                    line_type: LineType::System,
                },
            }]
        }
    }
}

#[cfg(test)]
mod tests;
