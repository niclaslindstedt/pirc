//! Script-host interaction integration tests.
//!
//! Covers: scripts sending commands via the host, scripts using echo for
//! local display, reading host state (nick, server, channel, port),
//! scripts modifying behavior based on events, error reporting through
//! the host, and combined multi-feature scenarios.

use std::time::{Duration, Instant};

use pirc_scripting::ast::EventType;
use pirc_scripting::engine::ScriptEngine;

use super::{
    connect_event, engine_with_script, join_event, text_event, MockScriptHost,
};

// ── Script sends command via host ───────────────────────────────────

#[test]
fn script_sends_msg_command_to_channel() {
    let engine = &mut engine_with_script(
        r#"
alias announce {
    msg $chan "Important announcement: " $1
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("announce", "Downtime", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert!(cmds[0].1.len() >= 2);
}

#[test]
fn script_sends_multiple_commands() {
    let engine = &mut engine_with_script(
        r##"
alias multi_cmd {
    msg $chan "message 1"
    msg $chan "message 2"
    join "#other"
}
"##,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("multi_cmd", "", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 3);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[1].0, "msg");
    assert_eq!(cmds[2].0, "join");
}

// ── Script uses echo for local display ──────────────────────────────

#[test]
fn echo_outputs_to_host_echo() {
    let engine = &mut engine_with_script(
        r#"
alias info {
    echo "Line 1"
    echo "Line 2"
    echo "Line 3"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("info", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["Line 1", "Line 2", "Line 3"]);
    // echo should not generate commands
    assert!(host.commands().is_empty());
}

// ── Script reads host state ─────────────────────────────────────────

#[test]
fn script_reads_current_nick_from_host() {
    let engine = &mut engine_with_script(
        r#"
alias show_nick {
    echo $me
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.nick = "coolbot".to_string();
    engine.execute_alias("show_nick", "", &mut host);
    assert_eq!(host.echoed()[0], "coolbot");
}

#[test]
fn script_reads_server_from_host() {
    let engine = &mut engine_with_script(
        r#"
alias show_server {
    echo $server
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.server = Some("irc.freenode.net".to_string());
    engine.execute_alias("show_server", "", &mut host);
    assert_eq!(host.echoed()[0], "irc.freenode.net");
}

#[test]
fn script_reads_channel_from_host() {
    let engine = &mut engine_with_script(
        r#"
alias show_channel {
    echo $chan
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.channel = Some("#mychannel".to_string());
    engine.execute_alias("show_channel", "", &mut host);
    assert_eq!(host.echoed()[0], "#mychannel");
}

#[test]
fn script_reads_port_from_host() {
    let engine = &mut engine_with_script(
        r#"
alias show_port {
    echo $port
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.port = 6697;
    engine.execute_alias("show_port", "", &mut host);
    assert_eq!(host.echoed()[0], "6697");
}

// ── Script modifies behavior based on events ────────────────────────

#[test]
fn script_responds_differently_based_on_event_content() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*!help* {
    msg $chan "$nick: Available commands: !help, !ping, !time"
}

on TEXT:*!ping* {
    msg $chan "$nick: pong!"
}
"#,
    );

    // Test !help
    let mut host1 = MockScriptHost::new();
    let ctx1 = text_event("alice", "#support", "!help");
    engine.dispatch_event(EventType::Text, &ctx1, &mut host1);
    let cmds = host1.commands();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].1[1].contains("Available commands"));

    // Test !ping
    let mut host2 = MockScriptHost::new();
    let ctx2 = text_event("bob", "#support", "!ping");
    engine.dispatch_event(EventType::Text, &ctx2, &mut host2);
    let cmds = host2.commands();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].1[1].contains("pong"));
}

#[test]
fn script_auto_greet_on_connect() {
    let engine = &mut engine_with_script(
        r##"
on CONNECT:* {
    join "#welcome"
    msg "#welcome" "Bot is online!"
}
"##,
    );

    let mut host = MockScriptHost::new();
    let ctx = connect_event("irc.example.com");
    engine.dispatch_event(EventType::Connect, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].0, "join");
    assert_eq!(cmds[0].1[0], "#welcome");
    assert_eq!(cmds[1].0, "msg");
    assert_eq!(cmds[1].1[0], "#welcome");
    assert_eq!(cmds[1].1[1], "Bot is online!");
}

// ── Error reporting through host ────────────────────────────────────

#[test]
fn runtime_error_reported_to_host() {
    let engine = &mut engine_with_script(
        r#"
alias bad {
    var %x = 1 / 0
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("bad", "", &mut host);

    let errors = host.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("division by zero"),
        "error should mention division by zero: {}",
        errors[0]
    );
}

#[test]
fn error_in_one_handler_does_not_prevent_other_handlers() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:* {
    var %x = 1 / 0
}

alias still_ok {
    echo "engine alive"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#test", "trigger error");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);
    assert_eq!(host.errors().len(), 1);

    // Engine should still work
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("still_ok", "", &mut host2);
    assert_eq!(host2.echoed()[0], "engine alive");
}

// ── Combined multi-feature scenario ─────────────────────────────────

#[test]
fn full_bot_scenario_events_aliases_timers_globals() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();

    // Load a bot script that uses events, aliases, timers, and globals
    let src = r#"
alias init {
    var %%join_count = 0
    var %%msg_count = 0
}

on JOIN:* {
    set %%join_count (%%join_count + 1)
    msg $chan "Welcome" $nick
}

on TEXT:*stats* {
    echo %%join_count
    echo %%msg_count
}

on TEXT:* {
    set %%msg_count (%%msg_count + 1)
}

timer status 60 0 {
    echo %%join_count
    echo %%msg_count
}
"#;
    engine.load_script(src, "bot.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Initialize globals
    engine.execute_alias("init", "", &mut host);

    // Simulate users joining
    let j1 = join_event("alice", "#main");
    engine.dispatch_event(EventType::Join, &j1, &mut host);

    let j2 = join_event("bob", "#main");
    engine.dispatch_event(EventType::Join, &j2, &mut host);

    // Verify welcome messages
    let cmds = host.commands();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[1].0, "msg");

    // Simulate some messages
    let t1 = text_event("alice", "#main", "hello");
    engine.dispatch_event(EventType::Text, &t1, &mut host);

    let t2 = text_event("bob", "#main", "hi there");
    engine.dispatch_event(EventType::Text, &t2, &mut host);

    // Request stats
    let mut host2 = MockScriptHost::new();
    let stats = text_event("alice", "#main", "show stats please");
    engine.dispatch_event(EventType::Text, &stats, &mut host2);

    // Stats handler fires, then catch-all fires
    let echoed = host2.echoed();
    // stats handler echoes %%join_count and %%msg_count
    assert!(echoed.len() >= 2, "stats handler should echo join_count and msg_count");
    assert_eq!(echoed[0], "2", "should have 2 joins");

    // Tick timer
    let mut host3 = MockScriptHost::new();
    engine.tick_timers(now + Duration::from_secs(60), &mut host3);
    let timer_echoed = host3.echoed();
    assert_eq!(timer_echoed.len(), 2);
    assert_eq!(timer_echoed[0], "2", "timer should see 2 joins");
}

// ── Script listing ──────────────────────────────────────────────────

#[test]
fn list_scripts_returns_loaded_filenames() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    engine
        .load_script(r#"alias a { echo "a" }"#, "first.pirc", now)
        .unwrap();
    engine
        .load_script(r#"alias b { echo "b" }"#, "second.pirc", now)
        .unwrap();

    let scripts = engine.list_scripts();
    assert_eq!(scripts.len(), 2);
    assert!(scripts.contains(&"first.pirc".to_string()));
    assert!(scripts.contains(&"second.pirc".to_string()));
}

// ── execute_command dispatches to alias ──────────────────────────────

#[test]
fn execute_command_dispatches_to_alias() {
    let engine = &mut engine_with_script(
        r#"
alias greet {
    echo "Hi" $1
}
"#,
    );

    let mut host = MockScriptHost::new();
    let handled = engine.execute_command("greet Alice", &mut host);
    assert!(handled, "execute_command should find the alias");
    assert_eq!(host.echoed()[0], "Hi Alice");
}

#[test]
fn execute_command_returns_false_for_unknown() {
    let engine = &mut engine_with_script(
        r#"
alias known { echo "known" }
"#,
    );

    let mut host = MockScriptHost::new();
    let handled = engine.execute_command("unknown_cmd args", &mut host);
    assert!(!handled, "unknown command should return false");
}

// ── Builtin functions accessible from scripts ───────────────────────

#[test]
fn builtin_functions_work_in_script_context() {
    let engine = &mut engine_with_script(
        r#"
alias test_builtins {
    var %text = "Hello World"
    echo $len(%text)
    echo $upper(%text)
    echo $lower(%text)
    echo $left(%text, 5)
    echo $right(%text, 5)
    echo $replace(%text, "World", "IRC")
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("test_builtins", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "11");
    assert_eq!(echoed[1], "HELLO WORLD");
    assert_eq!(echoed[2], "hello world");
    assert_eq!(echoed[3], "Hello");
    assert_eq!(echoed[4], "World");
    assert_eq!(echoed[5], "Hello IRC");
}

// ── Regex functions accessible from scripts ─────────────────────────

#[test]
fn regex_functions_work_in_script_context() {
    let engine = &mut engine_with_script(
        r#"
alias test_regex {
    var %input = "User123 joined #channel"
    var %matched = $regex(%input, "([A-Za-z]+)([0-9]+)")
    echo %matched
    echo $regml(1)
    echo $regml(2)
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("test_regex", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "1", "$regex returns 1 on match");
    assert_eq!(echoed[1], "User");
    assert_eq!(echoed[2], "123");
}
