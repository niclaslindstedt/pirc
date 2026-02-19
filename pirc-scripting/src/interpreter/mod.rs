//! Tree-walking interpreter for the pirc scripting language.
//!
//! Evaluates parsed AST nodes: expressions produce [`Value`]s,
//! statements mutate [`Environment`] state and may produce side-effects
//! via a [`CommandHandler`] trait.

mod environment;
mod value;

#[cfg(test)]
mod tests;

pub use environment::Environment;
pub use value::Value;

use crate::ast::{
    BinaryOp, CommandStatement, ExprStatement, Expression, IfStatement, ReturnStatement,
    SetStatement, Statement, StringPart, UnaryOp, VarDeclStatement, WhileStatement,
};

/// Maximum number of loop iterations before the interpreter bails out.
const MAX_LOOP_ITERATIONS: u64 = 100_000;

/// Errors produced during script interpretation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// A type mismatch in an operation (e.g., adding a bool to an int).
    #[error("type error: {0}")]
    TypeError(String),

    /// An undefined variable was referenced.
    #[error("undefined variable: {0}")]
    UndefinedVariable(String),

    /// Division by zero.
    #[error("division by zero")]
    DivisionByZero,

    /// An unknown built-in identifier was referenced.
    #[error("unknown builtin: ${0}")]
    UnknownBuiltin(String),

    /// An unknown function was called.
    #[error("unknown function: ${0}")]
    UnknownFunction(String),

    /// An unknown command was invoked.
    #[error("unknown command: {0}")]
    UnknownCommand(String),

    /// A loop exceeded the maximum iteration count.
    #[error("loop iteration limit exceeded ({MAX_LOOP_ITERATIONS})")]
    LoopLimit,

    /// Internal: a `return` was executed (used for control flow).
    #[error("return outside of alias/event/timer body")]
    Return(Value),

    /// Internal: a `break` was executed outside a loop context.
    #[error("break outside of loop")]
    Break,

    /// Internal: a `continue` was executed outside a loop context.
    #[error("continue outside of loop")]
    Continue,
}

/// Trait for handling script commands (e.g., `msg`, `echo`, `join`).
///
/// Implementations receive evaluated argument values and perform the
/// appropriate action (sending IRC messages, printing output, etc.).
pub trait CommandHandler {
    /// Handle a command with the given name and evaluated arguments.
    ///
    /// # Errors
    ///
    /// Returns a [`RuntimeError`] if the command is unknown or fails.
    fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError>;
}

/// The script interpreter.
///
/// Evaluates expressions and executes statements against an [`Environment`],
/// dispatching commands through a [`CommandHandler`].
pub struct Interpreter<'a> {
    env: &'a mut Environment,
    cmd_handler: &'a mut dyn CommandHandler,
}

impl<'a> Interpreter<'a> {
    /// Creates a new interpreter with the given environment and command handler.
    pub fn new(env: &'a mut Environment, cmd_handler: &'a mut dyn CommandHandler) -> Self {
        Self { env, cmd_handler }
    }

    // ── Expression evaluation ──────────────────────────────────────────

    /// Evaluates an expression and returns its [`Value`].
    ///
    /// # Errors
    ///
    /// Returns a [`RuntimeError`] on type mismatches, undefined variables, etc.
    pub fn eval_expr(&mut self, expr: &Expression) -> Result<Value, RuntimeError> {
        match expr {
            // Literals
            Expression::StringLiteral { value, .. } => Ok(Value::String(value.clone())),
            Expression::IntLiteral { value, .. } => Ok(Value::Int(*value)),
            Expression::NumberLiteral { value, .. } => Ok(Value::Number(*value)),
            Expression::BoolLiteral { value, .. } => Ok(Value::Bool(*value)),

            // Variables
            Expression::Variable { name, .. } => self
                .env
                .get_local(name)
                .ok_or_else(|| RuntimeError::UndefinedVariable(format!("%{name}"))),

            Expression::GlobalVariable { name, .. } => {
                Ok(self.env.get_global(name).unwrap_or(Value::Null))
            }

            // Built-in identifiers: return well-known constants or Null
            Expression::BuiltinId { name, .. } => Ok(eval_builtin(name)),

            // Plain identifiers are treated as string literals (mIRC semantics)
            Expression::Identifier { name, .. } => Ok(Value::String(name.clone())),

            // Function calls (stub — later tickets provide real builtins)
            Expression::FunctionCall { name, args, .. } => self.eval_function_call(name, args),

            // Interpolated strings
            Expression::Interpolated { parts, .. } => self.eval_interpolated(parts),

            // Grouped (parenthesized)
            Expression::Grouped { expr, .. } => self.eval_expr(expr),

            // Binary operations
            Expression::BinaryOp {
                left, op, right, ..
            } => self.eval_binary_op(left, *op, right),

            // Unary operations
            Expression::UnaryOp { op, operand, .. } => self.eval_unary_op(*op, operand),
        }
    }

