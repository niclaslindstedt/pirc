use super::helpers::*;
use crate::interpreter::builtins::BuiltinContext;
use crate::interpreter::functions::{FunctionRegistry, RegexState};
use crate::interpreter::{Interpreter, RuntimeError, Value};

#[test]
fn fn_len_string() {
    assert_eq!(
        eval(&func_call("len", vec![str_expr("hello")])).unwrap(),
        Value::Int(5)
    );
}

#[test]
fn fn_len_empty() {
    assert_eq!(
        eval(&func_call("len", vec![str_expr("")])).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn fn_left_basic() {
    assert_eq!(
        eval(&func_call("left", vec![str_expr("hello"), int_expr(3)])).unwrap(),
        Value::String("hel".to_string())
    );
}

#[test]
fn fn_left_exceeds_length() {
    assert_eq!(
        eval(&func_call("left", vec![str_expr("hi"), int_expr(10)])).unwrap(),
        Value::String("hi".to_string())
    );
}

#[test]
fn fn_right_basic() {
    assert_eq!(
        eval(&func_call("right", vec![str_expr("hello"), int_expr(3)])).unwrap(),
        Value::String("llo".to_string())
    );
}

#[test]
fn fn_right_exceeds_length() {
    assert_eq!(
        eval(&func_call("right", vec![str_expr("hi"), int_expr(10)])).unwrap(),
        Value::String("hi".to_string())
    );
}

#[test]
fn fn_mid_basic() {
    assert_eq!(
        eval(&func_call(
            "mid",
            vec![str_expr("hello world"), int_expr(6), int_expr(5)]
        ))
        .unwrap(),
        Value::String("world".to_string())
    );
}

#[test]
fn fn_mid_from_start() {
    assert_eq!(
        eval(&func_call(
            "mid",
            vec![str_expr("hello"), int_expr(0), int_expr(3)]
        ))
        .unwrap(),
        Value::String("hel".to_string())
    );
}

#[test]
fn fn_upper() {
    assert_eq!(
        eval(&func_call("upper", vec![str_expr("hello")])).unwrap(),
        Value::String("HELLO".to_string())
    );
}

#[test]
fn fn_lower() {
    assert_eq!(
        eval(&func_call("lower", vec![str_expr("HELLO")])).unwrap(),
        Value::String("hello".to_string())
    );
}

#[test]
fn fn_replace_basic() {
    assert_eq!(
        eval(&func_call(
            "replace",
            vec![str_expr("hello world"), str_expr("world"), str_expr("pirc")]
        ))
        .unwrap(),
        Value::String("hello pirc".to_string())
    );
}

#[test]
fn fn_replace_multiple_occurrences() {
    assert_eq!(
        eval(&func_call(
            "replace",
            vec![str_expr("aabaa"), str_expr("a"), str_expr("x")]
        ))
        .unwrap(),
        Value::String("xxbxx".to_string())
    );
}

#[test]
fn fn_find_found() {
    assert_eq!(
        eval(&func_call(
            "find",
            vec![str_expr("hello world"), str_expr("world")]
        ))
        .unwrap(),
        Value::Int(6)
    );
}

#[test]
fn fn_find_not_found() {
    assert_eq!(
        eval(&func_call(
            "find",
            vec![str_expr("hello"), str_expr("xyz")]
        ))
        .unwrap(),
        Value::Int(-1)
    );
}

#[test]
fn fn_find_at_start() {
    assert_eq!(
        eval(&func_call(
            "find",
            vec![str_expr("hello"), str_expr("hel")]
        ))
        .unwrap(),
        Value::Int(0)
    );
}

#[test]
fn fn_token_basic() {
    assert_eq!(
        eval(&func_call(
            "token",
            vec![str_expr("a,b,c"), int_expr(2), str_expr(",")]
        ))
        .unwrap(),
        Value::String("b".to_string())
    );
}

#[test]
fn fn_token_first() {
    assert_eq!(
        eval(&func_call(
            "token",
            vec![str_expr("one two three"), int_expr(1), str_expr(" ")]
        ))
        .unwrap(),
        Value::String("one".to_string())
    );
}

#[test]
fn fn_token_out_of_range() {
    assert_eq!(
        eval(&func_call(
            "token",
            vec![str_expr("a,b"), int_expr(5), str_expr(",")]
        ))
        .unwrap(),
        Value::Null
    );
}

#[test]
fn fn_token_zero_index() {
    assert_eq!(
        eval(&func_call(
            "token",
            vec![str_expr("a,b"), int_expr(0), str_expr(",")]
        ))
        .unwrap(),
        Value::Null
    );
}

#[test]
fn fn_numtok_basic() {
    assert_eq!(
        eval(&func_call(
            "numtok",
            vec![str_expr("a,b,c"), str_expr(",")]
        ))
        .unwrap(),
        Value::Int(3)
    );
}

#[test]
fn fn_numtok_empty_string() {
    assert_eq!(
        eval(&func_call("numtok", vec![str_expr(""), str_expr(",")])).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn fn_strip_whitespace() {
    assert_eq!(
        eval(&func_call("strip", vec![str_expr("  hello  ")])).unwrap(),
        Value::String("hello".to_string())
    );
}

#[test]
fn fn_strip_no_whitespace() {
    assert_eq!(
        eval(&func_call("strip", vec![str_expr("hello")])).unwrap(),
        Value::String("hello".to_string())
    );
}

#[test]
fn fn_chr_basic() {
    assert_eq!(
        eval(&func_call("chr", vec![int_expr(65)])).unwrap(),
        Value::String("A".to_string())
    );
}

#[test]
fn fn_chr_invalid() {
    assert!(eval(&func_call("chr", vec![int_expr(-1)])).is_err());
}

#[test]
fn fn_asc_basic() {
    assert_eq!(
        eval(&func_call("asc", vec![str_expr("A")])).unwrap(),
        Value::Int(65)
    );
}

#[test]
fn fn_asc_first_char() {
    assert_eq!(
        eval(&func_call("asc", vec![str_expr("hello")])).unwrap(),
        Value::Int(104)
    );
}

#[test]
fn fn_asc_empty() {
    assert!(eval(&func_call("asc", vec![str_expr("")])).is_err());
}

// ── Function arity errors ──────────────────────────────────────────────

#[test]
fn fn_arity_too_few_args() {
    let result = eval(&func_call("len", vec![]));
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
}

#[test]
fn fn_arity_too_many_args() {
    let result = eval(&func_call("len", vec![str_expr("a"), str_expr("b")]));
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
}

#[test]
fn fn_unknown_function() {
    let result = eval(&func_call("doesnotexist", vec![str_expr("a")]));
    assert!(matches!(result, Err(RuntimeError::UnknownFunction(_))));
}

// ── with_context constructor ───────────────────────────────────────────

#[test]
fn with_context_constructor_works() {
    let mut env = crate::interpreter::environment::Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut ctx = BuiltinContext::new();
    ctx.set("nick", Value::String("test".to_string()));
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();

    let mut interp =
        Interpreter::with_context(&mut env, &mut handler, &ctx, &functions, &regex_state);

    let result = interp.eval_expr(&builtin_expr("nick")).unwrap();
    assert_eq!(result, Value::String("test".to_string()));

    let result = interp
        .eval_expr(&func_call("len", vec![str_expr("hello")]))
        .unwrap();
    assert_eq!(result, Value::Int(5));
}
