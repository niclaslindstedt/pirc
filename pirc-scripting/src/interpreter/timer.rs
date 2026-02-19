//! Timer management for scheduled and repeating script actions.
//!
//! The [`TimerManager`] tracks active timers and determines which should fire
//! based on elapsed time. Each timer has a name, interval, optional repetition
//! limit, and a body of statements to execute when fired.
//!
//! Time is injected via `std::time::Instant` parameters, making the manager
//! fully testable without async or real-time dependencies.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::ast::Statement;

use super::builtins::BuiltinContext;
use super::environment::Environment;
use super::functions::{FunctionRegistry, RegexState};
use super::{CommandHandler, Interpreter, RuntimeError};

/// A single active timer.
#[derive(Debug, Clone)]
struct Timer {
    /// Interval between firings.
    interval: Duration,
    /// Remaining repetitions. `None` means infinite (repeat forever).
    remaining: Option<u64>,
    /// The statements to execute when the timer fires.
    body: Vec<Statement>,
    /// When this timer should next fire.
    next_fire: Instant,
}

/// A timer that is ready to fire, returned by [`TimerManager::tick`].
#[derive(Debug, Clone)]
pub struct FiredTimer {
    /// The timer name.
    pub name: String,
    /// The statements to execute.
    pub body: Vec<Statement>,
}

/// Manages active timers for the scripting engine.
///
/// Timers are registered by name and fire at regular intervals. The manager
/// is tick-driven: callers pass the current `Instant` and receive back the
/// list of timers that should fire. This design avoids async and makes
/// testing deterministic.
#[derive(Debug, Clone)]
pub struct TimerManager {
    /// Active timers indexed by lowercased name.
    timers: HashMap<String, Timer>,
}

