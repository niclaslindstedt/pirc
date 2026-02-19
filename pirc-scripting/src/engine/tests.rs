use super::*;
use crate::ast::EventType;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A test command handler that records all dispatched commands.
struct TestCmdHandler {
    commands: Arc<Mutex<Vec<(String, Vec<String>)>>>,
}

impl TestCmdHandler {
    fn new() -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn commands(&self) -> Vec<(String, Vec<String>)> {
        self.commands.lock().unwrap().clone()
    }
}

impl CommandHandler for TestCmdHandler {
    fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
        self.commands.lock().unwrap().push((
            name.to_string(),
            args.iter().map(ToString::to_string).collect(),
        ));
        Ok(())
    }
}

// ── Construction tests ────────────────────────────────────────────

#[test]
fn new_engine_is_empty() {
    let engine = ScriptEngine::new();
    assert_eq!(engine.script_count(), 0);
    assert!(engine.list_scripts().is_empty());
    assert!(engine.list_aliases().is_empty());
    assert!(engine.list_timers().is_empty());
}

#[test]
fn default_engine_is_empty() {
    let engine = ScriptEngine::default();
    assert_eq!(engine.script_count(), 0);
}

// ── Script loading tests ──────────────────────────────────────────

#[test]
fn load_script_registers_alias() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "hello"
}
"#;
    let result = engine.load_script(src, "test.pirc", now);
    assert!(result.is_ok());
    assert_eq!(engine.script_count(), 1);
    assert!(engine.aliases().contains("greet"));
    assert_eq!(engine.list_aliases(), vec!["greet"]);
}

#[test]
fn load_script_registers_event() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
on JOIN:* {
    msg $chan "Welcome $nick!"
}
"#;
    let result = engine.load_script(src, "test.pirc", now);
    assert!(result.is_ok());
    assert_eq!(engine.events().handler_count(), 1);
}

#[test]
fn load_script_registers_timer() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
timer keepalive 60 0 {
    msg $chan "ping"
}
"#;
    let result = engine.load_script(src, "test.pirc", now);
    assert!(result.is_ok());
    assert!(engine.timers().contains("keepalive"));
    assert_eq!(engine.list_timers(), vec!["keepalive"]);
}

#[test]
fn load_script_returns_warnings() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias test {
    msg $chan %undeclared
}
"#;
    let result = engine.load_script(src, "test.pirc", now).unwrap();
    assert!(!result.warnings.is_empty());
}

#[test]
fn load_script_rejects_semantic_errors() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias test {
    break
}
"#;
    let result = engine.load_script(src, "test.pirc", now);
    assert!(result.is_err());
    assert_eq!(engine.script_count(), 0);
}

#[test]
fn load_script_rejects_parse_errors() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = "alias { }"; // Missing alias name
    let result = engine.load_script(src, "bad.pirc", now);
    assert!(result.is_err());
}

#[test]
fn load_script_replaces_existing() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();

    let src1 = r#"
alias greet {
    msg $chan "hello"
}
"#;
    engine.load_script(src1, "test.pirc", now).unwrap();
    assert!(engine.aliases().contains("greet"));

    let src2 = r#"
alias farewell {
    msg $chan "goodbye"
}
"#;
    engine.load_script(src2, "test.pirc", now).unwrap();
    // Old alias should be gone, new one registered
    assert!(!engine.aliases().contains("greet"));
    assert!(engine.aliases().contains("farewell"));
    assert_eq!(engine.script_count(), 1);
}

// ── Unload tests ──────────────────────────────────────────────────

#[test]
fn unload_removes_aliases() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "hello"
}
alias farewell {
    msg $chan "bye"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    assert_eq!(engine.list_aliases().len(), 2);

    engine.unload_script("test.pirc");
    assert!(engine.list_aliases().is_empty());
    assert_eq!(engine.script_count(), 0);
}

#[test]
fn unload_removes_events() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
on JOIN:* {
    msg $chan "welcome"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    assert_eq!(engine.events().handler_count(), 1);

    engine.unload_script("test.pirc");
    assert_eq!(engine.events().handler_count(), 0);
}

#[test]
fn unload_removes_timers() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
timer keepalive 60 0 {
    msg $chan "ping"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    assert!(!engine.timers().is_empty());

    engine.unload_script("test.pirc");
    assert!(engine.timers().is_empty());
}

