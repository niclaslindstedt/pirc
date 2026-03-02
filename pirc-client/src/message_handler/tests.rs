use super::*;
use pirc_protocol::Prefix;

const TS: &str = "12:00";

fn user_prefix(nick: &str) -> Prefix {
    Prefix::user(nick, nick, "host.com")
}

fn server_prefix() -> Prefix {
    Prefix::server("irc.test.net")
}

/// Collect all PushLine actions into (BufferId, BufferLine) tuples.
fn collect_lines(actions: &[HandlerAction]) -> Vec<(&BufferId, &BufferLine)> {
    actions
        .iter()
        .filter_map(|a| match a {
            HandlerAction::PushLine { target, line } => Some((target, line)),
            _ => None,
        })
        .collect()
}

/// Check if any action is OpenChannel with the given name.
fn has_open_channel(actions: &[HandlerAction], name: &str) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, HandlerAction::OpenChannel(c) if c == name))
}

/// Check if any action is UpdateNick with the given nick.
fn has_update_nick(actions: &[HandlerAction], nick: &str) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, HandlerAction::UpdateNick(n) if n == nick))
}

// ── PRIVMSG to channel ──────────────────────────────────────────

#[test]
fn privmsg_to_channel() {
    let msg = Message::with_prefix(
        user_prefix("alice"),
        Command::Privmsg,
        vec!["#general".into(), "hello world".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    assert!(has_open_channel(&actions, "#general"));

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#general".into()));
    assert_eq!(lines[0].1.sender, Some("alice".into()));
    assert_eq!(lines[0].1.content, "hello world");
    assert_eq!(lines[0].1.line_type, LineType::Message);
}

#[test]
fn privmsg_to_channel_ampersand() {
    let msg = Message::with_prefix(
        user_prefix("alice"),
        Command::Privmsg,
        vec!["&local".into(), "test".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    assert!(has_open_channel(&actions, "&local"));
    let lines = collect_lines(&actions);
    assert_eq!(*lines[0].0, BufferId::Channel("&local".into()));
}

// ── PRIVMSG to our nick (query) ─────────────────────────────────

#[test]
fn privmsg_query_to_us() {
    let msg = Message::with_prefix(
        user_prefix("bob"),
        Command::Privmsg,
        vec!["mynick".into(), "hey there".into()],
    );
    let actions = route_message(&msg, "mynick", TS);

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Query("bob".into()));
    assert_eq!(lines[0].1.sender, Some("bob".into()));
    assert_eq!(lines[0].1.content, "hey there");
    assert_eq!(lines[0].1.line_type, LineType::Message);
}

#[test]
fn privmsg_query_case_insensitive() {
    let msg = Message::with_prefix(
        user_prefix("bob"),
        Command::Privmsg,
        vec!["MyNick".into(), "hi".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Query("bob".into()));
}

#[test]
fn privmsg_missing_params() {
    let msg = Message::new(Command::Privmsg, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── NOTICE ──────────────────────────────────────────────────────

#[test]
fn notice_to_channel() {
    let msg = Message::with_prefix(
        user_prefix("chanserv"),
        Command::Notice,
        vec!["#ops".into(), "Channel registered".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    assert!(has_open_channel(&actions, "#ops"));

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#ops".into()));
    assert_eq!(lines[0].1.line_type, LineType::Notice);
}

#[test]
fn notice_to_star_goes_to_status() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Notice,
        vec!["*".into(), "Looking up your hostname...".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "Looking up your hostname...");
    assert_eq!(lines[0].1.line_type, LineType::Notice);
}

#[test]
fn notice_to_auth_goes_to_status() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Notice,
        vec!["AUTH".into(), "*** Checking ident".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
}

#[test]
fn notice_private_goes_to_status() {
    let msg = Message::with_prefix(
        user_prefix("nickserv"),
        Command::Notice,
        vec!["mynick".into(), "You are now identified".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "-nickserv- You are now identified");
    assert_eq!(lines[0].1.line_type, LineType::Notice);
}

// ── JOIN ────────────────────────────────────────────────────────

#[test]
fn join_creates_buffer_and_pushes_line() {
    let msg = Message::with_prefix(user_prefix("alice"), Command::Join, vec!["#rust".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(has_open_channel(&actions, "#rust"));

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#rust".into()));
    assert_eq!(lines[0].1.content, "alice has joined #rust");
    assert_eq!(lines[0].1.line_type, LineType::Join);
}

#[test]
fn join_missing_channel() {
    let msg = Message::new(Command::Join, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

#[test]
fn join_self_emits_switch_to_channel() {
    let msg = Message::with_prefix(user_prefix("mynick"), Command::Join, vec!["#rust".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions
        .iter()
        .any(|a| matches!(a, HandlerAction::SwitchToChannel(c) if c == "#rust")));
}

#[test]
fn join_other_user_does_not_switch() {
    let msg = Message::with_prefix(user_prefix("alice"), Command::Join, vec!["#rust".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(!actions
        .iter()
        .any(|a| matches!(a, HandlerAction::SwitchToChannel(_))));
}

// ── PART ────────────────────────────────────────────────────────

#[test]
fn part_with_reason() {
    let msg = Message::with_prefix(
        user_prefix("bob"),
        Command::Part,
        vec!["#general".into(), "Goodbye!".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#general".into()));
    assert_eq!(lines[0].1.content, "bob has left #general (Goodbye!)");
    assert_eq!(lines[0].1.line_type, LineType::Part);
}

#[test]
fn part_without_reason() {
    let msg = Message::with_prefix(user_prefix("bob"), Command::Part, vec!["#general".into()]);
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1.content, "bob has left #general");
}

#[test]
fn part_missing_channel() {
    let msg = Message::new(Command::Part, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── QUIT ────────────────────────────────────────────────────────

#[test]
fn quit_with_reason() {
    let msg = Message::with_prefix(
        user_prefix("charlie"),
        Command::Quit,
        vec!["Leaving".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "charlie has quit (Leaving)");
    assert_eq!(lines[0].1.line_type, LineType::Quit);
}

#[test]
fn quit_without_reason() {
    let msg = Message::with_prefix(user_prefix("charlie"), Command::Quit, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1.content, "charlie has quit");
}

// ── KICK ────────────────────────────────────────────────────────

#[test]
fn kick_other_user() {
    let msg = Message::with_prefix(
        user_prefix("ops"),
        Command::Kick,
        vec!["#general".into(), "baduser".into(), "Spam".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#general".into()));
    assert_eq!(
        lines[0].1.content,
        "baduser has been kicked from #general by ops (Spam)"
    );
    assert_eq!(lines[0].1.line_type, LineType::Kick);
}

#[test]
fn kick_us() {
    let msg = Message::with_prefix(
        user_prefix("ops"),
        Command::Kick,
        vec!["#general".into(), "mynick".into(), "Behave".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].1.content,
        "You have been kicked from #general by ops (Behave)"
    );
}

#[test]
fn kick_us_no_reason() {
    let msg = Message::with_prefix(
        user_prefix("ops"),
        Command::Kick,
        vec!["#general".into(), "mynick".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].1.content,
        "You have been kicked from #general by ops"
    );
}

#[test]
fn kick_missing_params() {
    let msg = Message::new(Command::Kick, vec!["#general".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── NICK ────────────────────────────────────────────────────────

#[test]
fn nick_change_our_nick() {
    let msg = Message::with_prefix(user_prefix("mynick"), Command::Nick, vec!["newnick".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(has_update_nick(&actions, "newnick"));

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "You are now known as newnick");
}

#[test]
fn nick_change_other_user() {
    let msg = Message::with_prefix(
        user_prefix("alice"),
        Command::Nick,
        vec!["alice_away".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    assert!(!has_update_nick(&actions, "alice_away"));

    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "alice is now known as alice_away");
}

#[test]
fn nick_missing_new_nick() {
    let msg = Message::with_prefix(user_prefix("alice"), Command::Nick, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── TOPIC ───────────────────────────────────────────────────────

#[test]
fn topic_change() {
    let msg = Message::with_prefix(
        user_prefix("alice"),
        Command::Topic,
        vec!["#rust".into(), "Rust programming language".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#rust".into()));
    assert_eq!(
        lines[0].1.content,
        "alice has changed the topic to: Rust programming language"
    );
    assert_eq!(lines[0].1.line_type, LineType::Topic);
}

#[test]
fn topic_cleared() {
    let msg = Message::with_prefix(
        user_prefix("alice"),
        Command::Topic,
        vec!["#rust".into(), "".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1.content, "alice has cleared the topic for #rust");
}

#[test]
fn topic_missing_channel() {
    let msg = Message::new(Command::Topic, vec![]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── ERROR ───────────────────────────────────────────────────────

#[test]
fn error_message() {
    let msg = Message::new(
        Command::Error,
        vec!["Closing Link: too many connections".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(
        lines[0].1.content,
        "ERROR: Closing Link: too many connections"
    );
    assert_eq!(lines[0].1.line_type, LineType::Error);
}

// ── RPL_TOPIC (332) ─────────────────────────────────────────────

#[test]
fn rpl_topic() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(332),
        vec!["mynick".into(), "#rust".into(), "Welcome to #rust!".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#rust".into()));
    assert_eq!(lines[0].1.content, "Topic: Welcome to #rust!");
    assert_eq!(lines[0].1.line_type, LineType::Topic);
}

// ── RPL_NOTOPIC (331) ───────────────────────────────────────────

#[test]
fn rpl_notopic() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(331),
        vec!["mynick".into(), "#rust".into(), "No topic is set".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#rust".into()));
    assert_eq!(lines[0].1.content, "No topic is set");
}

// ── RPL_NAMREPLY (353) ──────────────────────────────────────────

#[test]
fn rpl_namreply() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(353),
        vec![
            "mynick".into(),
            "=".into(),
            "#rust".into(),
            "@alice bob +charlie".into(),
        ],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#rust".into()));
    assert_eq!(lines[0].1.content, "Users: @alice bob +charlie");
}

// ── RPL_ENDOFNAMES (366) ────────────────────────────────────────

#[test]
fn rpl_endofnames_silent() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(366),
        vec!["mynick".into(), "#rust".into(), "End of /NAMES list".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

// ── Error numerics ──────────────────────────────────────────────

#[test]
fn err_nosuchnick() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(401),
        vec![
            "mynick".into(),
            "badnick".into(),
            "No such nick/channel".into(),
        ],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "badnick: No such nick/channel");
    assert_eq!(lines[0].1.line_type, LineType::Error);
}

#[test]
fn err_nosuchchannel() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(403),
        vec!["mynick".into(), "#badchan".into(), "No such channel".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "#badchan: No such channel");
}

#[test]
fn err_cannotsendtochan() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(404),
        vec![
            "mynick".into(),
            "#moderated".into(),
            "Cannot send to channel".into(),
        ],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Channel("#moderated".into()));
    assert_eq!(lines[0].1.content, "Cannot send to channel");
    assert_eq!(lines[0].1.line_type, LineType::Error);
}

#[test]
fn err_notonchannel() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(442),
        vec![
            "mynick".into(),
            "#secret".into(),
            "You're not on that channel".into(),
        ],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "#secret: You're not on that channel");
}

#[test]
fn other_error_numeric() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(461),
        vec![
            "mynick".into(),
            "JOIN".into(),
            "Not enough parameters".into(),
        ],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.line_type, LineType::Error);
}

// ── Informational numerics ──────────────────────────────────────

#[test]
fn informational_numeric_to_status() {
    let msg = Message::with_prefix(
        server_prefix(),
        Command::Numeric(251),
        vec!["mynick".into(), "There are 42 users online".into()],
    );
    let actions = route_message(&msg, "mynick", TS);
    let lines = collect_lines(&actions);
    assert_eq!(lines.len(), 1);
    assert_eq!(*lines[0].0, BufferId::Status);
    assert_eq!(lines[0].1.content, "There are 42 users online");
    assert_eq!(lines[0].1.line_type, LineType::System);
}

// ── Unhandled commands ──────────────────────────────────────────

#[test]
fn ping_not_routed() {
    let msg = Message::new(Command::Ping, vec!["server".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}

#[test]
fn pong_not_routed() {
    let msg = Message::new(Command::Pong, vec!["server".into()]);
    let actions = route_message(&msg, "mynick", TS);
    assert!(actions.is_empty());
}
