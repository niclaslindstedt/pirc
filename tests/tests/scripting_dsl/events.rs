//! Event hook integration tests.
//!
//! Covers: JOIN, TEXT, QUIT, NICK, CONNECT, PART, DISCONNECT events,
//! multiple handlers on the same event, glob pattern matching, and
//! event context propagation.

use pirc_scripting::ast::EventType;

use super::{
    connect_event, disconnect_event, engine_with_script, join_event, nick_event, part_event,
    quit_event, text_event, MockScriptHost,
};

// ── JOIN event ──────────────────────────────────────────────────────

#[test]
fn on_join_fires_for_matching_pattern() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:*lobby* {
    msg $chan "Welcome to the lobby, $nick!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = join_event("alice", "#lobby");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[0], "#lobby");
    assert_eq!(cmds[0].1[1], "Welcome to the lobby, alice!");
}

#[test]
fn on_join_wildcard_fires_for_any_channel() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:* {
    echo "$nick joined $chan"
}
"#,
    );

    let mut host = MockScriptHost::new();

    // First channel
    let ctx1 = join_event("alice", "#general");
    engine.dispatch_event(EventType::Join, &ctx1, &mut host);

    // Second channel
    let ctx2 = join_event("bob", "#dev");
    engine.dispatch_event(EventType::Join, &ctx2, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 2);
    assert_eq!(echoed[0], "alice joined #general");
    assert_eq!(echoed[1], "bob joined #dev");
}

#[test]
fn on_join_does_not_fire_for_text_event() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:* {
    echo "join handler"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#test", "hello");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    assert!(host.echoed().is_empty(), "JOIN handler should not fire on TEXT");
}

// ── TEXT event ──────────────────────────────────────────────────────

#[test]
fn on_text_fires_with_pattern_match() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*hello* {
    msg $chan "Hi there, $nick!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#chat", "hello everyone");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].1[1], "Hi there, alice!");
}

#[test]
fn on_text_does_not_fire_when_pattern_mismatches() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*hello* {
    echo "matched"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#chat", "goodbye everyone");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    assert!(host.echoed().is_empty(), "should not match 'goodbye'");
}

// ── QUIT event ──────────────────────────────────────────────────────

#[test]
fn on_quit_fires_when_user_disconnects() {
    let engine = &mut engine_with_script(
        r#"
on QUIT:* {
    echo "$nick has quit"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = quit_event("alice", "Connection reset");
    engine.dispatch_event(EventType::Quit, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "alice has quit");
}

// ── NICK event ──────────────────────────────────────────────────────

#[test]
fn on_nick_fires_on_nick_change() {
    let engine = &mut engine_with_script(
        r#"
on NICK:* {
    echo "$nick changed nick"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = nick_event("oldnick", "newnick");
    engine.dispatch_event(EventType::Nick, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "oldnick changed nick");
}

// ── CONNECT event ───────────────────────────────────────────────────

#[test]
fn on_connect_fires_on_server_connection() {
    let engine = &mut engine_with_script(
        r#"
on CONNECT:* {
    echo "Connected to $server"
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.server = Some("irc.libera.chat".to_string());
    let ctx = connect_event("irc.libera.chat");
    engine.dispatch_event(EventType::Connect, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "Connected to irc.libera.chat");
}

// ── PART event ──────────────────────────────────────────────────────

#[test]
fn on_part_fires_when_user_leaves() {
    let engine = &mut engine_with_script(
        r#"
on PART:* {
    msg $chan "Goodbye $nick!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = part_event("alice", "#test", "leaving now");
    engine.dispatch_event(EventType::Part, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[1], "Goodbye alice!");
}

// ── DISCONNECT event ────────────────────────────────────────────────

#[test]
fn on_disconnect_fires_on_server_disconnect() {
    let engine = &mut engine_with_script(
        r#"
on DISCONNECT:* {
    echo "Disconnected from $server"
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.server = Some("irc.example.com".to_string());
    let ctx = disconnect_event("irc.example.com");
    engine.dispatch_event(EventType::Disconnect, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "Disconnected from irc.example.com");
}

// ── Multiple handlers on same event ─────────────────────────────────

#[test]
fn multiple_handlers_all_fire_for_same_event() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:* {
    echo "handler 1"
}

on JOIN:* {
    echo "handler 2"
}

on JOIN:* {
    echo "handler 3"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = join_event("alice", "#test");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 3, "all three handlers should fire");
    assert_eq!(echoed[0], "handler 1");
    assert_eq!(echoed[1], "handler 2");
    assert_eq!(echoed[2], "handler 3");
}

#[test]
fn handlers_with_different_patterns_fire_selectively() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*hello* {
    echo "greeting detected"
}

on TEXT:*bye* {
    echo "farewell detected"
}

on TEXT:* {
    echo "catch-all"
}
"#,
    );

    // "hello" matches first and catch-all
    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#chat", "hello world");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);
    assert_eq!(host.echoed(), vec!["greeting detected", "catch-all"]);

    // "bye" matches second and catch-all
    let mut host2 = MockScriptHost::new();
    let ctx2 = text_event("bob", "#chat", "bye everyone");
    engine.dispatch_event(EventType::Text, &ctx2, &mut host2);
    assert_eq!(host2.echoed(), vec!["farewell detected", "catch-all"]);

    // "hello and bye" matches all three
    let mut host3 = MockScriptHost::new();
    let ctx3 = text_event("carol", "#chat", "hello and bye");
    engine.dispatch_event(EventType::Text, &ctx3, &mut host3);
    assert_eq!(
        host3.echoed(),
        vec!["greeting detected", "farewell detected", "catch-all"]
    );
}

// ── Event context builtin identifiers ───────────────────────────────

#[test]
fn event_context_provides_nick_chan_text() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:* {
    echo "nick=$nick chan=$chan"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#general", "test message");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "nick=alice chan=#general");
}

// ── Event parameter access ($0-$9) ─────────────────────────────────

#[test]
fn event_text_tokens_accessible_via_dollar_params() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:* {
    echo $1
    echo $2
    echo $3
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#test", "one two three");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "one");
    assert_eq!(echoed[1], "two");
    assert_eq!(echoed[2], "three");
}

// ── Glob pattern matching for channels ──────────────────────────────

#[test]
fn event_pattern_with_wildcard_prefix_glob() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:*dev* {
    echo "matched dev channel"
}
"#,
    );

    let mut host = MockScriptHost::new();
    // Should match any channel containing "dev"
    let ctx = join_event("alice", "#dev-1");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);
    assert_eq!(host.echoed().len(), 1);

    // Should not match channel without "dev"
    let mut host2 = MockScriptHost::new();
    let ctx2 = join_event("bob", "#general");
    engine.dispatch_event(EventType::Join, &ctx2, &mut host2);
    assert!(host2.echoed().is_empty(), "general should not match *dev*");
}

// ── Events from multiple loaded scripts ─────────────────────────────

#[test]
fn events_from_multiple_scripts_all_fire() {
    let now = std::time::Instant::now();
    let mut engine = pirc_scripting::engine::ScriptEngine::new();

    let src1 = r#"
on JOIN:* {
    echo "script1: join"
}
"#;
    let src2 = r#"
on JOIN:* {
    echo "script2: join"
}
"#;
    engine.load_script(src1, "s1.pirc", now).unwrap();
    engine.load_script(src2, "s2.pirc", now).unwrap();

    let mut host = MockScriptHost::new();
    let ctx = super::join_event("alice", "#test");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 2, "both scripts' handlers should fire");
    assert_eq!(echoed[0], "script1: join");
    assert_eq!(echoed[1], "script2: join");
}
