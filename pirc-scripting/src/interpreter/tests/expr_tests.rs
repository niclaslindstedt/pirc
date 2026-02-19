use super::helpers::*;
use crate::ast::{self, Expression, StringPart};
use crate::interpreter::environment::Environment;
use crate::interpreter::{CommandHandler, Interpreter, RuntimeError, Value};

#[test]
fn eval_literals() {
    assert_eq!(eval(&int_expr(42)).unwrap(), Value::Int(42));
    assert_eq!(eval(&num_expr(3.14)).unwrap(), Value::Number(3.14));
    assert_eq!(
        eval(&str_expr("hello")).unwrap(),
        Value::String("hello".into())
    );
    assert_eq!(eval(&bool_expr(true)).unwrap(), Value::Bool(true));
    assert_eq!(eval(&bool_expr(false)).unwrap(), Value::Bool(false));
}

#[test]
fn eval_identifier_as_string() {
    assert_eq!(
        eval(&ident_expr("foo")).unwrap(),
        Value::String("foo".into())
    );
}

#[test]
fn eval_builtin_constants() {
    assert_eq!(eval(&builtin_expr("null")).unwrap(), Value::Null);
    assert_eq!(eval(&builtin_expr("true")).unwrap(), Value::Bool(true));
    assert_eq!(eval(&builtin_expr("false")).unwrap(), Value::Bool(false));
    assert_eq!(
        eval(&builtin_expr("cr")).unwrap(),
        Value::String("\r".into())
    );
    assert_eq!(
        eval(&builtin_expr("lf")).unwrap(),
        Value::String("\n".into())
    );
    assert_eq!(
        eval(&builtin_expr("crlf")).unwrap(),
        Value::String("\r\n".into())
    );
    assert_eq!(
        eval(&builtin_expr("tab")).unwrap(),
        Value::String("\t".into())
    );
    // Unknown builtins return Null as stub
    assert_eq!(eval(&builtin_expr("nick")).unwrap(), Value::Null);
}

#[test]
fn eval_grouped() {
    let expr = grouped(int_expr(42));
    assert_eq!(eval(&expr).unwrap(), Value::Int(42));
}

#[test]
fn eval_binary_arithmetic() {
    let add = binary(int_expr(3), ast::BinaryOp::Add, int_expr(4));
    assert_eq!(eval(&add).unwrap(), Value::Int(7));

    let sub = binary(int_expr(10), ast::BinaryOp::Sub, int_expr(3));
    assert_eq!(eval(&sub).unwrap(), Value::Int(7));

    let mul = binary(int_expr(3), ast::BinaryOp::Mul, int_expr(4));
    assert_eq!(eval(&mul).unwrap(), Value::Int(12));

    let div = binary(int_expr(10), ast::BinaryOp::Div, int_expr(3));
    assert_eq!(eval(&div).unwrap(), Value::Int(3));

    let modulo = binary(int_expr(10), ast::BinaryOp::Mod, int_expr(3));
    assert_eq!(eval(&modulo).unwrap(), Value::Int(1));
}

#[test]
fn eval_binary_comparison() {
    let eq = binary(int_expr(5), ast::BinaryOp::Eq, int_expr(5));
    assert_eq!(eval(&eq).unwrap(), Value::Bool(true));

    let neq = binary(int_expr(5), ast::BinaryOp::Neq, int_expr(3));
    assert_eq!(eval(&neq).unwrap(), Value::Bool(true));

    let lt = binary(int_expr(3), ast::BinaryOp::Lt, int_expr(5));
    assert_eq!(eval(&lt).unwrap(), Value::Bool(true));

    let gt = binary(int_expr(5), ast::BinaryOp::Gt, int_expr(3));
    assert_eq!(eval(&gt).unwrap(), Value::Bool(true));

    let lte = binary(int_expr(5), ast::BinaryOp::Lte, int_expr(5));
    assert_eq!(eval(&lte).unwrap(), Value::Bool(true));

    let gte = binary(int_expr(5), ast::BinaryOp::Gte, int_expr(5));
    assert_eq!(eval(&gte).unwrap(), Value::Bool(true));
}

#[test]
fn eval_binary_logical() {
    // AND short-circuit: false && ... => false (right side not evaluated)
    let and_false = binary(bool_expr(false), ast::BinaryOp::And, bool_expr(true));
    assert_eq!(eval(&and_false).unwrap(), Value::Bool(false));

    let and_true = binary(bool_expr(true), ast::BinaryOp::And, bool_expr(true));
    assert_eq!(eval(&and_true).unwrap(), Value::Bool(true));

    // OR short-circuit: true || ... => true
    let or_true = binary(bool_expr(true), ast::BinaryOp::Or, bool_expr(false));
    assert_eq!(eval(&or_true).unwrap(), Value::Bool(true));

    let or_false = binary(bool_expr(false), ast::BinaryOp::Or, bool_expr(false));
    assert_eq!(eval(&or_false).unwrap(), Value::Bool(false));
}

