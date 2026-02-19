use super::*;
use crate::ast::{
    self, CommandStatement, ExprStatement, Expression, IfStatement, ReturnStatement, SetStatement,
    Statement, StringPart, VarDeclStatement, WhileStatement,
};
use crate::token::Span;

// ── Test helpers ───────────────────────────────────────────────────────

/// A command handler that rejects all commands (for expression-only tests).
struct RejectingHandler;

impl CommandHandler for RejectingHandler {
    fn handle_command(&mut self, name: &str, _args: &[Value]) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnknownCommand(name.to_string()))
    }
}

fn span() -> Span {
    Span::new(0, 0)
}

fn int_expr(n: i64) -> Expression {
    Expression::IntLiteral {
        value: n,
        span: span(),
    }
}

fn num_expr(n: f64) -> Expression {
    Expression::NumberLiteral {
        value: n,
        span: span(),
    }
}

fn str_expr(s: &str) -> Expression {
    Expression::StringLiteral {
        value: s.to_string(),
        span: span(),
    }
}

fn bool_expr(b: bool) -> Expression {
    Expression::BoolLiteral {
        value: b,
        span: span(),
    }
}

fn var_expr(name: &str) -> Expression {
    Expression::Variable {
        name: name.to_string(),
        span: span(),
    }
}

fn global_var_expr(name: &str) -> Expression {
    Expression::GlobalVariable {
        name: name.to_string(),
        span: span(),
    }
}

fn builtin_expr(name: &str) -> Expression {
    Expression::BuiltinId {
        name: name.to_string(),
        span: span(),
    }
}

fn ident_expr(name: &str) -> Expression {
    Expression::Identifier {
        name: name.to_string(),
        span: span(),
    }
}

fn binary(left: Expression, op: ast::BinaryOp, right: Expression) -> Expression {
    Expression::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
        span: span(),
    }
}

fn unary(op: ast::UnaryOp, operand: Expression) -> Expression {
    Expression::UnaryOp {
        op,
        operand: Box::new(operand),
        span: span(),
    }
}

fn grouped(expr: Expression) -> Expression {
    Expression::Grouped {
        expr: Box::new(expr),
        span: span(),
    }
}

fn var_decl(name: &str, global: bool, value: Expression) -> Statement {
    Statement::VarDecl(VarDeclStatement {
        name: name.to_string(),
        global,
        value,
        span: span(),
    })
}

fn set_stmt(name: &str, global: bool, value: Expression) -> Statement {
    Statement::Set(SetStatement {
        name: name.to_string(),
        global,
        value,
        span: span(),
    })
}

fn if_stmt(
    condition: Expression,
    then_body: Vec<Statement>,
    elseif_branches: Vec<ast::ElseIfBranch>,
    else_body: Option<Vec<Statement>>,
) -> Statement {
    Statement::If(IfStatement {
        condition,
        then_body,
        elseif_branches,
        else_body,
        span: span(),
    })
}

fn while_stmt(condition: Expression, body: Vec<Statement>) -> Statement {
    Statement::While(WhileStatement {
        condition,
        body,
        span: span(),
    })
}

fn return_stmt(value: Option<Expression>) -> Statement {
    Statement::Return(ReturnStatement {
        value,
        span: span(),
    })
}

fn expr_stmt(expr: Expression) -> Statement {
    Statement::ExprStatement(ExprStatement {
        expr,
        span: span(),
    })
}

fn cmd_stmt(name: &str, args: Vec<Expression>) -> Statement {
    Statement::Command(CommandStatement {
        name: name.to_string(),
        args,
        span: span(),
    })
}

/// A command handler that records all commands for test assertions.
struct TestCommandHandler {
    commands: Vec<(String, Vec<Value>)>,
}

impl TestCommandHandler {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }
}

impl CommandHandler for TestCommandHandler {
    fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
        self.commands.push((name.to_string(), args.to_vec()));
        Ok(())
    }
}

