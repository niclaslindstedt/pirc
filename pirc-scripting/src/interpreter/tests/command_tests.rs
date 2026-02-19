use super::helpers::*;
use crate::ast::AliasDefinition;
use crate::interpreter::command::AliasRegistry;
use crate::interpreter::environment::Environment;
use crate::interpreter::functions::{FunctionRegistry, RegexState};
use crate::interpreter::{CommandHandler, Interpreter, RuntimeError, Value};

// ── AliasRegistry tests ────────────────────────────────────────────────

#[test]
fn registry_new_is_empty() {
    let reg = AliasRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_register_and_lookup() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hello")])],
        span: span(),
    };
    reg.register(&alias);
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
    assert!(reg.get("greet").is_some());
}

#[test]
fn registry_case_insensitive_lookup() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "Greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hello")])],
        span: span(),
    };
    reg.register(&alias);

    assert!(reg.get("greet").is_some());
    assert!(reg.get("GREET").is_some());
    assert!(reg.get("Greet").is_some());
    assert!(reg.get("gReEt").is_some());
}

#[test]
fn registry_contains() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![],
        span: span(),
    };
    reg.register(&alias);

    assert!(reg.contains("test"));
    assert!(reg.contains("TEST"));
    assert!(!reg.contains("other"));
}

#[test]
fn registry_unknown_alias_returns_none() {
    let reg = AliasRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_overwrite_existing_alias() {
    let mut reg = AliasRegistry::new();
    let alias1 = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hello")])],
        span: span(),
    };
    let alias2 = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("goodbye")])],
        span: span(),
    };
    reg.register(&alias1);
    reg.register(&alias2);

    assert_eq!(reg.len(), 1);
    let body = reg.get("greet").unwrap();
    assert_eq!(body.len(), 1);
}

#[test]
fn registry_multiple_aliases() {
    let mut reg = AliasRegistry::new();
    for name in &["one", "two", "three"] {
        let alias = AliasDefinition {
            name: (*name).to_string(),
            body: vec![],
            span: span(),
        };
        reg.register(&alias);
    }
    assert_eq!(reg.len(), 3);
    assert!(reg.contains("one"));
    assert!(reg.contains("two"));
    assert!(reg.contains("three"));
}

// ── Helper: create interpreter with alias dispatch ─────────────────────

fn make_alias_interp<'a>(
    env: &'a mut Environment,
    handler: &'a mut dyn CommandHandler,
    aliases: &'a AliasRegistry,
    echo_output: &'a mut Vec<String>,
) -> Interpreter<'a> {
    let mut interp = Interpreter::new(env, handler);
    interp.set_aliases(aliases);
    interp.set_echo_output(echo_output);
    interp
}

fn make_full_alias_interp<'a>(
    env: &'a mut Environment,
    handler: &'a mut dyn CommandHandler,
    aliases: &'a AliasRegistry,
    echo_output: &'a mut Vec<String>,
    ctx: &'a crate::interpreter::builtins::BuiltinContext,
    functions: &'a FunctionRegistry,
    regex_state: &'a RegexState,
) -> Interpreter<'a> {
    let mut interp = Interpreter::with_context(env, handler, ctx, functions, regex_state);
    interp.set_aliases(aliases);
    interp.set_echo_output(echo_output);
    interp
}

// ── Built-in command tests ─────────────────────────────────────────────

#[test]
fn builtin_echo_captures_output() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let aliases = AliasRegistry::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("echo", vec![str_expr("hello"), str_expr("world")])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &aliases, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["hello world"]);
    // Should NOT go to external handler
    assert!(handler.commands.is_empty());
}

#[test]
fn builtin_echo_case_insensitive() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let aliases = AliasRegistry::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("ECHO", vec![str_expr("test")])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &aliases, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["test"]);
}

#[test]
fn builtin_echo_no_args() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let aliases = AliasRegistry::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("echo", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &aliases, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec![""]);
}

