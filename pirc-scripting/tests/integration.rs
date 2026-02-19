//! End-to-end integration tests for the pirc scripting runtime.
//!
//! Each test exercises the full pipeline: parse → load → execute,
//! using a `MockScriptHost` to capture outputs and verify behavior.

#![allow(clippy::needless_raw_string_hashes)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pirc_scripting::ast::EventType;
use pirc_scripting::engine::ScriptEngine;
use pirc_scripting::interpreter::{
    CommandHandler, EventContext, RuntimeError, ScriptHost, ScriptRuntimeError, Value,
};

// ── Test infrastructure ──────────────────────────────────────────────

/// Mock host that records commands, echo output, errors, and warnings.
#[allow(clippy::type_complexity)]
struct MockScriptHost {
    commands: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    echoed: Arc<Mutex<Vec<String>>>,
    errors: Arc<Mutex<Vec<String>>>,
    warnings: Arc<Mutex<Vec<String>>>,
    nick: String,
    server: Option<String>,
    channel: Option<String>,
    port: u16,
}

impl MockScriptHost {
    fn new() -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
            echoed: Arc::new(Mutex::new(Vec::new())),
            errors: Arc::new(Mutex::new(Vec::new())),
            warnings: Arc::new(Mutex::new(Vec::new())),
            nick: "testbot".to_string(),
            server: Some("irc.example.com".to_string()),
            channel: Some("#test".to_string()),
            port: 6667,
        }
    }

    fn commands(&self) -> Vec<(String, Vec<String>)> {
        self.commands.lock().unwrap().clone()
    }

    fn echoed(&self) -> Vec<String> {
        self.echoed.lock().unwrap().clone()
    }

    fn errors(&self) -> Vec<String> {
        self.errors.lock().unwrap().clone()
    }
}

impl CommandHandler for MockScriptHost {
    fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
        self.commands.lock().unwrap().push((
            name.to_string(),
            args.iter().map(ToString::to_string).collect(),
        ));
        Ok(())
    }
}

impl ScriptHost for MockScriptHost {
    fn current_nick(&self) -> &str {
        &self.nick
    }

    fn current_server(&self) -> Option<&str> {
        self.server.as_deref()
    }

    fn current_channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    fn server_port(&self) -> u16 {
        self.port
    }

    fn echo(&mut self, text: &str) {
        self.echoed.lock().unwrap().push(text.to_string());
    }

    fn report_error(&mut self, error: &ScriptRuntimeError) {
        self.errors.lock().unwrap().push(error.to_string());
    }

    fn report_warning(&mut self, warning: &str) {
        self.warnings.lock().unwrap().push(warning.to_string());
    }
}

/// Helper: create a `ScriptEngine` with a script loaded from source.
fn engine_with_script(src: &str) -> ScriptEngine {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    engine
        .load_script(src, "test.pirc", now)
        .expect("script should parse and load");
    engine
}

/// Helper: create an `EventContext` for a channel message event.
fn text_event(nick: &str, channel: &str, text: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Text),
        nick: Some(nick.to_string()),
        channel: Some(channel.to_string()),
        text: Some(text.to_string()),
        ..EventContext::default()
    }
}

/// Helper: create an `EventContext` for a JOIN event.
fn join_event(nick: &str, channel: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Join),
        nick: Some(nick.to_string()),
        channel: Some(channel.to_string()),
        ..EventContext::default()
    }
}

// ── 1. Auto-greet on join ────────────────────────────────────────────

#[test]
fn auto_greet_on_join() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:* {
    msg $chan "Welcome $nick!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = join_event("alice", "#lobby");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1, "expected one msg command");
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[0], "#lobby");
    assert_eq!(cmds[0].1[1], "Welcome alice!");
}

