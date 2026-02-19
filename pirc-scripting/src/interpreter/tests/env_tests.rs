use super::helpers::*;
use crate::ast::{self, Statement};
use crate::interpreter::environment::Environment;
use crate::interpreter::{Interpreter, Value};

#[test]
fn env_local_scope_shadowing() {
    let mut env = Environment::new();
    env.set_local("x", Value::Int(1));

    env.push_scope();
    env.set_local("x", Value::Int(2));
    assert_eq!(env.get_local("x"), Some(Value::Int(2)));

    env.pop_scope();
    assert_eq!(env.get_local("x"), Some(Value::Int(1)));
}

#[test]
fn env_update_local_finds_outer_scope() {
    let mut env = Environment::new();
    env.set_local("x", Value::Int(1));

    env.push_scope();
    // update_local should find x in the outer scope
    env.update_local("x", Value::Int(99));
    assert_eq!(env.get_local("x"), Some(Value::Int(99)));

    env.pop_scope();
    // After pop, the update should still be in the outer scope
    assert_eq!(env.get_local("x"), Some(Value::Int(99)));
}

#[test]
fn env_globals_independent_of_scopes() {
    let mut env = Environment::new();
    env.set_global("g", Value::String("hello".into()));

    env.push_scope();
    assert_eq!(
        env.get_global("g"),
        Some(Value::String("hello".into()))
    );
    env.set_global("g", Value::String("updated".into()));
    env.pop_scope();

    assert_eq!(
        env.get_global("g"),
        Some(Value::String("updated".into()))
    );
}

#[test]
fn env_get_local_returns_none_for_missing() {
    let env = Environment::new();
    assert_eq!(env.get_local("nonexistent"), None);
}

#[test]
fn env_get_global_returns_none_for_missing() {
    let env = Environment::new();
    assert_eq!(env.get_global("nonexistent"), None);
}

#[test]
#[should_panic(expected = "cannot pop the root scope")]
fn env_pop_root_scope_panics() {
    let mut env = Environment::new();
    env.pop_scope(); // Should panic: only root scope left
}

#[test]
fn env_default_trait() {
    let env = Environment::default();
    assert_eq!(env.get_local("x"), None);
}

// ── If scoping tests ──────────────────────────────────────────────

#[test]
fn exec_if_creates_scope() {
    // Variables declared inside if body should not leak to outer scope
    let stmts = vec![
        if_stmt(
            bool_expr(true),
            vec![var_decl("inner", false, int_expr(42))],
            vec![],
            None,
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("inner"), None);
}

// ── While scoping tests ────────────────────────────────────────────

#[test]
fn exec_while_creates_scope_per_iteration() {
    // Each iteration has its own scope
    let stmts = vec![
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(3)),
            vec![
                var_decl("inner", false, var_expr("i")),
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    // inner should not leak
    assert_eq!(env.get_local("inner"), None);
    assert_eq!(env.get_local("i"), Some(Value::Int(3)));
}