#[test]
fn builtin_noop_does_nothing() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let aliases = AliasRegistry::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("noop", vec![str_expr("ignored")])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &aliases, &mut echo_output);
    let result = interp.exec_stmts(&stmts);

    assert!(result.is_ok());
    assert!(handler.commands.is_empty());
    assert!(echo_output.is_empty());
}

#[test]
fn builtin_halt_stops_execution() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let aliases = AliasRegistry::new();
    let mut echo_output = Vec::new();

    let stmts = vec![
        cmd_stmt("echo", vec![str_expr("before")]),
        cmd_stmt("halt", vec![]),
        cmd_stmt("echo", vec![str_expr("after")]),
    ];
    let mut interp = make_alias_interp(&mut env, &mut handler, &aliases, &mut echo_output);
    let result = interp.exec_stmts(&stmts);

    assert!(matches!(result, Err(RuntimeError::Halt)));
    assert_eq!(echo_output, vec!["before"]);
}

// ── Command dispatch order tests ───────────────────────────────────────

#[test]
fn dispatch_builtin_takes_precedence_over_alias() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "echo".to_string(),
        body: vec![cmd_stmt("msg", vec![str_expr("from alias")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("echo", vec![str_expr("builtin")])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    // Built-in echo should be used, not the alias
    assert_eq!(echo_output, vec!["builtin"]);
    assert!(handler.commands.is_empty());
}

#[test]
fn dispatch_alias_takes_precedence_over_external() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("from alias")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("greet", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    // Alias should run, not external handler
    assert_eq!(echo_output, vec!["from alias"]);
    assert!(handler.commands.is_empty());
}

#[test]
fn dispatch_unknown_goes_to_external_handler() {
    let reg = AliasRegistry::new();
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("msg", vec![str_expr("#chan"), str_expr("hello")])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(handler.commands.len(), 1);
    assert_eq!(handler.commands[0].0, "msg");
}

// ── Alias execution tests ──────────────────────────────────────────────

#[test]
fn alias_basic_execution() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hello world")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("greet", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["hello world"]);
}

#[test]
fn alias_arguments_populate_dollar_zero() {
    let mut reg = AliasRegistry::new();
    // Alias body: echo $0
    let alias = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("echo", vec![builtin_expr("0")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("greet", vec![str_expr("alice"), str_expr("bob")])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    // $0 should be all args joined
    assert_eq!(echo_output, vec!["alice bob"]);
}

#[test]
fn alias_arguments_populate_dollar_1_through_9() {
    let mut reg = AliasRegistry::new();
    // Alias body: echo $1 $2 $3
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![cmd_stmt(
            "echo",
            vec![builtin_expr("1"), builtin_expr("2"), builtin_expr("3")],
        )],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt(
        "test",
        vec![str_expr("alpha"), str_expr("beta"), str_expr("gamma")],
    )];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["alpha beta gamma"]);
}

#[test]
fn alias_missing_args_resolve_to_null() {
    let mut reg = AliasRegistry::new();
    // Alias body: echo $5 (only 2 args passed)
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![cmd_stmt("echo", vec![builtin_expr("5")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("test", vec![str_expr("a"), str_expr("b")])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    // $5 doesn't exist, resolves to Null which displays as ""
    assert_eq!(echo_output, vec![""]);
}

#[test]
fn alias_scope_isolation() {
    let mut reg = AliasRegistry::new();
    // Alias body: var %x = 42
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![var_decl("x", false, int_expr(42))],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("test", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    // Variable from alias should not leak to caller
    assert!(env.get_local("x").is_none());
}

#[test]
fn alias_can_set_globals() {
    let mut reg = AliasRegistry::new();
    // Alias body: var %%count = 42
    let alias = AliasDefinition {
        name: "setglobal".to_string(),
        body: vec![var_decl("count", true, int_expr(42))],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("setglobal", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    // Global should be visible after alias completes
    assert_eq!(env.get_global("count"), Some(Value::Int(42)));
}

#[test]
fn alias_return_is_caught() {
    let mut reg = AliasRegistry::new();
    // Alias body: echo "before"; return; echo "after"
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![
            cmd_stmt("echo", vec![str_expr("before")]),
            return_stmt(None),
            cmd_stmt("echo", vec![str_expr("after")]),
        ],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![
        cmd_stmt("test", vec![]),
        cmd_stmt("echo", vec![str_expr("continued")]),
    ];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    // Return should stop alias but not caller
    assert_eq!(echo_output, vec!["before", "continued"]);
}

// ── Alias-calls-alias (recursion) tests ────────────────────────────────

#[test]
fn alias_calls_another_alias() {
    let mut reg = AliasRegistry::new();

    // inner: echo "inner"
    let inner = AliasDefinition {
        name: "inner".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("inner")])],
        span: span(),
    };
    reg.register(&inner);

    // outer: echo "before"; inner; echo "after"
    let outer = AliasDefinition {
        name: "outer".to_string(),
        body: vec![
            cmd_stmt("echo", vec![str_expr("before")]),
            cmd_stmt("inner", vec![]),
            cmd_stmt("echo", vec![str_expr("after")]),
        ],
        span: span(),
    };
    reg.register(&outer);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("outer", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["before", "inner", "after"]);
}

#[test]
fn alias_passes_args_to_inner_alias() {
    let mut reg = AliasRegistry::new();

    // inner: echo $1
    let inner = AliasDefinition {
        name: "inner".to_string(),
        body: vec![cmd_stmt("echo", vec![builtin_expr("1")])],
        span: span(),
    };
    reg.register(&inner);

    // outer: inner "hello"
    let outer = AliasDefinition {
        name: "outer".to_string(),
        body: vec![cmd_stmt("inner", vec![str_expr("hello")])],
        span: span(),
    };
    reg.register(&outer);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("outer", vec![])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["hello"]);
}

#[test]
fn alias_recursion_limit() {
    let mut reg = AliasRegistry::new();

    // recursive: recursive (calls itself forever)
    let alias = AliasDefinition {
        name: "recursive".to_string(),
        body: vec![cmd_stmt("recursive", vec![])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("recursive", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    let result = interp.exec_stmts(&stmts);

    assert!(matches!(result, Err(RuntimeError::AliasRecursionLimit)));
}

#[test]
fn alias_mutual_recursion_limit() {
    let mut reg = AliasRegistry::new();

    // ping_alias: pong_alias
    let ping_alias = AliasDefinition {
        name: "ping".to_string(),
        body: vec![cmd_stmt("pong", vec![])],
        span: span(),
    };
    reg.register(&ping_alias);

    // pong_alias: ping_alias
    let pong_alias = AliasDefinition {
        name: "pong".to_string(),
        body: vec![cmd_stmt("ping", vec![])],
        span: span(),
    };
    reg.register(&pong_alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("ping", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    let result = interp.exec_stmts(&stmts);

    assert!(matches!(result, Err(RuntimeError::AliasRecursionLimit)));
}

#[test]
fn alias_calls_external_command() {
    let mut reg = AliasRegistry::new();

    // greet: msg $1 "Hello!"
    let alias = AliasDefinition {
        name: "greet".to_string(),
        body: vec![cmd_stmt("msg", vec![builtin_expr("1"), str_expr("Hello!")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("greet", vec![str_expr("#channel")])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(handler.commands.len(), 1);
    assert_eq!(handler.commands[0].0, "msg");
    assert_eq!(
        handler.commands[0].1,
        vec![
            Value::String("#channel".to_string()),
            Value::String("Hello!".to_string()),
        ]
    );
}

#[test]
fn alias_halt_propagates_through_alias() {
    let mut reg = AliasRegistry::new();

    // stopper: halt
    let alias = AliasDefinition {
        name: "stopper".to_string(),
        body: vec![cmd_stmt("halt", vec![])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![
        cmd_stmt("echo", vec![str_expr("before")]),
        cmd_stmt("stopper", vec![]),
        cmd_stmt("echo", vec![str_expr("after")]),
    ];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    let result = interp.exec_stmts(&stmts);

    assert!(matches!(result, Err(RuntimeError::Halt)));
    assert_eq!(echo_output, vec!["before"]);
}

#[test]
fn alias_case_insensitive_invocation() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "Greet".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hi")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    // Invoke with different case
    let stmts = vec![cmd_stmt("greet", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["hi"]);
}

#[test]
fn alias_with_control_flow() {
    let mut reg = AliasRegistry::new();
    // Alias that uses if/else based on arg
    // alias check { if ($1 == "yes") { echo "positive" } else { echo "negative" } }
    let alias = AliasDefinition {
        name: "check".to_string(),
        body: vec![if_stmt(
            binary(
                builtin_expr("1"),
                crate::ast::BinaryOp::Eq,
                str_expr("yes"),
            ),
            vec![cmd_stmt("echo", vec![str_expr("positive")])],
            vec![],
            Some(vec![cmd_stmt("echo", vec![str_expr("negative")])]),
        )],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    // Test positive case
    let stmts = vec![cmd_stmt("check", vec![str_expr("yes")])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(echo_output, vec!["positive"]);

    // Test negative case
    echo_output.clear();
    let stmts = vec![cmd_stmt("check", vec![str_expr("no")])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(echo_output, vec!["negative"]);
}

#[test]
fn alias_nested_scope_isolation() {
    let mut reg = AliasRegistry::new();

    // inner: var %x = "inner"
    let inner = AliasDefinition {
        name: "inner".to_string(),
        body: vec![var_decl("x", false, str_expr("inner"))],
        span: span(),
    };
    reg.register(&inner);

    // outer: var %x = "outer"; inner; echo %x
    // After inner returns, %x should still be "outer"
    let outer = AliasDefinition {
        name: "outer".to_string(),
        body: vec![
            var_decl("x", false, str_expr("outer")),
            cmd_stmt("inner", vec![]),
            cmd_stmt("echo", vec![var_expr("x")]),
        ],
        span: span(),
    };
    reg.register(&outer);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    let stmts = vec![cmd_stmt("outer", vec![])];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["outer"]);
}

// ── Backward compatibility tests ───────────────────────────────────────

#[test]
fn no_aliases_commands_go_to_handler() {
    // Without aliases configured, commands should go directly to handler
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![cmd_stmt("echo", vec![str_expr("test")])];

    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();

    // "echo" should go to handler, not be intercepted as builtin
    assert_eq!(handler.commands.len(), 1);
    assert_eq!(handler.commands[0].0, "echo");
}

#[test]
fn alias_depth_tracking() {
    // Verify that alias depth resets properly after alias execution
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![cmd_stmt("echo", vec![str_expr("hi")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();

    // Call alias twice — depth should reset between calls
    let stmts = vec![
        cmd_stmt("test", vec![]),
        cmd_stmt("test", vec![]),
    ];
    let mut interp = make_alias_interp(&mut env, &mut handler, &reg, &mut echo_output);
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["hi", "hi"]);
}

#[test]
fn alias_context_inherits_outer_builtins() {
    let mut reg = AliasRegistry::new();
    // Alias body: echo $nick (should inherit from outer context)
    let alias = AliasDefinition {
        name: "whoami".to_string(),
        body: vec![cmd_stmt("echo", vec![builtin_expr("nick")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let mut ctx = crate::interpreter::builtins::BuiltinContext::new();
    ctx.set("nick", Value::String("alice".to_string()));
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("whoami", vec![])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    assert_eq!(echo_output, vec!["alice"]);
}

#[test]
fn alias_no_args_dollar_zero_is_empty() {
    let mut reg = AliasRegistry::new();
    let alias = AliasDefinition {
        name: "test".to_string(),
        body: vec![cmd_stmt("echo", vec![builtin_expr("0")])],
        span: span(),
    };
    reg.register(&alias);

    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut echo_output = Vec::new();
    let ctx = crate::interpreter::builtins::BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let stmts = vec![cmd_stmt("test", vec![])];
    let mut interp = make_full_alias_interp(
        &mut env,
        &mut handler,
        &reg,
        &mut echo_output,
        &ctx,
        &functions,
        &regex_state,
    );
    interp.exec_stmts(&stmts).unwrap();

    // $0 with no args should be empty string
    assert_eq!(echo_output, vec![""]);
}