    fn eval_function_call(
        &mut self,
        name: &str,
        args: &[Expression],
    ) -> Result<Value, RuntimeError> {
        // Evaluate arguments eagerly (we'll need them for any builtin)
        let _evaluated: Vec<Value> = args
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;

        // Stub: no built-in functions yet (T241 adds them)
        Err(RuntimeError::UnknownFunction(name.to_string()))
    }

    fn eval_interpolated(&mut self, parts: &[StringPart]) -> Result<Value, RuntimeError> {
        let mut buf = String::new();
        for part in parts {
            match part {
                StringPart::Literal(s) => buf.push_str(s),
                StringPart::Expr(expr) => {
                    let val = self.eval_expr(expr)?;
                    buf.push_str(&val.to_string());
                }
            }
        }
        Ok(Value::String(buf))
    }

    fn eval_binary_op(
        &mut self,
        left: &Expression,
        op: BinaryOp,
        right: &Expression,
    ) -> Result<Value, RuntimeError> {
        // Short-circuit for logical operators
        if op == BinaryOp::And {
            let lv = self.eval_expr(left)?;
            if !lv.is_truthy() {
                return Ok(Value::Bool(false));
            }
            let rv = self.eval_expr(right)?;
            return Ok(Value::Bool(rv.is_truthy()));
        }
        if op == BinaryOp::Or {
            let lv = self.eval_expr(left)?;
            if lv.is_truthy() {
                return Ok(Value::Bool(true));
            }
            let rv = self.eval_expr(right)?;
            return Ok(Value::Bool(rv.is_truthy()));
        }

        let lv = self.eval_expr(left)?;
        let rv = self.eval_expr(right)?;

        match op {
            // Arithmetic
            BinaryOp::Add => lv.add(&rv),
            BinaryOp::Sub => lv.sub(&rv),
            BinaryOp::Mul => lv.mul(&rv),
            BinaryOp::Div => lv.div(&rv),
            BinaryOp::Mod => lv.modulo(&rv),

            // Comparison
            BinaryOp::Eq => Ok(Value::Bool(lv.equals(&rv))),
            BinaryOp::Neq => Ok(Value::Bool(!lv.equals(&rv))),
            BinaryOp::Lt => lv.compare(&rv, Ordering::Less),
            BinaryOp::Gt => lv.compare(&rv, Ordering::Greater),
            BinaryOp::Lte => lv.compare_lte(&rv),
            BinaryOp::Gte => lv.compare_gte(&rv),

            // Logical (already handled above, but needed for exhaustive match)
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }

    fn eval_unary_op(
        &mut self,
        op: UnaryOp,
        operand: &Expression,
    ) -> Result<Value, RuntimeError> {
        let val = self.eval_expr(operand)?;
        match op {
            UnaryOp::Not => Ok(Value::Bool(!val.is_truthy())),
            UnaryOp::Neg => val.negate(),
        }
    }

    // ── Statement execution ────────────────────────────────────────────

    /// Executes a list of statements.
    ///
    /// Returns `Ok(Value::Null)` on normal completion, or propagates
    /// control-flow signals (`Return`, `Break`, `Continue`) as errors.
    ///
    /// # Errors
    ///
    /// Returns a [`RuntimeError`] on evaluation failures or control-flow signals.
    pub fn exec_stmts(&mut self, stmts: &[Statement]) -> Result<Value, RuntimeError> {
        let mut result = Value::Null;
        for stmt in stmts {
            result = self.exec_stmt(stmt)?;
        }
        Ok(result)
    }

    /// Executes a single statement.
    ///
    /// # Errors
    ///
    /// Returns a [`RuntimeError`] on evaluation failures or control-flow signals.
    pub fn exec_stmt(&mut self, stmt: &Statement) -> Result<Value, RuntimeError> {
        match stmt {
            Statement::VarDecl(decl) => self.exec_var_decl(decl),
            Statement::Set(set) => self.exec_set(set),
            Statement::If(if_stmt) => self.exec_if(if_stmt),
            Statement::While(while_stmt) => self.exec_while(while_stmt),
            Statement::Return(ret) => self.exec_return(ret),
            Statement::Break(_) => Err(RuntimeError::Break),
            Statement::Continue(_) => Err(RuntimeError::Continue),
            Statement::ExprStatement(expr_stmt) => self.exec_expr_stmt(expr_stmt),
            Statement::Command(cmd) => self.exec_command(cmd),
        }
    }

    fn exec_var_decl(&mut self, decl: &VarDeclStatement) -> Result<Value, RuntimeError> {
        let val = self.eval_expr(&decl.value)?;
        if decl.global {
            self.env.set_global(&decl.name, val);
        } else {
            self.env.set_local(&decl.name, val);
        }
        Ok(Value::Null)
    }

    fn exec_set(&mut self, set: &SetStatement) -> Result<Value, RuntimeError> {
        let val = self.eval_expr(&set.value)?;
        if set.global {
            self.env.set_global(&set.name, val);
        } else {
            // For `set`, update existing local variable or create in current scope
            if self.env.get_local(&set.name).is_some() {
                self.env.update_local(&set.name, val);
            } else {
                self.env.set_local(&set.name, val);
            }
        }
        Ok(Value::Null)
    }

    fn exec_if(&mut self, if_stmt: &IfStatement) -> Result<Value, RuntimeError> {
        let cond = self.eval_expr(&if_stmt.condition)?;
        if cond.is_truthy() {
            self.env.push_scope();
            let result = self.exec_stmts(&if_stmt.then_body);
            self.env.pop_scope();
            return result;
        }

        for branch in &if_stmt.elseif_branches {
            let cond = self.eval_expr(&branch.condition)?;
            if cond.is_truthy() {
                self.env.push_scope();
                let result = self.exec_stmts(&branch.body);
                self.env.pop_scope();
                return result;
            }
        }

        if let Some(else_body) = &if_stmt.else_body {
            self.env.push_scope();
            let result = self.exec_stmts(else_body);
            self.env.pop_scope();
            return result;
        }

        Ok(Value::Null)
    }

    fn exec_while(&mut self, while_stmt: &WhileStatement) -> Result<Value, RuntimeError> {
        let mut iterations: u64 = 0;
        loop {
            let cond = self.eval_expr(&while_stmt.condition)?;
            if !cond.is_truthy() {
                break;
            }

            iterations += 1;
            if iterations > MAX_LOOP_ITERATIONS {
                return Err(RuntimeError::LoopLimit);
            }

            self.env.push_scope();
            let result = self.exec_stmts(&while_stmt.body);
            self.env.pop_scope();

            match result {
                Ok(_) | Err(RuntimeError::Continue) => {}
                Err(RuntimeError::Break) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(Value::Null)
    }

    fn exec_return(&mut self, ret: &ReturnStatement) -> Result<Value, RuntimeError> {
        let val = match &ret.value {
            Some(expr) => self.eval_expr(expr)?,
            None => Value::Null,
        };
        Err(RuntimeError::Return(val))
    }

    fn exec_expr_stmt(&mut self, expr_stmt: &ExprStatement) -> Result<Value, RuntimeError> {
        self.eval_expr(&expr_stmt.expr)?;
        Ok(Value::Null)
    }

    fn exec_command(&mut self, cmd: &CommandStatement) -> Result<Value, RuntimeError> {
        let args: Vec<Value> = cmd
            .args
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;
        self.cmd_handler.handle_command(&cmd.name, &args)?;
        Ok(Value::Null)
    }
}

/// Evaluates a built-in identifier to its value.
///
/// Returns well-known constants (`$null`, `$true`, `$false`, `$cr`, `$lf`,
/// `$crlf`, `$tab`) directly. All other builtins return `Null` as a stub
/// until the client runtime context provides real values (T241+).
fn eval_builtin(name: &str) -> Value {
    match name {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "cr" => Value::String("\r".to_string()),
        "lf" => Value::String("\n".to_string()),
        "crlf" => Value::String("\r\n".to_string()),
        "tab" => Value::String("\t".to_string()),
        // "null" and all other builtins return Null as a stub
        _ => Value::Null,
    }
}

use std::cmp::Ordering;
