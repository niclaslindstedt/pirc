//! Alias (custom command) integration tests.
//!
//! Covers: defining and invoking aliases, parameter passing ($1, $2, etc.),
//! alias chaining (calling another alias), overriding built-in commands,
//! conditional logic in aliases, and case-insensitive lookup.

use super::{engine_with_script, MockScriptHost};

// ── Basic alias define and invoke ───────────────────────────────────

#[test]
fn define_alias_and_invoke() {
    let engine = &mut engine_with_script(
        r#"
alias hello {
    echo "Hello, world!"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let found = engine.execute_alias("hello", "", &mut host);
    assert!(found, "alias should be found");
    assert_eq!(host.echoed(), vec!["Hello, world!"]);
}

#[test]
fn invoke_nonexistent_alias_returns_false() {
    let engine = &mut engine_with_script(
        r#"
alias existing {
    echo "exists"
}
"#,
    );

    let mut host = MockScriptHost::new();
    let found = engine.execute_alias("nonexistent", "", &mut host);
    assert!(!found, "nonexistent alias should return false");
    assert!(host.echoed().is_empty());
}

// ── Alias with parameters ($1, $2, ...) ─────────────────────────────

#[test]
fn alias_with_positional_parameters() {
    let engine = &mut engine_with_script(
        r#"
alias greet {
    msg $chan "Hello" $1 "from" $2
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("greet", "Alice Bob", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[2], "Alice");
    assert_eq!(cmds[0].1[4], "Bob");
}

#[test]
fn alias_with_dollar_zero_gets_full_args() {
    let engine = &mut engine_with_script(
        r#"
alias repeat {
    echo $0
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("repeat", "one two three", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "one two three");
}

#[test]
fn alias_parameter_beyond_provided_returns_null() {
    let engine = &mut engine_with_script(
        r#"
alias check_params {
    echo $1
    echo $5
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("check_params", "only_one", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed[0], "only_one");
    // $5 should resolve to null, which echoes as empty
    assert_eq!(echoed[1], "");
}

// ── Alias chaining (calling another alias) ──────────────────────────

#[test]
fn alias_calls_another_alias() {
    let engine = &mut engine_with_script(
        r#"
alias inner {
    echo "from inner"
}

alias outer {
    echo "before"
    inner
    echo "after"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("outer", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 3);
    assert_eq!(echoed[0], "before");
    assert_eq!(echoed[1], "from inner");
    assert_eq!(echoed[2], "after");
}

#[test]
fn three_level_alias_chaining() {
    let engine = &mut engine_with_script(
        r#"
alias level3 {
    echo "level 3"
}

alias level2 {
    echo "level 2 start"
    level3
    echo "level 2 end"
}

alias level1 {
    echo "level 1 start"
    level2
    echo "level 1 end"
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("level1", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed.len(), 5);
    assert_eq!(echoed[0], "level 1 start");
    assert_eq!(echoed[1], "level 2 start");
    assert_eq!(echoed[2], "level 3");
    assert_eq!(echoed[3], "level 2 end");
    assert_eq!(echoed[4], "level 1 end");
}

#[test]
fn alias_chaining_with_argument_forwarding() {
    let engine = &mut engine_with_script(
        r#"
alias format_msg {
    msg $chan $1 $2
}

alias announce {
    format_msg "[ANNOUNCE]" $1
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("announce", "Restarting", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[1], "[ANNOUNCE]");
    assert_eq!(cmds[0].1[2], "Restarting");
}

// ── Alias sends commands to host ────────────────────────────────────

#[test]
fn alias_sends_irc_command_to_host() {
    let engine = &mut engine_with_script(
        r#"
alias join_channel {
    join $1
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("join_channel", "#new_channel", &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "join");
    assert_eq!(cmds[0].1[0], "#new_channel");
}

// ── Case-insensitive alias lookup ───────────────────────────────────

#[test]
fn alias_lookup_is_case_insensitive() {
    let engine = &mut engine_with_script(
        r#"
alias MyAlias {
    echo "found it"
}
"#,
    );

    let mut host = MockScriptHost::new();

    // Lowercase lookup
    assert!(engine.execute_alias("myalias", "", &mut host));
    assert_eq!(host.echoed(), vec!["found it"]);

    // Uppercase lookup
    let mut host2 = MockScriptHost::new();
    assert!(engine.execute_alias("MYALIAS", "", &mut host2));
    assert_eq!(host2.echoed(), vec!["found it"]);
}

// ── Alias with conditional logic ────────────────────────────────────

#[test]
fn alias_with_if_else_conditional() {
    let engine = &mut engine_with_script(
        r#"
alias react {
    if ($1 == "good") {
        echo "thumbs up"
    } elseif ($1 == "bad") {
        echo "thumbs down"
    } else {
        echo "shrug"
    }
}
"#,
    );

    let mut host1 = MockScriptHost::new();
    engine.execute_alias("react", "good", &mut host1);
    assert_eq!(host1.echoed(), vec!["thumbs up"]);

    let mut host2 = MockScriptHost::new();
    engine.execute_alias("react", "bad", &mut host2);
    assert_eq!(host2.echoed(), vec!["thumbs down"]);

    let mut host3 = MockScriptHost::new();
    engine.execute_alias("react", "meh", &mut host3);
    assert_eq!(host3.echoed(), vec!["shrug"]);
}

// ── Alias with while loop ───────────────────────────────────────────

#[test]
fn alias_with_while_loop() {
    let engine = &mut engine_with_script(
        r#"
alias count_up {
    var %i = 1
    while (%i <= 3) {
        echo %i
        set %i (%i + 1)
    }
}
"#,
    );

    let mut host = MockScriptHost::new();
    engine.execute_alias("count_up", "", &mut host);

    let echoed = host.echoed();
    assert_eq!(echoed, vec!["1", "2", "3"]);
}

// ── Alias with return statement ─────────────────────────────────────

#[test]
fn alias_with_early_return() {
    let engine = &mut engine_with_script(
        r#"
alias check {
    if ($1 == "stop") {
        echo "stopping"
        return
    }
    echo "continuing"
}
"#,
    );

    // With "stop" - should return early
    let mut host1 = MockScriptHost::new();
    engine.execute_alias("check", "stop", &mut host1);
    assert_eq!(host1.echoed(), vec!["stopping"]);

    // Without "stop" - should continue
    let mut host2 = MockScriptHost::new();
    engine.execute_alias("check", "go", &mut host2);
    assert_eq!(host2.echoed(), vec!["continuing"]);
}

// ── Alias listing ───────────────────────────────────────────────────

#[test]
fn list_aliases_returns_all_registered() {
    let engine = &mut engine_with_script(
        r#"
alias alpha { echo "a" }
alias beta { echo "b" }
alias gamma { echo "c" }
"#,
    );

    let aliases = engine.list_aliases();
    assert_eq!(aliases.len(), 3);
    assert!(aliases.contains(&"alpha".to_string()));
    assert!(aliases.contains(&"beta".to_string()));
    assert!(aliases.contains(&"gamma".to_string()));
}