#[test]
fn auto_greet_only_fires_on_matching_event() {
    let engine = &mut engine_with_script(
        r#"
on JOIN:* {
    msg $chan "Welcome $nick!"
}
"#,
    );

    // JOIN fires the handler
    let mut host = MockScriptHost::new();
    let ctx = join_event("bob", "#general");
    engine.dispatch_event(EventType::Join, &ctx, &mut host);
    assert_eq!(host.commands().len(), 1);
    assert_eq!(host.commands()[0].1[1], "Welcome bob!");

    // PART does NOT fire the JOIN handler
    let mut host2 = MockScriptHost::new();
    let part_ctx = EventContext {
        event_type: Some(EventType::Part),
        nick: Some("bob".to_string()),
        channel: Some("#general".to_string()),
        ..EventContext::default()
    };
    engine.dispatch_event(EventType::Part, &part_ctx, &mut host2);
    assert!(host2.commands().is_empty());
}

// ── 2. Custom alias with if/else and variables ───────────────────────

#[test]
fn custom_alias_with_conditional_logic() {
    let engine = &mut engine_with_script(
        r#"
alias greet {
    var %greeting = "Hello"
    if ($1 == "morning") {
        set %greeting "Good morning"
    }
    msg $chan %greeting $2
}
"#,
    );

    // Test with "morning" argument
    let mut host = MockScriptHost::new();
    engine.execute_alias("greet", "morning Alice", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    // $chan, %greeting, $2 are separate arguments
    assert_eq!(cmds[0].1[1], "Good morning");
    assert_eq!(cmds[0].1[2], "Alice");

    // Test with different argument (should use default greeting)
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("greet", "evening Bob", &mut host2);

    let cmds2 = host2.commands();
    assert_eq!(cmds2.len(), 1);
    assert_eq!(cmds2[0].1[1], "Hello");
    assert_eq!(cmds2[0].1[2], "Bob");
}

// ── 3. Timer-based auto-announce ─────────────────────────────────────

#[test]
fn timer_based_auto_announce() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer announce 30 3 {
    msg $chan "Remember to check the rules!"
}
"#;
    engine.load_script(src, "announce.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Before interval: nothing
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert!(host.commands().is_empty(), "timer should not fire early");

    // First fire at 30s
    engine.tick_timers(now + Duration::from_secs(30), &mut host);
    assert_eq!(host.commands().len(), 1);
    assert_eq!(host.commands()[0].0, "msg");
    assert_eq!(host.commands()[0].1[1], "Remember to check the rules!");

    // Second fire at 60s
    engine.tick_timers(now + Duration::from_secs(60), &mut host);
    assert_eq!(host.commands().len(), 2);

    // Third fire at 90s
    engine.tick_timers(now + Duration::from_secs(90), &mut host);
    assert_eq!(host.commands().len(), 3);

    // Timer exhausted (3 repetitions), should not fire again
    engine.tick_timers(now + Duration::from_secs(120), &mut host);
    assert_eq!(host.commands().len(), 3, "timer should be exhausted");
    assert!(engine.timers().is_empty());
}

// ── 4. Text manipulation builtins ────────────────────────────────────

#[test]
fn text_manipulation_builtins() {
    let engine = &mut engine_with_script(
        r#"
alias textops {
    var %str = "Hello World"
    echo $len(%str)
    echo $upper(%str)
    echo $lower(%str)
    echo $replace(%str, "World", "Rust")
    echo $find(%str, "World")
    echo $token(%str, 1, " ")
    echo $token(%str, 2, " ")
    echo $numtok(%str, " ")
    echo $left(%str, 5)
    echo $right(%str, 5)
    echo $mid(%str, 6, 5)
    echo $strip("  padded  ")
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("textops", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "11", "$len");
    assert_eq!(echoed[1], "HELLO WORLD", "$upper");
    assert_eq!(echoed[2], "hello world", "$lower");
    assert_eq!(echoed[3], "Hello Rust", "$replace");
    assert_eq!(echoed[4], "6", "$find");
    assert_eq!(echoed[5], "Hello", "$token 1");
    assert_eq!(echoed[6], "World", "$token 2");
    assert_eq!(echoed[7], "2", "$numtok");
    assert_eq!(echoed[8], "Hello", "$left");
    assert_eq!(echoed[9], "World", "$right");
    assert_eq!(echoed[10], "World", "$mid");
    assert_eq!(echoed[11], "padded", "$strip");
}

#[test]
fn regex_and_capture_groups() {
    // Note: regex patterns with backslash-d must avoid the string escape
    // system. We use character class [0-9] instead of \d.
    let engine = &mut engine_with_script(
        r#"
alias checkregex {
    var %input = "Error 404: Not Found"
    var %matched = $regex(%input, "Error ([0-9]+): (.+)")
    echo %matched
    echo $regml(1)
    echo $regml(2)
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("checkregex", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "1", "$regex should return 1 on match");
    assert_eq!(echoed[1], "404", "first capture group");
    assert_eq!(echoed[2], "Not Found", "second capture group");
}

// ── 5. Variable scoping: local vs global ─────────────────────────────

#[test]
fn variable_scoping_local_vs_global() {
    let engine = &mut engine_with_script(
        r#"
alias setup_global {
    var %%counter = 0
}

alias increment {
    set %%counter (%%counter + 1)
    echo %%counter
}

alias local_test {
    var %x = 42
    echo %x
}
"#,
    );

    let mut host = MockScriptHost::new();

    // Set up global variable
    engine.execute_alias("setup_global", "", &mut host);

    // Increment global twice: should see 1, then 2
    engine.execute_alias("increment", "", &mut host);
    engine.execute_alias("increment", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "1", "first increment");
    assert_eq!(echoed[1], "2", "second increment (global persists)");

    // Local variables should be scoped to alias call
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("local_test", "", &mut host2);
    engine.execute_alias("local_test", "", &mut host2);

    let echoed2 = host2.echoed();
    assert_eq!(echoed2[0], "42");
    assert_eq!(echoed2[1], "42", "local var resets each call");
}

// ── 6. Multiple event handlers for same event type ───────────────────

#[test]
fn multiple_event_handlers_same_type() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*hello* {
    msg $chan "Handler 1: greeting detected"
}

on TEXT:*help* {
    msg $chan "Handler 2: help request"
}

on TEXT:* {
    msg $chan "Handler 3: catch-all"
}
"#,
    );

    // "hello" matches handler 1 and catch-all (handler 3)
    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#chat", "hello there");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 2, "hello matches handler 1 and catch-all");
    assert_eq!(cmds[0].1[1], "Handler 1: greeting detected");
    assert_eq!(cmds[1].1[1], "Handler 3: catch-all");

    // "need help" matches handler 2 and catch-all
    let mut host2 = MockScriptHost::new();
    let ctx2 = text_event("bob", "#chat", "need help please");
    engine.dispatch_event(EventType::Text, &ctx2, &mut host2);

    let cmds2 = host2.commands();
    assert_eq!(cmds2.len(), 2, "help matches handler 2 and catch-all");
    assert_eq!(cmds2[0].1[1], "Handler 2: help request");
    assert_eq!(cmds2[1].1[1], "Handler 3: catch-all");

    // "hello help" matches all three
    let mut host3 = MockScriptHost::new();
    let ctx3 = text_event("carol", "#chat", "hello can you help");
    engine.dispatch_event(EventType::Text, &ctx3, &mut host3);

    let cmds3 = host3.commands();
    assert_eq!(cmds3.len(), 3, "matches all three handlers");
}

// ── 7. Script load/unload/reload lifecycle ───────────────────────────

#[test]
fn script_load_unload_reload_lifecycle() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
alias greet {
    msg $chan "hello"
}

on JOIN:* {
    msg $chan "welcome"
}

timer reminder 60 0 {
    msg $chan "reminder"
}
"#;

    // Load: verify everything is registered
    engine.load_script(src, "main.pirc", now).unwrap();
    assert_eq!(engine.script_count(), 1);
    assert!(engine.aliases().contains("greet"));
    assert_eq!(engine.events().handler_count(), 1);
    assert!(engine.timers().contains("reminder"));

    // Verify alias actually works
    let mut host = MockScriptHost::new();
    assert!(engine.execute_alias("greet", "", &mut host));
    assert_eq!(host.commands().len(), 1);

    // Unload: verify everything is removed
    engine.unload_script("main.pirc");
    assert_eq!(engine.script_count(), 0);
    assert!(!engine.aliases().contains("greet"));
    assert_eq!(engine.events().handler_count(), 0);
    assert!(engine.timers().is_empty());

    // Verify alias no longer works after unload
    let mut host2 = MockScriptHost::new();
    assert!(!engine.execute_alias("greet", "", &mut host2));

    // Reload from stored source
    engine.load_script(src, "main.pirc", now).unwrap();
    let result = engine.reload_script("main.pirc", now);
    assert!(result.is_ok());

    // Verify re-registered
    assert_eq!(engine.script_count(), 1);
    assert!(engine.aliases().contains("greet"));
    assert_eq!(engine.events().handler_count(), 1);
    assert!(engine.timers().contains("reminder"));

    // Verify alias works again after reload
    let mut host3 = MockScriptHost::new();
    assert!(engine.execute_alias("greet", "", &mut host3));
    assert_eq!(host3.commands().len(), 1);
}

// ── 8. Error handling: runtime error does not crash engine ───────────

#[test]
fn error_handling_division_by_zero() {
    let engine = &mut engine_with_script(
        r#"
alias bad_math {
    var %x = 1 / 0
    msg $chan "this should not execute"
}

alias good_math {
    var %x = 10 / 2
    echo %x
}
"#,
    );

    let mut host = MockScriptHost::new();

    // Execute the failing alias
    engine.execute_alias("bad_math", "", &mut host);

    // Error should be reported
    let errors = host.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("division by zero"),
        "error: {}",
        errors[0]
    );
    assert!(
        errors[0].contains("alias 'bad_math'"),
        "error context: {}",
        errors[0]
    );
    // The msg command should NOT have executed
    assert!(host.commands().is_empty());

    // Engine should still work for other aliases
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("good_math", "", &mut host2);
    assert!(host2.errors().is_empty(), "good alias should succeed");
    assert_eq!(host2.echoed()[0], "5");
}

#[test]
fn error_in_event_handler_does_not_block_engine() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:*crash* {
    var %x = 1 / 0
}

alias still_works {
    echo "engine is fine"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = text_event("alice", "#test", "please crash now");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    assert_eq!(host.errors().len(), 1);

    // Engine still functional after error
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("still_works", "", &mut host2);
    assert_eq!(host2.echoed()[0], "engine is fine");
}

// ── 9. Nested alias calls ────────────────────────────────────────────

#[test]
fn nested_alias_calls() {
    let engine = &mut engine_with_script(
        r#"
alias outer {
    echo "outer start"
    middle
    echo "outer end"
}

alias middle {
    echo "middle start"
    inner
    echo "middle end"
}

alias inner {
    echo "inner"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("outer", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 5);
    assert_eq!(echoed[0], "outer start");
    assert_eq!(echoed[1], "middle start");
    assert_eq!(echoed[2], "inner");
    assert_eq!(echoed[3], "middle end");
    assert_eq!(echoed[4], "outer end");
}

#[test]
fn nested_alias_with_arguments() {
    let engine = &mut engine_with_script(
        r#"
alias format_msg {
    msg $chan $1 $2
}

alias announce {
    format_msg "ANNOUNCE:" $1
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("announce", "Server restarting", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    // format_msg receives "ANNOUNCE:" as $1 and "Server" as $2
    assert_eq!(cmds[0].1[1], "ANNOUNCE:");
    assert_eq!(cmds[0].1[2], "Server");
}

// ── 10. String interpolation in event context ────────────────────────

#[test]
fn string_interpolation_in_event_handler() {
    let engine = &mut engine_with_script(
        r#"
on TEXT:* {
    echo "$nick said something in $chan"
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.channel = Some("#general".to_string());
    let ctx = text_event("alice", "#general", "hello everyone");
    engine.dispatch_event(EventType::Text, &ctx, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "alice said something in #general");
}

#[test]
fn interpolation_with_builtins_in_alias() {
    let engine = &mut engine_with_script(
        r#"
alias whoami {
    echo "I am $me on $server in $chan"
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.nick = "pirc_bot".to_string();
    host.server = Some("irc.libera.chat".to_string());
    host.port = 6697;
    host.channel = Some("#rust".to_string());

    engine.execute_alias("whoami", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "I am pirc_bot on irc.libera.chat in #rust");
}

#[test]
fn interpolation_of_local_vars_in_strings() {
    let engine = &mut engine_with_script(
        r#"
alias test_interp {
    var %name = "World"
    echo "Hello %name!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("test_interp", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "Hello World!");
}

#[test]
fn global_vars_persist_across_alias_calls() {
    let engine = &mut engine_with_script(
        r#"
alias setup {
    var %%greeting = "Howdy"
}

alias greet {
    echo %%greeting
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("setup", "", &mut host);
    engine.execute_alias("greet", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "Howdy");
}

// ── Combined scenario ────────────────────────────────────────────────

#[test]
fn full_pipeline_combined_scenario() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();

    // Initialize global counter before loading scripts
    let init_src = r#"
alias init {
    var %%greet_count = 0
}
"#;
    engine.load_script(init_src, "init.pirc", now).unwrap();
    let mut init_host = MockScriptHost::new();
    engine.execute_alias("init", "", &mut init_host);

    let src = r#"
alias greet {
    msg $chan "Hello" $1
    set %%greet_count (%%greet_count + 1)
}

on JOIN:* {
    greet $nick
}

on TEXT:*stats* {
    echo %%greet_count
}

timer heartbeat 60 0 {
    msg $chan "still here"
}
"#;
    engine.load_script(src, "bot.pirc", now).unwrap();

    // Simulate joins
    let mut host = MockScriptHost::new();
    host.channel = Some("#main".to_string());

    let join1 = join_event("alice", "#main");
    engine.dispatch_event(EventType::Join, &join1, &mut host);

    let join2 = join_event("bob", "#main");
    engine.dispatch_event(EventType::Join, &join2, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 2, "two join events = two greet commands");
    assert_eq!(cmds[0].0, "msg");

    // Check stats via event
    let mut host2 = MockScriptHost::new();
    host2.channel = Some("#main".to_string());
    let stats_ctx = text_event("carol", "#main", "show stats please");
    engine.dispatch_event(EventType::Text, &stats_ctx, &mut host2);

    let echoed = host2.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "2");

    // Tick timer
    let mut host3 = MockScriptHost::new();
    host3.channel = Some("#main".to_string());
    engine.tick_timers(now + Duration::from_secs(60), &mut host3);

    let cmds3 = host3.commands();
    assert_eq!(cmds3.len(), 1);
    assert_eq!(cmds3[0].1[1], "still here");
}

#[test]
fn while_loop_and_variable_accumulation() {
    let engine = &mut engine_with_script(
        r#"
alias countdown {
    var %i = 5
    while (%i > 0) {
        echo %i
        set %i (%i - 1)
    }
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("countdown", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 5);
    assert_eq!(echoed[0], "5");
    assert_eq!(echoed[1], "4");
    assert_eq!(echoed[2], "3");
    assert_eq!(echoed[3], "2");
    assert_eq!(echoed[4], "1");
}

#[test]
fn multiple_scripts_interact_through_globals() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();

    let src1 = r#"
alias set_prefix {
    set %%prefix $1
}
"#;
    let src2 = r#"
alias show_prefix {
    echo %%prefix
}
"#;
    engine.load_script(src1, "config.pirc", now).unwrap();
    engine.load_script(src2, "display.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Set global from script 1
    engine.execute_alias("set_prefix", ">>", &mut host);

    // Read global from script 2
    engine.execute_alias("show_prefix", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], ">>");
}

#[test]
fn event_handler_with_part_event() {
    let engine = &mut engine_with_script(
        r#"
on PART:* {
    msg $chan "Goodbye $nick!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let ctx = EventContext {
        event_type: Some(EventType::Part),
        nick: Some("alice".to_string()),
        channel: Some("#test".to_string()),
        ..EventContext::default()
    };

    engine.dispatch_event(EventType::Part, &ctx, &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[1], "Goodbye alice!");
}