impl Default for TimerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerManager {
    /// Creates a new empty timer manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timers: HashMap::new(),
        }
    }

    /// Registers a timer with the given name, interval, repetition count, and body.
    ///
    /// If a timer with the same name already exists, it is replaced.
    /// A repetition count of 0 means the timer repeats indefinitely.
    /// The timer's first firing is scheduled at `now + interval`.
    pub fn register(
        &mut self,
        name: &str,
        interval: Duration,
        repetitions: u64,
        body: Vec<Statement>,
        now: Instant,
    ) {
        let remaining = if repetitions == 0 {
            None
        } else {
            Some(repetitions)
        };
        self.timers.insert(
            name.to_lowercase(),
            Timer {
                interval,
                remaining,
                body,
                next_fire: now + interval,
            },
        );
    }

    /// Removes a timer by name (case-insensitive).
    ///
    /// Returns `true` if a timer was removed, `false` if no timer with that name existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.timers.remove(&name.to_lowercase()).is_some()
    }

    /// Advances time and returns all timers that should fire.
    ///
    /// For each timer whose `next_fire <= now`:
    /// - The timer's body is included in the returned list.
    /// - If the timer has finite repetitions, the count is decremented.
    /// - If repetitions are exhausted, the timer is removed.
    /// - Otherwise, `next_fire` is advanced by the interval.
    pub fn tick(&mut self, now: Instant) -> Vec<FiredTimer> {
        let mut fired = Vec::new();
        let mut to_remove = Vec::new();

        for (name, timer) in &mut self.timers {
            if timer.next_fire <= now {
                fired.push(FiredTimer {
                    name: name.clone(),
                    body: timer.body.clone(),
                });

                // Update timer state
                if let Some(ref mut remaining) = timer.remaining {
                    *remaining -= 1;
                    if *remaining == 0 {
                        to_remove.push(name.clone());
                        continue;
                    }
                }
                timer.next_fire += timer.interval;
            }
        }

        for name in to_remove {
            self.timers.remove(&name);
        }

        fired
    }

    /// Returns the number of active timers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    /// Returns `true` if there are no active timers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }

    /// Returns `true` if a timer with the given name exists (case-insensitive).
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.timers.contains_key(&name.to_lowercase())
    }

    /// Returns the names of all active timers.
    #[must_use]
    pub fn timer_names(&self) -> Vec<String> {
        self.timers.keys().cloned().collect()
    }

    /// Returns a summary of all active timers for display by the `/timers` command.
    ///
    /// Each entry is formatted as `"name: interval=Xs, repetitions=N|infinite"`.
    #[must_use]
    pub fn timer_info(&self) -> Vec<String> {
        let mut info: Vec<String> = self
            .timers
            .iter()
            .map(|(name, timer)| {
                let reps = match timer.remaining {
                    Some(n) => format!("{n}"),
                    None => "infinite".to_string(),
                };
                format!(
                    "{name}: interval={:.1}s, repetitions={reps}",
                    timer.interval.as_secs_f64()
                )
            })
            .collect();
        info.sort();
        info
    }

    /// Executes a fired timer's body in a fresh local scope.
    ///
    /// Timer bodies can access global variables but get their own local scope.
    /// `Return` is caught as normal completion (like aliases and events).
    ///
    /// # Errors
    ///
    /// Returns a [`RuntimeError`] if the timer body encounters a runtime error
    /// (except `Return`, which is caught).
    pub fn execute_fired(
        fired: &FiredTimer,
        env: &mut Environment,
        cmd_handler: &mut dyn CommandHandler,
        builtin_ctx: &BuiltinContext,
        functions: &FunctionRegistry,
        regex_state: &RegexState,
    ) -> Result<(), RuntimeError> {
        env.push_scope();

        let mut interp =
            Interpreter::with_context(env, cmd_handler, builtin_ctx, functions, regex_state);
        let result = interp.exec_stmts(&fired.body);

        env.pop_scope();

        match result {
            Ok(_) | Err(RuntimeError::Return(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CommandStatement, Expression, ReturnStatement, VarDeclStatement};
    use crate::interpreter::command::AliasRegistry;
    use crate::interpreter::value::Value;
    use crate::token::Span;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn echo_stmt(text: &str) -> Statement {
        Statement::Command(CommandStatement {
            name: "echo".to_string(),
            args: vec![Expression::StringLiteral {
                value: text.to_string(),
                span: span(),
            }],
            span: span(),
        })
    }

    // ── Registration tests ─────────────────────────────────────────────

    #[test]
    fn new_manager_is_empty() {
        let mgr = TimerManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn register_timer() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![echo_stmt("hi")], now);

        assert_eq!(mgr.len(), 1);
        assert!(mgr.contains("test"));
    }

    #[test]
    fn register_case_insensitive() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("MyTimer", Duration::from_secs(10), 0, vec![], now);

        assert!(mgr.contains("mytimer"));
        assert!(mgr.contains("MYTIMER"));
        assert!(mgr.contains("MyTimer"));
    }

    #[test]
    fn register_replaces_existing() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 5, vec![echo_stmt("old")], now);
        mgr.register("test", Duration::from_secs(20), 3, vec![echo_stmt("new")], now);

        assert_eq!(mgr.len(), 1);
    }

    // ── Removal tests ──────────────────────────────────────────────────

    #[test]
    fn remove_existing_timer() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![], now);

        assert!(mgr.remove("test"));
        assert!(mgr.is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut mgr = TimerManager::new();
        assert!(!mgr.remove("nope"));
    }

    #[test]
    fn remove_case_insensitive() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("MyTimer", Duration::from_secs(10), 0, vec![], now);

        assert!(mgr.remove("MYTIMER"));
        assert!(mgr.is_empty());
    }

    // ── Tick / firing tests ────────────────────────────────────────────

    #[test]
    fn tick_before_interval_returns_nothing() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![echo_stmt("hi")], now);

        // Tick 5 seconds later — too early
        let fired = mgr.tick(now + Duration::from_secs(5));
        assert!(fired.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn tick_at_interval_fires() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![echo_stmt("hi")], now);

        let fired = mgr.tick(now + Duration::from_secs(10));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "test");
    }

    #[test]
    fn tick_past_interval_fires() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![echo_stmt("hi")], now);

        let fired = mgr.tick(now + Duration::from_secs(15));
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn infinite_timer_continues_after_fire() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(10), 0, vec![echo_stmt("hi")], now);

        // First fire
        let fired = mgr.tick(now + Duration::from_secs(10));
        assert_eq!(fired.len(), 1);
        assert_eq!(mgr.len(), 1); // Still active

        // Should not fire again at +15 (next fire is at +20)
        let fired = mgr.tick(now + Duration::from_secs(15));
        assert!(fired.is_empty());

        // Should fire again at +20
        let fired = mgr.tick(now + Duration::from_secs(20));
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn finite_timer_decrements_and_removes() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(5), 3, vec![echo_stmt("hi")], now);

        // Fire 1
        let fired = mgr.tick(now + Duration::from_secs(5));
        assert_eq!(fired.len(), 1);
        assert_eq!(mgr.len(), 1); // 2 remaining

        // Fire 2
        let fired = mgr.tick(now + Duration::from_secs(10));
        assert_eq!(fired.len(), 1);
        assert_eq!(mgr.len(), 1); // 1 remaining

        // Fire 3 — last one, should be removed
        let fired = mgr.tick(now + Duration::from_secs(15));
        assert_eq!(fired.len(), 1);
        assert!(mgr.is_empty()); // Removed
    }

    #[test]
    fn single_repetition_timer_fires_once_then_removed() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("once", Duration::from_secs(5), 1, vec![echo_stmt("once")], now);

        let fired = mgr.tick(now + Duration::from_secs(5));
        assert_eq!(fired.len(), 1);
        assert!(mgr.is_empty());
    }

    #[test]
    fn multiple_timers_fire_independently() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("fast", Duration::from_secs(5), 0, vec![echo_stmt("fast")], now);
        mgr.register("slow", Duration::from_secs(20), 0, vec![echo_stmt("slow")], now);

        // At +5, only fast should fire
        let fired = mgr.tick(now + Duration::from_secs(5));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "fast");

        // At +10, fast fires again
        let fired = mgr.tick(now + Duration::from_secs(10));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "fast");

        // At +20, both fire
        let mut fired = mgr.tick(now + Duration::from_secs(20));
        assert_eq!(fired.len(), 2);
        fired.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(fired[0].name, "fast");
        assert_eq!(fired[1].name, "slow");
    }

    #[test]
    fn timer_names_returns_all_names() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("alpha", Duration::from_secs(10), 0, vec![], now);
        mgr.register("beta", Duration::from_secs(10), 0, vec![], now);

        let mut names = mgr.timer_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn fired_timer_body_matches_registered() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        let body = vec![echo_stmt("hello"), echo_stmt("world")];
        mgr.register("test", Duration::from_secs(1), 0, body.clone(), now);

        let fired = mgr.tick(now + Duration::from_secs(1));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].body.len(), 2);
    }

    #[test]
    fn replace_timer_resets_schedule() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("test", Duration::from_secs(5), 0, vec![], now);

        // At +4, replace with a 10-second timer
        let later = now + Duration::from_secs(4);
        mgr.register("test", Duration::from_secs(10), 0, vec![], later);

        // At +5 (original fire time), should NOT fire (reset to +14)
        let fired = mgr.tick(now + Duration::from_secs(5));
        assert!(fired.is_empty());

        // At +14, should fire
        let fired = mgr.tick(now + Duration::from_secs(14));
        assert_eq!(fired.len(), 1);
    }

    // ── timer_info tests ───────────────────────────────────────────────

    #[test]
    fn timer_info_empty() {
        let mgr = TimerManager::new();
        assert!(mgr.timer_info().is_empty());
    }

    #[test]
    fn timer_info_shows_infinite() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("keepalive", Duration::from_secs(300), 0, vec![], now);

        let info = mgr.timer_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0], "keepalive: interval=300.0s, repetitions=infinite");
    }

    #[test]
    fn timer_info_shows_finite() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("countdown", Duration::from_secs(5), 10, vec![], now);

        let info = mgr.timer_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0], "countdown: interval=5.0s, repetitions=10");
    }

    #[test]
    fn timer_info_sorted_by_name() {
        let mut mgr = TimerManager::new();
        let now = Instant::now();
        mgr.register("zebra", Duration::from_secs(10), 0, vec![], now);
        mgr.register("alpha", Duration::from_secs(5), 3, vec![], now);

        let info = mgr.timer_info();
        assert_eq!(info.len(), 2);
        assert!(info[0].starts_with("alpha:"));
        assert!(info[1].starts_with("zebra:"));
    }

    // ── execute_fired tests ────────────────────────────────────────────

    /// A command handler that records all commands for test assertions.
    struct TestCmdHandler {
        commands: Vec<(String, Vec<Value>)>,
    }

    impl TestCmdHandler {
        fn new() -> Self {
            Self {
                commands: Vec::new(),
            }
        }
    }

    impl CommandHandler for TestCmdHandler {
        fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
            self.commands.push((name.to_string(), args.to_vec()));
            Ok(())
        }
    }

    fn var_decl_stmt(name: &str, global: bool, value: i64) -> Statement {
        Statement::VarDecl(VarDeclStatement {
            name: name.to_string(),
            global,
            value: Expression::IntLiteral {
                value,
                span: span(),
            },
            span: span(),
        })
    }

    #[test]
    fn execute_fired_runs_body() {
        let fired = FiredTimer {
            name: "test".to_string(),
            body: vec![echo_stmt("timer fired")],
        };
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let ctx = BuiltinContext::new();
        let functions = FunctionRegistry::new();
        let regex_state = RegexState::new();

        let result =
            TimerManager::execute_fired(&fired, &mut env, &mut handler, &ctx, &functions, &regex_state);
        assert!(result.is_ok());
        assert_eq!(handler.commands.len(), 1);
        assert_eq!(handler.commands[0].0, "echo");
    }

    #[test]
    fn execute_fired_fresh_scope_isolates_locals() {
        let fired = FiredTimer {
            name: "test".to_string(),
            body: vec![var_decl_stmt("x", false, 42)],
        };
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let ctx = BuiltinContext::new();
        let functions = FunctionRegistry::new();
        let regex_state = RegexState::new();

        TimerManager::execute_fired(&fired, &mut env, &mut handler, &ctx, &functions, &regex_state)
            .unwrap();

        // Local variable should not leak to outer scope
        assert!(env.get_local("x").is_none());
    }

    #[test]
    fn execute_fired_can_set_globals() {
        let fired = FiredTimer {
            name: "test".to_string(),
            body: vec![var_decl_stmt("counter", true, 99)],
        };
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let ctx = BuiltinContext::new();
        let functions = FunctionRegistry::new();
        let regex_state = RegexState::new();

        TimerManager::execute_fired(&fired, &mut env, &mut handler, &ctx, &functions, &regex_state)
            .unwrap();

        // Global variable should persist
        assert_eq!(env.get_global("counter"), Some(Value::Int(99)));
    }

    #[test]
    fn execute_fired_catches_return() {
        let fired = FiredTimer {
            name: "test".to_string(),
            body: vec![
                echo_stmt("before"),
                Statement::Return(ReturnStatement {
                    value: None,
                    span: span(),
                }),
                echo_stmt("after"),
            ],
        };
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let ctx = BuiltinContext::new();
        let functions = FunctionRegistry::new();
        let regex_state = RegexState::new();

        let result =
            TimerManager::execute_fired(&fired, &mut env, &mut handler, &ctx, &functions, &regex_state);
        // Return should be caught as normal completion
        assert!(result.is_ok());
        // Only "before" should execute, "after" is skipped by return
        assert_eq!(handler.commands.len(), 1);
    }

    #[test]
    fn execute_fired_propagates_halt() {
        let fired = FiredTimer {
            name: "test".to_string(),
            body: vec![Statement::Command(CommandStatement {
                name: "halt".to_string(),
                args: vec![],
                span: span(),
            })],
        };
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let ctx = BuiltinContext::new();
        let functions = FunctionRegistry::new();
        let regex_state = RegexState::new();

        // Note: halt goes through the command handler, which won't intercept it
        // because execute_fired doesn't set aliases on the interpreter.
        // The halt command goes to the external handler.
        let result =
            TimerManager::execute_fired(&fired, &mut env, &mut handler, &ctx, &functions, &regex_state);
        // Without alias dispatch, halt goes to external handler
        assert!(result.is_ok());
    }

    // ── Built-in timer command tests ───────────────────────────────────

    #[test]
    fn builtin_timeoff_removes_timer() {
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let aliases = AliasRegistry::new();
        let mut echo_output = Vec::new();
        let mut tm = TimerManager::new();
        let now = Instant::now();
        tm.register("mytimer", Duration::from_secs(10), 0, vec![], now);

        let stmts = vec![Statement::Command(CommandStatement {
            name: "timeoff".to_string(),
            args: vec![Expression::StringLiteral {
                value: "mytimer".to_string(),
                span: span(),
            }],
            span: span(),
        })];

        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.set_aliases(&aliases);
        interp.set_echo_output(&mut echo_output);
        interp.set_timer_manager(&mut tm);
        interp.exec_stmts(&stmts).unwrap();

        assert!(tm.is_empty());
    }

    #[test]
    fn builtin_timer_remove_removes_timer() {
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let aliases = AliasRegistry::new();
        let mut echo_output = Vec::new();
        let mut tm = TimerManager::new();
        let now = Instant::now();
        tm.register("mytimer", Duration::from_secs(10), 0, vec![], now);

        // /timer -r mytimer
        let stmts = vec![Statement::Command(CommandStatement {
            name: "timer".to_string(),
            args: vec![
                Expression::Identifier {
                    name: "-r".to_string(),
                    span: span(),
                },
                Expression::StringLiteral {
                    value: "mytimer".to_string(),
                    span: span(),
                },
            ],
            span: span(),
        })];

        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.set_aliases(&aliases);
        interp.set_echo_output(&mut echo_output);
        interp.set_timer_manager(&mut tm);
        interp.exec_stmts(&stmts).unwrap();

        assert!(tm.is_empty());
    }

    #[test]
    fn builtin_timers_lists_active() {
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let aliases = AliasRegistry::new();
        let mut echo_output = Vec::new();
        let mut tm = TimerManager::new();
        let now = Instant::now();
        tm.register("keepalive", Duration::from_secs(300), 0, vec![], now);
        tm.register("reminder", Duration::from_secs(60), 5, vec![], now);

        let stmts = vec![Statement::Command(CommandStatement {
            name: "timers".to_string(),
            args: vec![],
            span: span(),
        })];

        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.set_aliases(&aliases);
        interp.set_echo_output(&mut echo_output);
        interp.set_timer_manager(&mut tm);
        interp.exec_stmts(&stmts).unwrap();

        assert_eq!(echo_output.len(), 2);
        // Output is sorted by name
        assert!(echo_output[0].starts_with("keepalive:"));
        assert!(echo_output[1].starts_with("reminder:"));
    }

    #[test]
    fn builtin_timers_empty_shows_message() {
        let mut env = Environment::new();
        let mut handler = TestCmdHandler::new();
        let aliases = AliasRegistry::new();
        let mut echo_output = Vec::new();
        let mut tm = TimerManager::new();

        let stmts = vec![Statement::Command(CommandStatement {
            name: "timers".to_string(),
            args: vec![],
            span: span(),
        })];

        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.set_aliases(&aliases);
        interp.set_echo_output(&mut echo_output);
        interp.set_timer_manager(&mut tm);
        interp.exec_stmts(&stmts).unwrap();

        assert_eq!(echo_output, vec!["No active timers"]);
    }
}