#[test]
fn unload_nonexistent_is_noop() {
    let mut engine = ScriptEngine::new();
    engine.unload_script("nonexistent.pirc");
    assert_eq!(engine.script_count(), 0);
}

// ── Reload tests ──────────────────────────────────────────────────

#[test]
fn reload_script() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "hello"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    let result = engine.reload_script("test.pirc", now);
    assert!(result.is_ok());
    assert!(engine.aliases().contains("greet"));
}

#[test]
fn reload_nonexistent_fails() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let result = engine.reload_script("missing.pirc", now);
    assert!(result.is_err());
}

// ── Event dispatch tests ──────────────────────────────────────────

#[test]
fn dispatch_event_fires_handler() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
on TEXT:*hello* {
    msg $chan "Hi $nick!"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();
    let ctx = EventContext {
        event_type: Some(EventType::Text),
        nick: Some("alice".to_string()),
        channel: Some("#test".to_string()),
        text: Some("hello world".to_string()),
        ..EventContext::default()
    };

    engine.dispatch_event(EventType::Text, &ctx, &mut handler);

    let cmds = handler.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
}

#[test]
fn dispatch_event_no_match_is_noop() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
on TEXT:*goodbye* {
    msg $chan "bye"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();
    let ctx = EventContext {
        event_type: Some(EventType::Text),
        text: Some("hello".to_string()),
        ..EventContext::default()
    };

    engine.dispatch_event(EventType::Text, &ctx, &mut handler);
    assert!(handler.commands().is_empty());
}

// ── Alias execution tests ─────────────────────────────────────────

#[test]
fn execute_alias_runs_body() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "hello"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();
    let found = engine.execute_alias("greet", "", &mut handler);
    assert!(found);

    let cmds = handler.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
}

#[test]
fn execute_alias_not_found() {
    let mut engine = ScriptEngine::new();
    let mut handler = TestCmdHandler::new();
    let found = engine.execute_alias("nonexistent", "", &mut handler);
    assert!(!found);
}

// ── Command execution tests ───────────────────────────────────────

#[test]
fn execute_command_finds_alias() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "hello"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();
    let handled = engine.execute_command("greet alice", &mut handler);
    assert!(handled);
}

#[test]
fn execute_command_no_match_returns_false() {
    let mut engine = ScriptEngine::new();
    let mut handler = TestCmdHandler::new();
    let handled = engine.execute_command("unknown", &mut handler);
    assert!(!handled);
}

#[test]
fn execute_command_empty_input() {
    let mut engine = ScriptEngine::new();
    let mut handler = TestCmdHandler::new();
    let handled = engine.execute_command("", &mut handler);
    assert!(!handled);
}

// ── Timer tick tests ──────────────────────────────────────────────

#[test]
fn tick_timers_fires_due_timer() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
timer heartbeat 5 1 {
    msg $chan "tick"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();

    // Before interval: nothing fires
    engine.tick_timers(now + Duration::from_secs(3), &mut handler);
    assert!(handler.commands().is_empty());

    // At interval: timer fires
    engine.tick_timers(now + Duration::from_secs(5), &mut handler);
    let cmds = handler.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
}

// ── Builtin setter tests ──────────────────────────────────────────

#[test]
fn set_builtin_updates_context() {
    let mut engine = ScriptEngine::new();
    engine.set_builtin("nick", Value::String("testbot".to_string()));
    // The builtin context is internal; verify by loading a script that uses it
    let now = Instant::now();
    let src = r#"
alias whoami {
    msg $chan $nick
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    // Alias uses the builtin context set above
    // (Full verification requires dispatch, covered by integration tests)
    assert!(engine.aliases().contains("whoami"));
}

// ── List methods tests ────────────────────────────────────────────

#[test]
fn list_scripts_returns_sorted() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();

    engine
        .load_script(
            "alias b { msg $chan \"b\" }",
            "beta.pirc",
            now,
        )
        .unwrap();
    engine
        .load_script(
            "alias a { msg $chan \"a\" }",
            "alpha.pirc",
            now,
        )
        .unwrap();

    assert_eq!(engine.list_scripts(), vec!["alpha.pirc", "beta.pirc"]);
}

#[test]
fn list_aliases_sorted() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias zebra { msg $chan "z" }
alias alpha { msg $chan "a" }
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    assert_eq!(engine.list_aliases(), vec!["alpha", "zebra"]);
}

// ── Error callback tests ──────────────────────────────────────────