fn eval(expr: &Expression) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.eval_expr(expr)
}

fn exec(stmts: &[Statement]) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(stmts)
}

// ── Value tests ────────────────────────────────────────────────────────

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

// ── Expression evaluation tests ────────────────────────────────────────

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
fn eval_function_call_stub() {
    let expr = Expression::FunctionCall {
        name: "len".to_string(),
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

// ── Statement execution tests ──────────────────────────────────────────

#[test]
fn exec_var_decl_local() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("x", false, int_expr(10)),
        expr_stmt(var_expr("x")),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("x"), Some(Value::Int(10)));
}

#[test]
fn exec_var_decl_global() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![var_decl("count", true, int_expr(0))];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_global("count"), Some(Value::Int(0)));
}

#[test]
fn exec_set_local() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("x", false, int_expr(1)),
        set_stmt("x", false, int_expr(99)),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("x"), Some(Value::Int(99)));
}

#[test]
fn exec_set_global() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("g", true, int_expr(0)),
        set_stmt("g", true, int_expr(42)),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_global("g"), Some(Value::Int(42)));
}

#[test]
fn exec_if_true_branch() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(true),
            vec![set_stmt("result", false, str_expr("then"))],
            vec![],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::String("then".into())));
}

#[test]
fn exec_if_false_to_else() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("then"))],
            vec![],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::String("else".into())));
}

#[test]
fn exec_if_elseif() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("if"))],
            vec![ast::ElseIfBranch {
                condition: bool_expr(true),
                body: vec![set_stmt("result", false, str_expr("elseif"))],
                span: span(),
            }],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(
        env.get_local("result"),
        Some(Value::String("elseif".into()))
    );
}

#[test]
fn exec_if_no_match_no_else() {
    let stmts = vec![
        var_decl("result", false, str_expr("unchanged")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("changed"))],
            vec![],
            None,
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(
        env.get_local("result"),
        Some(Value::String("unchanged".into()))
    );
}

#[test]
fn exec_while_basic() {
    // var %i = 0; while (%i < 5) { set %i (%i + 1) }
    let stmts = vec![
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(5)),
            vec![set_stmt(
                "i",
                false,
                binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
            )],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("i"), Some(Value::Int(5)));
}

#[test]
fn exec_while_with_break() {
    // var %i = 0; while (true) { if (%i == 3) { break }; set %i (%i + 1) }
    let stmts = vec![
        var_decl("i", false, int_expr(0)),
        while_stmt(
            bool_expr(true),
            vec![
                if_stmt(
                    binary(var_expr("i"), ast::BinaryOp::Eq, int_expr(3)),
                    vec![Statement::Break(span())],
                    vec![],
                    None,
                ),
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
    assert_eq!(env.get_local("i"), Some(Value::Int(3)));
}

#[test]
fn exec_while_with_continue() {
    // Count only odd numbers: var %sum = 0; var %i = 0;
    // while (%i < 6) { set %i (%i + 1); if (%i % 2 == 0) { continue }; set %sum (%sum + %i) }
    let stmts = vec![
        var_decl("sum", false, int_expr(0)),
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(6)),
            vec![
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
                if_stmt(
                    binary(
                        binary(var_expr("i"), ast::BinaryOp::Mod, int_expr(2)),
                        ast::BinaryOp::Eq,
                        int_expr(0),
                    ),
                    vec![Statement::Continue(span())],
                    vec![],
                    None,
                ),
                set_stmt(
                    "sum",
                    false,
                    binary(var_expr("sum"), ast::BinaryOp::Add, var_expr("i")),
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    // 1 + 3 + 5 = 9
    assert_eq!(env.get_local("sum"), Some(Value::Int(9)));
}

#[test]
fn exec_return_value() {
    let stmts = vec![
        return_stmt(Some(int_expr(42))),
        // This should not be reached
        var_decl("x", false, int_expr(99)),
    ];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Return(Value::Int(42)))));
}

#[test]
fn exec_return_no_value() {
    let stmts = vec![return_stmt(None)];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Return(Value::Null))));
}

