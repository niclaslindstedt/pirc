use super::helpers::*;
use crate::ast;
use crate::interpreter::{RuntimeError, Value};

#[test]
fn value_display() {
    assert_eq!(Value::String("hello".into()).to_string(), "hello");
    assert_eq!(Value::Int(42).to_string(), "42");
    assert_eq!(Value::Number(3.14).to_string(), "3.14");
    assert_eq!(Value::Bool(true).to_string(), "true");
    assert_eq!(Value::Bool(false).to_string(), "false");
    assert_eq!(Value::Null.to_string(), "");
}

#[test]
fn value_truthiness() {
    assert!(!Value::Null.is_truthy());
    assert!(!Value::Bool(false).is_truthy());
    assert!(Value::Bool(true).is_truthy());
    assert!(!Value::Int(0).is_truthy());
    assert!(Value::Int(1).is_truthy());
    assert!(Value::Int(-1).is_truthy());
    assert!(!Value::Number(0.0).is_truthy());
    assert!(Value::Number(0.1).is_truthy());
    assert!(!Value::String(String::new()).is_truthy());
    assert!(Value::String("hi".into()).is_truthy());
}

#[test]
fn value_equality() {
    // Same types
    assert!(Value::Int(5).equals(&Value::Int(5)));
    assert!(!Value::Int(5).equals(&Value::Int(6)));
    assert!(Value::Number(1.5).equals(&Value::Number(1.5)));
    assert!(Value::Bool(true).equals(&Value::Bool(true)));
    assert!(!Value::Bool(true).equals(&Value::Bool(false)));
    assert!(Value::Null.equals(&Value::Null));

    // Case-insensitive string comparison
    assert!(Value::String("Hello".into()).equals(&Value::String("hello".into())));
    assert!(!Value::String("Hello".into()).equals(&Value::String("world".into())));

    // Int/Number coercion
    assert!(Value::Int(5).equals(&Value::Number(5.0)));
    assert!(Value::Number(5.0).equals(&Value::Int(5)));

    // Different types are not equal
    assert!(!Value::Int(1).equals(&Value::Bool(true)));
    assert!(!Value::String("1".into()).equals(&Value::Int(1)));
    assert!(!Value::Null.equals(&Value::Int(0)));
}

#[test]
fn value_type_name() {
    assert_eq!(Value::String("x".into()).type_name(), "string");
    assert_eq!(Value::Int(0).type_name(), "int");
    assert_eq!(Value::Number(0.0).type_name(), "number");
    assert_eq!(Value::Bool(true).type_name(), "bool");
    assert_eq!(Value::Null.type_name(), "null");
}

// ── Arithmetic tests ───────────────────────────────────────────────────

#[test]
fn value_add_ints() {
    assert_eq!(Value::Int(3).add(&Value::Int(4)).unwrap(), Value::Int(7));
}

#[test]
fn value_add_floats() {
    assert_eq!(
        Value::Number(1.5).add(&Value::Number(2.5)).unwrap(),
        Value::Number(4.0)
    );
}

#[test]
fn value_add_int_float_coercion() {
    assert_eq!(
        Value::Int(3).add(&Value::Number(0.5)).unwrap(),
        Value::Number(3.5)
    );
    assert_eq!(
        Value::Number(0.5).add(&Value::Int(3)).unwrap(),
        Value::Number(3.5)
    );
}

#[test]
fn value_add_strings() {
    assert_eq!(
        Value::String("hello ".into())
            .add(&Value::String("world".into()))
            .unwrap(),
        Value::String("hello world".into())
    );
}

#[test]
fn value_add_type_error() {
    assert!(Value::String("x".into()).add(&Value::Int(1)).is_err());
    assert!(Value::Bool(true).add(&Value::Int(1)).is_err());
}

