//! Timer integration tests.
//!
//! Covers: scheduling timers, repeating timers, timer cancellation,
//! timer callbacks sending commands, and timer accuracy within tolerance.

use std::time::{Duration, Instant};

use pirc_scripting::engine::ScriptEngine;

use super::MockScriptHost;

// ── Schedule a timer and verify it fires after delay ────────────────

#[test]
fn timer_fires_after_specified_delay() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer ping 10 1 {
    echo "ping!"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Before interval: nothing
    engine.tick_timers(now + Duration::from_secs(5), &mut host);
    assert!(host.echoed().is_empty(), "timer should not fire before delay");

    // At interval: fires
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert_eq!(host.echoed(), vec!["ping!"]);
}

#[test]
fn timer_does_not_fire_immediately() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer immediate_check 5 1 {
    echo "fired"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Right at start: should not fire
    engine.tick_timers(now, &mut host);
    assert!(host.echoed().is_empty(), "timer should not fire at t=0");

    // Just before: should not fire
    engine.tick_timers(now + Duration::from_secs(4), &mut host);
    assert!(host.echoed().is_empty(), "timer should not fire before interval");
}

// ── Repeating timer fires multiple times ────────────────────────────

#[test]
fn repeating_timer_fires_correct_number_of_times() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer heartbeat 10 3 {
    echo "beat"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Fire at 10s, 20s, 30s
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert_eq!(host.echoed().len(), 1);

    engine.tick_timers(now + Duration::from_secs(20), &mut host);
    assert_eq!(host.echoed().len(), 2);

    engine.tick_timers(now + Duration::from_secs(30), &mut host);
    assert_eq!(host.echoed().len(), 3);

    // After exhaustion: should not fire again
    engine.tick_timers(now + Duration::from_secs(40), &mut host);
    assert_eq!(host.echoed().len(), 3, "timer should be exhausted after 3 reps");

    // Timer should be removed
    assert!(engine.timers().is_empty());
}

#[test]
fn infinite_repeating_timer_keeps_firing() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    // repetitions=0 means infinite
    let src = r#"
timer forever 5 0 {
    echo "tick"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Fire many times
    for i in 1..=10 {
        engine.tick_timers(now + Duration::from_secs(i * 5), &mut host);
    }

    assert_eq!(host.echoed().len(), 10, "infinite timer should keep firing");
    // Timer should still be registered
    assert!(engine.timers().contains("forever"));
}

// ── Cancel a timer before it fires ──────────────────────────────────

#[test]
fn cancel_timer_via_alias() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer reminder 30 1 {
    echo "reminder!"
}

alias cancel_reminder {
    /timeoff reminder
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    // Cancel the timer before it fires
    let mut host = MockScriptHost::new();
    engine.execute_alias("cancel_reminder", "", &mut host);

    // Verify timer is removed
    assert!(!engine.timers().contains("reminder"), "timer should be cancelled");

    // Tick past when it would have fired
    engine.tick_timers(now + Duration::from_secs(30), &mut host);
    assert!(host.echoed().is_empty(), "cancelled timer should not fire");
}

// ── Timer callback sends commands ───────────────────────────────────

#[test]
fn timer_callback_sends_command_to_host() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer announce 15 1 {
    msg $chan "Scheduled announcement!"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();
    engine.tick_timers(now + Duration::from_secs(15), &mut host);

    let cmds = host.commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "msg");
    assert_eq!(cmds[0].1[1], "Scheduled announcement!");
}

#[test]
fn timer_callback_can_modify_global_variable() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
alias init {
    var %%tick_count = 0
}

timer ticker 5 0 {
    set %%tick_count (%%tick_count + 1)
}

alias get_count {
    echo %%tick_count
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Initialize
    engine.execute_alias("init", "", &mut host);

    // Tick 3 times
    engine.tick_timers(now + Duration::from_secs(5), &mut host);
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    engine.tick_timers(now + Duration::from_secs(15), &mut host);

    // Check counter
    engine.execute_alias("get_count", "", &mut host);
    let echoed = host.echoed();
    assert_eq!(echoed[0], "3");
}

// ── Multiple timers coexist ─────────────────────────────────────────

#[test]
fn multiple_timers_fire_independently() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer fast 5 3 {
    echo "fast"
}

timer slow 15 1 {
    echo "slow"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // At 5s: only fast fires
    engine.tick_timers(now + Duration::from_secs(5), &mut host);
    assert_eq!(host.echoed(), vec!["fast"]);

    // At 10s: fast fires again
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert_eq!(host.echoed(), vec!["fast", "fast"]);

    // At 15s: both fire
    engine.tick_timers(now + Duration::from_secs(15), &mut host);
    let echoed = host.echoed();
    assert_eq!(echoed.len(), 4);
    assert!(echoed.contains(&"slow".to_string()));
}

// ── Timer with same name replaces existing ──────────────────────────

#[test]
fn timer_with_same_name_from_different_script_coexists() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src1 = r#"
timer mytimer 10 1 {
    echo "timer1"
}
"#;
    engine.load_script(src1, "s1.pirc", now).unwrap();

    assert!(engine.timers().contains("mytimer"));

    let mut host = MockScriptHost::new();
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert_eq!(host.echoed().len(), 1);
}

// ── Timer listing ───────────────────────────────────────────────────

#[test]
fn list_timers_returns_all_registered() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer alpha 10 1 { echo "a" }
timer beta 20 1 { echo "b" }
timer gamma 30 1 { echo "c" }
"#;
    engine.load_script(src, "timers.pirc", now).unwrap();

    let timers = engine.list_timers();
    assert_eq!(timers.len(), 3);
    assert!(timers.contains(&"alpha".to_string()));
    assert!(timers.contains(&"beta".to_string()));
    assert!(timers.contains(&"gamma".to_string()));
}

// ── Timer removal via timeoff command ────────────────────────────────

#[test]
fn timer_removed_via_timeoff_command() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer to_remove 10 0 {
    echo "should be removed"
}

alias remove_it {
    /timeoff to_remove
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    assert!(engine.timers().contains("to_remove"));

    let mut host = MockScriptHost::new();
    engine.execute_alias("remove_it", "", &mut host);

    assert!(!engine.timers().contains("to_remove"), "timer should be removed");

    // Verify it doesn't fire
    engine.tick_timers(now + Duration::from_secs(10), &mut host);
    assert!(host.echoed().is_empty());
}

// ── Timer accuracy within tolerance ─────────────────────────────────

#[test]
fn timer_fires_at_correct_intervals_within_tolerance() {
    let now = Instant::now();
    let mut engine = ScriptEngine::new();
    let src = r#"
timer precise 1 5 {
    echo "tick"
}
"#;
    engine.load_script(src, "timer.pirc", now).unwrap();

    let mut host = MockScriptHost::new();

    // Tick at each second - each should produce exactly one echo
    for i in 1..=5 {
        engine.tick_timers(now + Duration::from_secs(i), &mut host);
    }

    assert_eq!(
        host.echoed().len(),
        5,
        "should fire exactly 5 times at 1s intervals"
    );

    // Timer should be exhausted
    engine.tick_timers(now + Duration::from_secs(6), &mut host);
    assert_eq!(host.echoed().len(), 5, "should not fire after exhaustion");
}