#[test]
fn exec_break_outside_loop() {
    let stmts = vec![Statement::Break(span())];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Break)));
}

#[test]
fn exec_continue_outside_loop() {
    let stmts = vec![Statement::Continue(span())];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Continue)));
}

#[test]
fn exec_command_handler() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![cmd_stmt("echo", vec![str_expr("hello"), int_expr(42)])];
    {
        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.exec_stmts(&stmts).unwrap();
    }
    assert_eq!(handler.commands.len(), 1);
    assert_eq!(handler.commands[0].0, "echo");
    assert_eq!(handler.commands[0].1, vec![
        Value::String("hello".into()),
        Value::Int(42),
    ]);
}

#[test]
fn exec_command_stub_handler_rejects() {
    let stmts = vec![cmd_stmt("msg", vec![str_expr("#chan"), str_expr("hi")])];
    let result = exec(&stmts);
    // TestCommandHandler accepts commands, so this should succeed
    assert!(result.is_ok());

    // But RejectingHandler rejects
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);
    let result = interp.exec_stmts(&stmts);
    assert!(matches!(result, Err(RuntimeError::UnknownCommand(_))));
}

// ── Environment tests ──────────────────────────────────────────────────

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

// ── Logical short-circuit tests ─────────────────────────────────────

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

// ── Nested control flow tests ─────────────────────────────────────

#[test]
fn exec_nested_if_while() {
    // var %result = 0
    // var %i = 0
    // while (%i < 10) {
    //   set %i (%i + 1)
    //   if (%i % 3 == 0) {
    //     set %result (%result + %i)
    //   }
    // }
    // result should be 3 + 6 + 9 = 18
    let stmts = vec![
        var_decl("result", false, int_expr(0)),
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(10)),
            vec![
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
                if_stmt(
                    binary(
                        binary(var_expr("i"), ast::BinaryOp::Mod, int_expr(3)),
                        ast::BinaryOp::Eq,
                        int_expr(0),
                    ),
                    vec![set_stmt(
                        "result",
                        false,
                        binary(var_expr("result"), ast::BinaryOp::Add, var_expr("i")),
                    )],
                    vec![],
                    None,
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::Int(18)));
}

#[test]
fn exec_expr_statement_discards_value() {
    // An expression statement evaluates but discards its result
    let stmts = vec![expr_stmt(int_expr(42))];
    let result = exec(&stmts).unwrap();
    assert_eq!(result, Value::Null);
}

// ── Interpolated string with multiple types ─────────────────────────

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

// ── Int/float comparison coercion ────────────────────────────────

#[test]
fn eval_int_float_comparison() {
    let lt = binary(int_expr(3), ast::BinaryOp::Lt, num_expr(3.5));
    assert_eq!(eval(&lt).unwrap(), Value::Bool(true));

    let eq = binary(int_expr(5), ast::BinaryOp::Eq, num_expr(5.0));
    assert_eq!(eval(&eq).unwrap(), Value::Bool(true));
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

// ── Global variable roundtrip ───────────────────────────────────────

#[test]
fn exec_global_variable_roundtrip() {
    let stmts = vec![
        var_decl("counter", true, int_expr(0)),
        set_stmt("counter", true, int_expr(42)),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    {
        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.exec_stmts(&stmts).unwrap();
    }
    assert_eq!(env.get_global("counter"), Some(Value::Int(42)));

    // Verify we can read it back through the interpreter
    let mut handler2 = TestCommandHandler::new();
    let mut interp2 = Interpreter::new(&mut env, &mut handler2);
    let result = interp2.eval_expr(&global_var_expr("counter")).unwrap();
    assert_eq!(result, Value::Int(42));
}
