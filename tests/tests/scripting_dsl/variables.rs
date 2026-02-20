//! Variable management integration tests.
//!
//! Covers: local variables within script scope, global variables across
//! scopes, variable interpolation in messages, variable persistence
//! across event invocations, and variable types.

use pirc_scripting::ast::EventType;

use super::{engine_with_script, text_event, MockScriptHost};

// ── Local variables within alias scope ──────────────────────────────

#[test]
fn local_variable_declaration_and_access() {
    let engine = &mut engine_with_script(
        r#"
alias test_local {
    var %x = 42
    echo %x
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("test_local", "", &mut host);
    assert_eq!(host.echoed(), vec!["42"]);
}

#[test]
fn local_variable_resets_each_call() {
    let engine = &mut engine_with_script(
        r#"
alias counter {
    var %x = 0
    set %x (%x + 1)
    echo %x
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("counter", "", &mut host);
    engine.execute_alias("counter", "", &mut host);
    engine.execute_alias("counter", "", &mut host);

    let echoed = host.echoed();
    // Each call should see 1, because %x resets each time
    assert_eq!(echoed, vec!["1", "1", "1"]);
}

#[test]
fn local_variables_isolated_between_aliases() {
    let engine = &mut engine_with_script(
        r#"
alias set_var {
    var %secret = "hidden"
    echo %secret
}

alias read_var {
    ; %secret is not declared here, accessing it may produce an error
    ; which means no echo output from this alias
    echo %secret
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("set_var", "", &mut host);
    engine.execute_alias("read_var", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "hidden", "set_var sees its own local");
    // read_var either echoes empty/null or produces an error -
    // the key point is %secret is NOT "hidden" from set_var's scope
    if echoed.len() > 1 {
        assert_ne!(echoed[1], "hidden", "read_var should not see set_var's local");
    }
}

// ── Global variables across scopes ──────────────────────────────────

#[test]
fn global_variable_set_and_get() {
    let engine = &mut engine_with_script(
        r#"
alias init {
    var %%name = "World"
}

alias greet {
    echo %%name
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("init", "", &mut host);
    engine.execute_alias("greet", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "World");
}

#[test]
fn global_variable_persists_across_alias_calls() {
    let engine = &mut engine_with_script(
        r#"
alias init_counter {
    var %%count = 0
}

alias increment {
    set %%count (%%count + 1)
    echo %%count
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("init_counter", "", &mut host);
    engine.execute_alias("increment", "", &mut host);
    engine.execute_alias("increment", "", &mut host);
    engine.execute_alias("increment", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["1", "2", "3"]);
}

#[test]
fn global_variable_persists_across_event_invocations() {
    let engine = &mut engine_with_script(
        r#"
alias init {
    var %%msg_count = 0
}

on TEXT:* {
    set %%msg_count (%%msg_count + 1)
    echo %%msg_count
}
"#,
    );

    let mut host = MockScriptHost::new();

    // Initialize
    engine.execute_alias("init", "", &mut host);

    // Dispatch multiple text events
    let ctx1 = text_event("alice", "#test", "msg1");
    engine.dispatch_event(EventType::Text, &ctx1, &mut host);

    let ctx2 = text_event("bob", "#test", "msg2");
    engine.dispatch_event(EventType::Text, &ctx2, &mut host);

    let ctx3 = text_event("carol", "#test", "msg3");
    engine.dispatch_event(EventType::Text, &ctx3, &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["1", "2", "3"]);
}

// ── Global variables shared across scripts ──────────────────────────

#[test]
fn global_variables_shared_across_different_scripts() {
    let now = std::time::Instant::now();
    let mut engine = pirc_scripting::engine::ScriptEngine::new();

    let src1 = r#"
alias set_global {
    var %%shared = "from_script1"
}
"#;
    let src2 = r#"
alias get_global {
    echo %%shared
}
"#;
    engine.load_script(src1, "writer.pirc", now).unwrap();
    engine.load_script(src2, "reader.pirc", now).unwrap();

    let mut host = MockScriptHost::new();
    engine.execute_alias("set_global", "", &mut host);
    engine.execute_alias("get_global", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "from_script1");
}

// ── Variable interpolation in strings ───────────────────────────────

#[test]
fn local_variable_interpolated_in_string() {
    let engine = &mut engine_with_script(
        r#"
alias greet {
    var %name = "Alice"
    echo "Hello %name, welcome!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("greet", "", &mut host);
    assert_eq!(host.echoed()[0], "Hello Alice, welcome!");
}

#[test]
fn global_variable_used_in_command_args() {
    let engine = &mut engine_with_script(
        r#"
alias setup {
    var %%prefix = "[BOT]"
}

alias say {
    echo %%prefix "Hello!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("setup", "", &mut host);
    engine.execute_alias("say", "", &mut host);
    assert_eq!(host.echoed()[0], "[BOT] Hello!");
}

#[test]
fn builtin_variable_interpolation_in_string() {
    let engine = &mut engine_with_script(
        r#"
alias whoami {
    echo "I am $me on $server"
}
"#,
    );

    let mut host = MockScriptHost::new();
    host.nick = "mybot".to_string();
    host.server = Some("irc.test.net".to_string());
    engine.execute_alias("whoami", "", &mut host);

    assert_eq!(host.echoed()[0], "I am mybot on irc.test.net");
}

// ── Variable types ──────────────────────────────────────────────────

#[test]
fn variable_types_string_int_bool() {
    let engine = &mut engine_with_script(
        r#"
alias type_test {
    var %str = "hello"
    var %num = 42
    var %flag = $true
    echo %str
    echo %num
    echo %flag
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("type_test", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "hello");
    assert_eq!(echoed[1], "42");
    assert_eq!(echoed[2], "true");
}

// ── Variable arithmetic ─────────────────────────────────────────────

#[test]
fn variable_arithmetic_operations() {
    let engine = &mut engine_with_script(
        r#"
alias math {
    var %a = 10
    var %b = 3
    echo (%a + %b)
    echo (%a - %b)
    echo (%a * %b)
    echo (%a / %b)
    echo (%a % %b)
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("math", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "13");
    assert_eq!(echoed[1], "7");
    assert_eq!(echoed[2], "30");
    assert_eq!(echoed[3], "3");
    assert_eq!(echoed[4], "1");
}

// ── Variable comparison ─────────────────────────────────────────────

#[test]
fn variable_comparison_operators() {
    let engine = &mut engine_with_script(
        r#"
alias compare {
    var %x = 5
    if (%x == 5) {
        echo "equal"
    }
    if (%x != 3) {
        echo "not equal"
    }
    if (%x > 3) {
        echo "greater"
    }
    if (%x < 10) {
        echo "less"
    }
    if (%x >= 5) {
        echo "gte"
    }
    if (%x <= 5) {
        echo "lte"
    }
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("compare", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(
        echoed,
        vec!["equal", "not equal", "greater", "less", "gte", "lte"]
    );
}

// ── String concatenation via add ────────────────────────────────────

#[test]
fn string_concatenation_with_plus() {
    let engine = &mut engine_with_script(
        r#"
alias concat {
    var %a = "Hello"
    var %b = " World"
    var %c = (%a + %b)
    echo %c
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("concat", "", &mut host);
    assert_eq!(host.echoed()[0], "Hello World");
}

// ── Null variable access ────────────────────────────────────────────

#[test]
fn unset_variable_resolves_to_null() {
    let engine = &mut engine_with_script(
        r#"
alias check_null {
    if (%%undefined == $null) {
        echo "is null"
    } else {
        echo "not null"
    }
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("check_null", "", &mut host);
    assert_eq!(host.echoed()[0], "is null");
}

// ── Variable set (update) vs var (declare) ──────────────────────────

#[test]
fn set_updates_existing_variable() {
    let engine = &mut engine_with_script(
        r#"
alias test_set {
    var %x = 1
    echo %x
    set %x 2
    echo %x
    set %x (%x + 10)
    echo %x
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("test_set", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["1", "2", "12"]);
}