#[test]
fn error_callback_called_on_runtime_error() {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let errors_clone = errors.clone();

    let mut engine = ScriptEngine::new();
    engine.set_error_callback(move |msg| {
        errors_clone.lock().unwrap().push(msg.to_string());
    });

    // Load a script with a handler that causes a runtime error (division by zero)
    let now = Instant::now();
    let src = r#"
on TEXT:* {
    var %x = 1 / 0
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();

    let mut handler = TestCmdHandler::new();
    let ctx = EventContext {
        event_type: Some(EventType::Text),
        text: Some("hello".to_string()),
        ..EventContext::default()
    };

    engine.dispatch_event(EventType::Text, &ctx, &mut handler);

    let errors = errors.lock().unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("division by zero"));
}

// ── File loading tests ────────────────────────────────────────────

#[test]
fn load_script_file_nonexistent() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let result = engine.load_script_file(Path::new("/nonexistent/test.pirc"), now);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), LoadError::Io { .. }));
}

#[test]
fn load_scripts_dir_nonexistent() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let results = engine.load_scripts_dir(Path::new("/nonexistent/dir"), now);
    assert_eq!(results.len(), 1);
    assert!(results[0].1.is_err());
}

// ── Integration test: full pipeline ───────────────────────────────

#[test]
fn integration_load_dispatch_alias() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    msg $chan "Hello from greet!"
}

on TEXT:*hi* {
    msg $chan "Someone said hi!"
}
"#;
    engine.load_script(src, "main.pirc", now).unwrap();

    // Execute the alias
    let mut handler = TestCmdHandler::new();
    assert!(engine.execute_alias("greet", "", &mut handler));
    assert_eq!(handler.commands().len(), 1);
    assert_eq!(handler.commands()[0].0, "msg");

    // Dispatch an event
    let mut handler2 = TestCmdHandler::new();
    let ctx = EventContext {
        event_type: Some(EventType::Text),
        text: Some("hi there".to_string()),
        channel: Some("#general".to_string()),
        ..EventContext::default()
    };
    engine.dispatch_event(EventType::Text, &ctx, &mut handler2);
    assert_eq!(handler2.commands().len(), 1);
}

#[test]
fn integration_multiple_scripts_isolated() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();

    let src1 = r#"
alias cmd1 { msg $chan "from script1" }
on JOIN:* { msg $chan "welcome from s1" }
"#;
    let src2 = r#"
alias cmd2 { msg $chan "from script2" }
on PART:* { msg $chan "bye from s2" }
"#;
    engine.load_script(src1, "s1.pirc", now).unwrap();
    engine.load_script(src2, "s2.pirc", now).unwrap();

    assert_eq!(engine.script_count(), 2);
    assert_eq!(engine.list_aliases().len(), 2);
    assert_eq!(engine.events().handler_count(), 2);

    // Unload only s1
    engine.unload_script("s1.pirc");
    assert_eq!(engine.script_count(), 1);
    assert!(!engine.aliases().contains("cmd1"));
    assert!(engine.aliases().contains("cmd2"));
    assert_eq!(engine.events().handler_count(), 1);
}

#[test]
fn integration_timer_full_lifecycle() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
timer ping 10 2 {
    msg $chan "ping"
}
"#;
    engine.load_script(src, "test.pirc", now).unwrap();
    assert_eq!(engine.list_timers(), vec!["ping"]);

    let mut handler = TestCmdHandler::new();

    // First tick at +10s
    engine.tick_timers(now + Duration::from_secs(10), &mut handler);
    assert_eq!(handler.commands().len(), 1);

    // Second tick at +20s (last repetition)
    engine.tick_timers(now + Duration::from_secs(20), &mut handler);
    assert_eq!(handler.commands().len(), 2);

    // Timer should be exhausted
    assert!(engine.timers().is_empty());
}

#[test]
fn load_error_display() {
    let err = LoadError::Script {
        filename: "test.pirc".to_string(),
        error: ScriptError::Lex(crate::error::LexError::UnexpectedCharacter {
            ch: '@',
            span: crate::token::Span::new(0, 1),
            location: crate::error::SourceLocation::new(1, 1),
        }),
    };
    let msg = err.to_string();
    assert!(msg.contains("test.pirc"));
    assert!(msg.contains("unexpected character"));

    let err = LoadError::Io {
        filename: "test.pirc".to_string(),
        message: "file not found".to_string(),
    };
    assert!(err.to_string().contains("I/O error"));
}