#[test]
fn value_sub() {
    assert_eq!(Value::Int(10).sub(&Value::Int(3)).unwrap(), Value::Int(7));
    assert_eq!(
        Value::Number(5.5).sub(&Value::Number(2.0)).unwrap(),
        Value::Number(3.5)
    );
    assert!(Value::String("x".into()).sub(&Value::Int(1)).is_err());
}

#[test]
fn value_mul() {
    assert_eq!(Value::Int(3).mul(&Value::Int(4)).unwrap(), Value::Int(12));
    assert_eq!(
        Value::Number(2.0).mul(&Value::Number(3.5)).unwrap(),
        Value::Number(7.0)
    );
}

#[test]
fn value_div() {
    assert_eq!(Value::Int(10).div(&Value::Int(3)).unwrap(), Value::Int(3));
    assert_eq!(
        Value::Number(10.0).div(&Value::Number(4.0)).unwrap(),
        Value::Number(2.5)
    );
}

#[test]
fn value_div_by_zero() {
    assert!(matches!(
        Value::Int(1).div(&Value::Int(0)),
        Err(RuntimeError::DivisionByZero)
    ));
    assert!(matches!(
        Value::Number(1.0).div(&Value::Number(0.0)),
        Err(RuntimeError::DivisionByZero)
    ));
    assert!(matches!(
        Value::Int(1).div(&Value::Number(0.0)),
        Err(RuntimeError::DivisionByZero)
    ));
    assert!(matches!(
        Value::Number(1.0).div(&Value::Int(0)),
        Err(RuntimeError::DivisionByZero)
    ));
}

#[test]
fn value_modulo() {
    assert_eq!(
        Value::Int(10).modulo(&Value::Int(3)).unwrap(),
        Value::Int(1)
    );
    assert!(matches!(
        Value::Int(10).modulo(&Value::Int(0)),
        Err(RuntimeError::DivisionByZero)
    ));
}

#[test]
fn value_negate() {
    assert_eq!(Value::Int(5).negate().unwrap(), Value::Int(-5));
    assert_eq!(Value::Number(3.14).negate().unwrap(), Value::Number(-3.14));
    assert!(Value::String("x".into()).negate().is_err());
    assert!(Value::Bool(true).negate().is_err());
    assert!(Value::Null.negate().is_err());
}

#[test]
fn value_comparison() {
    use std::cmp::Ordering;

    assert_eq!(
        Value::Int(3).compare(&Value::Int(5), Ordering::Less).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        Value::Int(5).compare(&Value::Int(3), Ordering::Greater).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        Value::Int(5).compare_lte(&Value::Int(5)).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        Value::Int(5).compare_gte(&Value::Int(5)).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        Value::Int(3).compare_gte(&Value::Int(5)).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn value_string_comparison() {
    use std::cmp::Ordering;

    // Case-insensitive
    assert_eq!(
        Value::String("apple".into())
            .compare(&Value::String("Banana".into()), Ordering::Less)
            .unwrap(),
        Value::Bool(true)
    );
}

// ── RuntimeError Display tests ──────────────────────────────────────

#[test]
fn runtime_error_display() {
    assert_eq!(
        RuntimeError::DivisionByZero.to_string(),
        "division by zero"
    );
    assert_eq!(
        RuntimeError::UndefinedVariable("%x".into()).to_string(),
        "undefined variable: %x"
    );
    assert_eq!(
        RuntimeError::UnknownBuiltin("foo".into()).to_string(),
        "unknown builtin: $foo"
    );
    assert_eq!(
        RuntimeError::UnknownFunction("bar".into()).to_string(),
        "unknown function: $bar"
    );
    assert_eq!(
        RuntimeError::UnknownCommand("xyz".into()).to_string(),
        "unknown command: xyz"
    );
    assert_eq!(
        RuntimeError::Break.to_string(),
        "break outside of loop"
    );
    assert_eq!(
        RuntimeError::Continue.to_string(),
        "continue outside of loop"
    );
}