#[test]
fn eval_unary_not() {
    let not_true = unary(ast::UnaryOp::Not, bool_expr(true));
    assert_eq!(eval(&not_true).unwrap(), Value::Bool(false));

    let not_false = unary(ast::UnaryOp::Not, bool_expr(false));
    assert_eq!(eval(&not_false).unwrap(), Value::Bool(true));

    // Not on truthy int
    let not_int = unary(ast::UnaryOp::Not, int_expr(0));
    assert_eq!(eval(&not_int).unwrap(), Value::Bool(true));
}

#[test]
fn eval_unary_neg() {
    let neg_int = unary(ast::UnaryOp::Neg, int_expr(5));
    assert_eq!(eval(&neg_int).unwrap(), Value::Int(-5));

    let neg_float = unary(ast::UnaryOp::Neg, num_expr(3.14));
    assert_eq!(eval(&neg_float).unwrap(), Value::Number(-3.14));
}

#[test]
fn eval_unary_neg_type_error() {
    let neg_str = unary(ast::UnaryOp::Neg, str_expr("hello"));
    assert!(matches!(eval(&neg_str), Err(RuntimeError::TypeError(_))));
}

#[test]
fn eval_string_concat() {
    let concat = binary(
        str_expr("hello "),
        ast::BinaryOp::Add,
        str_expr("world"),
    );
    assert_eq!(
        eval(&concat).unwrap(),
        Value::String("hello world".into())
    );
}

#[test]
fn eval_division_by_zero() {
    let div_zero = binary(int_expr(5), ast::BinaryOp::Div, int_expr(0));
    assert!(matches!(
        eval(&div_zero),
        Err(RuntimeError::DivisionByZero)
    ));
}

#[test]
fn eval_interpolated_string() {
    let mut env = Environment::new();
    env.set_local("name", Value::String("world".into()));
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);

    let expr = Expression::Interpolated {
        parts: vec![
            StringPart::Literal("hello ".to_string()),
            StringPart::Expr(var_expr("name")),
            StringPart::Literal("!".to_string()),
        ],
        span: span(),
    };
    assert_eq!(
        interp.eval_expr(&expr).unwrap(),
        Value::String("hello world!".into())
    );
}

#[test]
fn eval_variable_lookup() {
    let mut env = Environment::new();
    env.set_local("x", Value::Int(42));
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);

    assert_eq!(interp.eval_expr(&var_expr("x")).unwrap(), Value::Int(42));
}

#[test]
fn eval_undefined_variable() {
    assert!(matches!(
        eval(&var_expr("nope")),
        Err(RuntimeError::UndefinedVariable(_))
    ));
}

#[test]
fn eval_global_variable_default_null() {
    assert_eq!(eval(&global_var_expr("missing")).unwrap(), Value::Null);
}

#[test]
fn eval_function_call_unknown() {
    let expr = Expression::FunctionCall {
        name: "nonexistent".to_string(),
        args: vec![str_expr("hello")],
        span: span(),
    };
    assert!(matches!(
        eval(&expr),
        Err(RuntimeError::UnknownFunction(_))
    ));
}

#[test]
fn eval_complex_expression() {
    // (3 + 4) * 2 == 14
    let expr = binary(
        grouped(binary(int_expr(3), ast::BinaryOp::Add, int_expr(4))),
        ast::BinaryOp::Mul,
        int_expr(2),
    );
    assert_eq!(eval(&expr).unwrap(), Value::Int(14));
}

#[test]
fn eval_mixed_int_float_arithmetic() {
    let expr = binary(int_expr(3), ast::BinaryOp::Add, num_expr(0.14));
    let result = eval(&expr).unwrap();
    if let Value::Number(n) = result {
        assert!((n - 3.14).abs() < 1e-10);
    } else {
        panic!("expected Number, got {result:?}");
    }
}

#[test]
fn eval_and_short_circuits() {
    // false && (undefined variable) should not error
    let expr = binary(
        bool_expr(false),
        ast::BinaryOp::And,
        var_expr("undefined"),
    );
    assert_eq!(eval(&expr).unwrap(), Value::Bool(false));
}

#[test]
fn eval_or_short_circuits() {
    // true || (undefined variable) should not error
    let expr = binary(
        bool_expr(true),
        ast::BinaryOp::Or,
        var_expr("undefined"),
    );
    assert_eq!(eval(&expr).unwrap(), Value::Bool(true));
}

#[test]
fn eval_interpolated_with_numbers() {
    let mut env = Environment::new();
    env.set_local("n", Value::Int(42));
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);

    let expr = Expression::Interpolated {
        parts: vec![
            StringPart::Literal("value: ".to_string()),
            StringPart::Expr(var_expr("n")),
        ],
        span: span(),
    };
    assert_eq!(
        interp.eval_expr(&expr).unwrap(),
        Value::String("value: 42".into())
    );
}

#[test]
fn eval_int_float_comparison() {
    let lt = binary(int_expr(3), ast::BinaryOp::Lt, num_expr(3.5));
    assert_eq!(eval(&lt).unwrap(), Value::Bool(true));

    let eq = binary(int_expr(5), ast::BinaryOp::Eq, num_expr(5.0));
    assert_eq!(eval(&eq).unwrap(), Value::Bool(true));
}
