use crate::ast::{
    self, CommandStatement, ExprStatement, Expression, IfStatement, ReturnStatement, SetStatement,
    Statement, VarDeclStatement, WhileStatement,
};
use crate::interpreter::builtins::BuiltinContext;
use crate::interpreter::environment::Environment;
use crate::interpreter::functions::{FunctionRegistry, RegexState};
use crate::interpreter::{CommandHandler, Interpreter, RuntimeError, Value};
use crate::token::Span;

/// A command handler that rejects all commands (for expression-only tests).
pub struct RejectingHandler;

impl CommandHandler for RejectingHandler {
    fn handle_command(&mut self, name: &str, _args: &[Value]) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnknownCommand(name.to_string()))
    }
}

/// A command handler that records all commands for test assertions.
pub struct TestCommandHandler {
    pub commands: Vec<(String, Vec<Value>)>,
}

impl TestCommandHandler {
    pub fn new() -> Self {
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

pub fn span() -> Span {
    Span::new(0, 0)
}

pub fn int_expr(n: i64) -> Expression {
    Expression::IntLiteral {
        value: n,
        span: span(),
    }
}

pub fn num_expr(n: f64) -> Expression {
    Expression::NumberLiteral {
        value: n,
        span: span(),
    }
}

pub fn str_expr(s: &str) -> Expression {
    Expression::StringLiteral {
        value: s.to_string(),
        span: span(),
    }
}

pub fn bool_expr(b: bool) -> Expression {
    Expression::BoolLiteral {
        value: b,
        span: span(),
    }
}

pub fn var_expr(name: &str) -> Expression {
    Expression::Variable {
        name: name.to_string(),
        span: span(),
    }
}

pub fn global_var_expr(name: &str) -> Expression {
    Expression::GlobalVariable {
        name: name.to_string(),
        span: span(),
    }
}

pub fn builtin_expr(name: &str) -> Expression {
    Expression::BuiltinId {
        name: name.to_string(),
        span: span(),
    }
}

pub fn ident_expr(name: &str) -> Expression {
    Expression::Identifier {
        name: name.to_string(),
        span: span(),
    }
}

pub fn binary(left: Expression, op: ast::BinaryOp, right: Expression) -> Expression {
    Expression::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
        span: span(),
    }
}

pub fn unary(op: ast::UnaryOp, operand: Expression) -> Expression {
    Expression::UnaryOp {
        op,
        operand: Box::new(operand),
        span: span(),
    }
}

pub fn grouped(expr: Expression) -> Expression {
    Expression::Grouped {
        expr: Box::new(expr),
        span: span(),
    }
}

pub fn var_decl(name: &str, global: bool, value: Expression) -> Statement {
    Statement::VarDecl(VarDeclStatement {
        name: name.to_string(),
        global,
        value,
        span: span(),
    })
}

pub fn set_stmt(name: &str, global: bool, value: Expression) -> Statement {
    Statement::Set(SetStatement {
        name: name.to_string(),
        global,
        value,
        span: span(),
    })
}

pub fn if_stmt(
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

pub fn while_stmt(condition: Expression, body: Vec<Statement>) -> Statement {
    Statement::While(WhileStatement {
        condition,
        body,
        span: span(),
    })
}

pub fn return_stmt(value: Option<Expression>) -> Statement {
    Statement::Return(ReturnStatement {
        value,
        span: span(),
    })
}

pub fn expr_stmt(expr: Expression) -> Statement {
    Statement::ExprStatement(ExprStatement {
        expr,
        span: span(),
    })
}

pub fn cmd_stmt(name: &str, args: Vec<Expression>) -> Statement {
    Statement::Command(CommandStatement {
        name: name.to_string(),
        args,
        span: span(),
    })
}

pub fn func_call(name: &str, args: Vec<Expression>) -> Expression {
    Expression::FunctionCall {
        name: name.to_string(),
        args,
        span: span(),
    }
}

pub fn eval(expr: &Expression) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.eval_expr(expr)
}

pub fn eval_with_context(
    expr: &Expression,
    ctx: &BuiltinContext,
) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let functions = FunctionRegistry::new();
    let regex_state = RegexState::new();
    let mut interp =
        Interpreter::with_context(&mut env, &mut handler, ctx, &functions, &regex_state);
    interp.eval_expr(expr)
}

pub fn eval_with_regex(
    expr: &Expression,
    regex_state: &RegexState,
) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let ctx = BuiltinContext::new();
    let functions = FunctionRegistry::new();
    let mut interp =
        Interpreter::with_context(&mut env, &mut handler, &ctx, &functions, regex_state);
    interp.eval_expr(expr)
}

pub fn exec(stmts: &[Statement]) -> Result<Value, RuntimeError> {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(stmts)
}
