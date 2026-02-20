//! Script loading and parsing integration tests.
//!
//! Covers: loading from source, loading from file, syntax error reporting,
//! multiple scripts loaded sequentially, UTF-8 handling, and script
//! unload/reload lifecycle.

use std::io::Write;
use std::time::Instant;

use pirc_scripting::engine::{LoadError, ScriptEngine};

use super::MockScriptHost;

// ── Load script from source ─────────────────────────────────────────

#[test]
fn load_valid_script_from_source() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias hello {
    echo "Hello, world!"
}
"#;
    let result = engine.load_script(src, "hello.pirc", now);
    assert!(result.is_ok(), "valid script should load: {result:?}");
    assert_eq!(engine.script_count(), 1);
    assert!(engine.aliases().contains("hello"));
}

#[test]
fn load_script_and_execute_verifies_full_pipeline() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias greet {
    echo "Hi there!"
}
"#;
    engine.load_script(src, "greet.pirc", now).unwrap();

    let mut host = MockScriptHost::new();
    engine.execute_alias("greet", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 1);
    assert_eq!(echoed[0], "Hi there!");
}

// ── Syntax errors ───────────────────────────────────────────────────

#[test]
fn load_script_with_syntax_error_returns_clear_error() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias broken {
    echo "missing closing brace"
"#;
    let result = engine.load_script(src, "broken.pirc", now);
    assert!(result.is_err(), "script with syntax error should fail");

    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("broken.pirc"),
        "error should mention filename: {err_str}"
    );
}

#[test]
fn load_script_with_unknown_event_type_fails() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    // FOOBAR is not a recognized event type
    let src = r#"
on FOOBAR:* {
    echo "should not work"
}
"#;
    let result = engine.load_script(src, "bad_event.pirc", now);
    assert!(
        result.is_err(),
        "unknown event type should produce error: {result:?}"
    );
}

// ── Multiple scripts loaded sequentially ────────────────────────────

#[test]
fn multiple_scripts_loaded_sequentially() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();

    let src1 = r#"
alias greet {
    echo "hello"
}
"#;
    let src2 = r#"
alias farewell {
    echo "goodbye"
}
"#;
    let src3 = r#"
alias status {
    echo "online"
}
"#;

    engine.load_script(src1, "greet.pirc", now).unwrap();
    engine.load_script(src2, "farewell.pirc", now).unwrap();
    engine.load_script(src3, "status.pirc", now).unwrap();

    assert_eq!(engine.script_count(), 3);
    assert!(engine.aliases().contains("greet"));
    assert!(engine.aliases().contains("farewell"));
    assert!(engine.aliases().contains("status"));

    // All aliases should work
    let mut host = MockScriptHost::new();
    engine.execute_alias("greet", "", &mut host);
    engine.execute_alias("farewell", "", &mut host);
    engine.execute_alias("status", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["hello", "goodbye", "online"]);
}

// ── Script file loading ─────────────────────────────────────────────

#[test]
fn load_script_from_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let file_path = dir.path().join("test_script.pirc");

    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(
            br#"
alias file_hello {
    echo "loaded from file"
}
"#,
        )
        .unwrap();
    }

    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let result = engine.load_script_file(&file_path, now);
    assert!(result.is_ok(), "file loading should succeed: {result:?}");

    let mut host = MockScriptHost::new();
    engine.execute_alias("file_hello", "", &mut host);
    assert_eq!(host.echoed(), vec!["loaded from file"]);
}

#[test]
fn load_script_from_nonexistent_file_returns_io_error() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let result = engine.load_script_file(std::path::Path::new("/nonexistent/path.pirc"), now);
    assert!(result.is_err());

    match result.unwrap_err() {
        LoadError::Io { filename, message } => {
            assert!(
                filename.contains("path.pirc"),
                "should report filename: {filename}"
            );
            assert!(!message.is_empty(), "should have error message");
        }
        other => panic!("expected Io error, got: {other:?}"),
    }
}

// ── UTF-8 with special characters ───────────────────────────────────

#[test]
fn load_script_with_utf8_special_characters() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = r#"
alias unicode {
    echo "Héllo wörld! 你好世界 🎉"
}
"#;
    engine.load_script(src, "unicode.pirc", now).unwrap();

    let mut host = MockScriptHost::new();
    engine.execute_alias("unicode", "", &mut host);
    assert_eq!(host.echoed()[0], "Héllo wörld! 你好世界 🎉");
}

// ── Script unload and reload lifecycle ──────────────────────────────

#[test]
fn unload_removes_all_registrations() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
alias test_alias {
    echo "test"
}

on JOIN:* {
    echo "joined"
}

timer test_timer 60 0 {
    echo "tick"
}
"#;
    engine.load_script(src, "lifecycle.pirc", now).unwrap();

    // Verify registered
    assert!(engine.aliases().contains("test_alias"));
    assert_eq!(engine.events().handler_count(), 1);
    assert!(engine.timers().contains("test_timer"));

    // Unload
    engine.unload_script("lifecycle.pirc");

    // Verify all removed
    assert!(!engine.aliases().contains("test_alias"));
    assert_eq!(engine.events().handler_count(), 0);
    assert!(engine.timers().is_empty());
    assert_eq!(engine.script_count(), 0);
}

#[test]
fn reload_replaces_script_registrations() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
alias greeting {
    echo "version 1"
}
"#;
    engine.load_script(src, "reload_test.pirc", now).unwrap();

    // Verify alias works
    let mut host = MockScriptHost::new();
    engine.execute_alias("greeting", "", &mut host);
    assert_eq!(host.echoed()[0], "version 1");

    // Reload
    let result = engine.reload_script("reload_test.pirc", now);
    assert!(result.is_ok());

    // Alias should still work (same source reloaded)
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("greeting", "", &mut host2);
    assert_eq!(host2.echoed()[0], "version 1");
}

#[test]
fn load_scripts_dir_loads_all_pirc_files() {
    let dir = tempfile::tempdir().expect("create temp dir");

    // Create two script files
    std::fs::write(
        dir.path().join("script_a.pirc"),
        r#"alias script_a { echo "a" }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("script_b.pirc"),
        r#"alias script_b { echo "b" }"#,
    )
    .unwrap();
    // Create a non-.pirc file (should be ignored)
    std::fs::write(dir.path().join("readme.txt"), "not a script").unwrap();

    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let results = engine.load_scripts_dir(dir.path(), now);

    // Should have loaded 2 scripts
    let successes: Vec<_> = results.iter().filter(|(_, r)| r.is_ok()).collect();
    assert_eq!(
        successes.len(),
        2,
        "should load 2 .pirc files, got results: {results:?}"
    );

    assert!(engine.aliases().contains("script_a"));
    assert!(engine.aliases().contains("script_b"));
}

// ── Empty script ────────────────────────────────────────────────────

#[test]
fn load_empty_script_succeeds() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let result = engine.load_script("", "empty.pirc", now);
    assert!(result.is_ok(), "empty script should load: {result:?}");
    assert_eq!(engine.script_count(), 1);
}

// ── Script with only comments (whitespace) ──────────────────────────

#[test]
fn load_script_with_only_whitespace_succeeds() {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    let src = "   \n\n   \n";
    let result = engine.load_script(src, "whitespace.pirc", now);
    assert!(result.is_ok(), "whitespace-only script should load: {result:?}");
}
