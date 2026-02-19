use super::helpers::*;
use crate::interpreter::builtins::BuiltinContext;
use crate::interpreter::Value;

#[test]
fn builtin_static_constants() {
    assert_eq!(eval(&builtin_expr("null")).unwrap(), Value::Null);
    assert_eq!(eval(&builtin_expr("true")).unwrap(), Value::Bool(true));
    assert_eq!(eval(&builtin_expr("false")).unwrap(), Value::Bool(false));
    assert_eq!(
        eval(&builtin_expr("cr")).unwrap(),
        Value::String("\r".to_string())
    );
    assert_eq!(
        eval(&builtin_expr("lf")).unwrap(),
        Value::String("\n".to_string())
    );
    assert_eq!(
        eval(&builtin_expr("crlf")).unwrap(),
        Value::String("\r\n".to_string())
    );
    assert_eq!(
        eval(&builtin_expr("tab")).unwrap(),
        Value::String("\t".to_string())
    );
}

#[test]
fn builtin_context_identifiers() {
    let mut ctx = BuiltinContext::new();
    ctx.set("nick", Value::String("testuser".to_string()));
    ctx.set("chan", Value::String("#pirc".to_string()));

    assert_eq!(
        eval_with_context(&builtin_expr("nick"), &ctx).unwrap(),
        Value::String("testuser".to_string())
    );
    assert_eq!(
        eval_with_context(&builtin_expr("chan"), &ctx).unwrap(),
        Value::String("#pirc".to_string())
    );
}

#[test]
fn builtin_unknown_returns_null() {
    let ctx = BuiltinContext::new();
    assert_eq!(
        eval_with_context(&builtin_expr("unknown"), &ctx).unwrap(),
        Value::Null
    );
}

#[test]
fn builtin_numeric_params_full_line() {
    let mut ctx = BuiltinContext::new();
    ctx.set_event_text("hello world foo bar");

    assert_eq!(
        eval_with_context(&builtin_expr("0"), &ctx).unwrap(),
        Value::String("hello world foo bar".to_string())
    );
}

#[test]
fn builtin_numeric_params_tokens() {
    let mut ctx = BuiltinContext::new();
    ctx.set_event_text("hello world foo");

    assert_eq!(
        eval_with_context(&builtin_expr("1"), &ctx).unwrap(),
        Value::String("hello".to_string())
    );
    assert_eq!(
        eval_with_context(&builtin_expr("2"), &ctx).unwrap(),
        Value::String("world".to_string())
    );
    assert_eq!(
        eval_with_context(&builtin_expr("3"), &ctx).unwrap(),
        Value::String("foo".to_string())
    );
    // Out of range → Null
    assert_eq!(
        eval_with_context(&builtin_expr("4"), &ctx).unwrap(),
        Value::Null
    );
}

#[test]
fn builtin_numeric_params_no_event_text() {
    let ctx = BuiltinContext::new();
    assert_eq!(
        eval_with_context(&builtin_expr("0"), &ctx).unwrap(),
        Value::String(String::new())
    );
    assert_eq!(
        eval_with_context(&builtin_expr("1"), &ctx).unwrap(),
        Value::Null
    );
}

#[test]
fn builtin_event_text_sets_text_identifier() {
    let mut ctx = BuiltinContext::new();
    ctx.set_event_text("some message");
    assert_eq!(
        eval_with_context(&builtin_expr("text"), &ctx).unwrap(),
        Value::String("some message".to_string())
    );
}
