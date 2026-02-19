//! Tree-walking interpreter for the pirc scripting language.
//!
//! Evaluates parsed AST nodes: expressions produce [`Value`]s,
//! statements mutate [`Environment`] state and may produce side-effects
//! via a [`CommandHandler`] trait.
//!
//! Commands are resolved through a three-level dispatch chain:
//! 1. Built-in script commands (`/echo`, `/halt`, `/noop`)
//! 2. Registered aliases (via [`AliasRegistry`])
//! 3. External [`CommandHandler`] trait

mod builtins;
pub mod command;
mod environment;
pub mod event;
mod functions;
pub mod timer;
mod value;

#[cfg(test)]
mod tests;

pub use builtins::BuiltinContext;
pub use command::AliasRegistry;
pub use environment::Environment;
pub use event::{EventContext, EventDispatcher};
pub use functions::{FunctionRegistry, RegexState};
pub use timer::{FiredTimer, TimerManager};
pub use value::Value;

// Re-export ScriptHost and related types at the interpreter module level.
// (ScriptHost and ScriptRuntimeError are defined in this module.)

use crate::ast::{
    BinaryOp, CommandStatement, ExprStatement, Expression, IfStatement, ReturnStatement,
    SetStatement, Statement, StringPart, UnaryOp, VarDeclStatement, WhileStatement,
};

/// Maximum number of loop iterations before the interpreter bails out.
const MAX_LOOP_ITERATIONS: u64 = 100_000;

/// Maximum alias recursion depth to prevent infinite loops.
const MAX_ALIAS_DEPTH: usize = 100;

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

    /// Alias recursion depth exceeded the limit.
    #[error("alias recursion limit exceeded (max {MAX_ALIAS_DEPTH})")]
    AliasRecursionLimit,

    /// A `/halt` command was executed to stop script execution.
    #[error("script halted")]
    Halt,

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

/// Callback interface for the scripting engine to interact with the IRC client.
///
/// The client implements this trait to provide the scripting engine with:
/// - Command dispatch (via [`CommandHandler`])
/// - Client state queries (`$me`, `$server`, `$channel`, `$port`)
/// - User-facing output (`/echo`)
/// - Error and warning reporting
///
/// This decouples the scripting engine from the client without tight coupling.
pub trait ScriptHost: CommandHandler {
    /// Returns the client's current nickname.
    fn current_nick(&self) -> &str;

    /// Returns the server hostname the client is connected to, if any.
    fn current_server(&self) -> Option<&str>;

    /// Returns the currently active channel, if any.
    fn current_channel(&self) -> Option<&str>;

    /// Returns the server port number.
    fn server_port(&self) -> u16;

    /// Displays text to the user (like the `/echo` command).
    fn echo(&mut self, text: &str);

    /// Reports a runtime error to the client.
    ///
    /// The error includes a human-readable message with source context
    /// (filename, line/column) when available.
    fn report_error(&mut self, error: &ScriptRuntimeError);

    /// Reports a non-fatal warning to the client.
    fn report_warning(&mut self, warning: &str);
}

/// A runtime error with optional source location context.
///
/// Wraps a [`RuntimeError`] with the filename and location where
/// the error occurred, enabling informative error messages.
#[derive(Debug, Clone)]
pub struct ScriptRuntimeError {
    /// The underlying runtime error.
    pub error: RuntimeError,
    /// The script filename where the error occurred.
    pub filename: Option<String>,
    /// Human-readable context (e.g., "event handler", "alias 'greet'", "timer 'keepalive'").
    pub context: String,
}

impl std::fmt::Display for ScriptRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref filename) = self.filename {
            write!(f, "[{filename}] {}: {}", self.context, self.error)
        } else {
            write!(f, "{}: {}", self.context, self.error)
        }
    }
}

/// The script interpreter.
///
/// Evaluates expressions and executes statements against an [`Environment`],
/// dispatching commands through a three-level chain: built-in commands,
/// registered aliases, and finally the external [`CommandHandler`].
pub struct Interpreter<'a> {
    env: &'a mut Environment,
    cmd_handler: &'a mut dyn CommandHandler,
    builtin_ctx: &'a BuiltinContext,
    functions: &'a FunctionRegistry,
    regex_state: &'a RegexState,
    aliases: Option<&'a AliasRegistry>,
    echo_output: Option<&'a mut Vec<String>>,
    timer_manager: Option<&'a mut TimerManager>,
    alias_depth: usize,
    /// Owned builtin context override used during alias execution.
    /// When set, this takes precedence over `builtin_ctx`.
    ctx_override: Option<BuiltinContext>,
}

impl<'a> Interpreter<'a> {
    /// Creates a new interpreter with the given environment and command handler.
    ///
    /// Uses default (empty) builtin context and function registry.
    /// No alias registry or echo output is configured.
    pub fn new(env: &'a mut Environment, cmd_handler: &'a mut dyn CommandHandler) -> Self {
        static DEFAULT_CTX: std::sync::LazyLock<BuiltinContext> =
            std::sync::LazyLock::new(BuiltinContext::new);
        static DEFAULT_FUNCS: std::sync::LazyLock<FunctionRegistry> =
            std::sync::LazyLock::new(FunctionRegistry::new);
        static DEFAULT_REGEX: std::sync::LazyLock<RegexState> =
            std::sync::LazyLock::new(RegexState::new);
        Self {
            env,
            cmd_handler,
            builtin_ctx: &DEFAULT_CTX,
            functions: &DEFAULT_FUNCS,
            regex_state: &DEFAULT_REGEX,
            aliases: None,
            echo_output: None,
            timer_manager: None,
            alias_depth: 0,
            ctx_override: None,
        }
    }

    /// Creates a new interpreter with full context including builtins and functions.
    pub fn with_context(
        env: &'a mut Environment,
        cmd_handler: &'a mut dyn CommandHandler,
        builtin_ctx: &'a BuiltinContext,
        functions: &'a FunctionRegistry,
        regex_state: &'a RegexState,
    ) -> Self {
        Self {
            env,
            cmd_handler,
            builtin_ctx,
            functions,
            regex_state,
            aliases: None,
            echo_output: None,
            timer_manager: None,
            alias_depth: 0,
            ctx_override: None,
        }
    }

    /// Sets the alias registry for command dispatch.
    pub fn set_aliases(&mut self, aliases: &'a AliasRegistry) {
        self.aliases = Some(aliases);
    }

    /// Sets the echo output buffer for the `/echo` built-in command.
    pub fn set_echo_output(&mut self, output: &'a mut Vec<String>) {
        self.echo_output = Some(output);
    }

    /// Sets the timer manager for built-in timer commands (`/timers`, `/timeoff`).
    pub fn set_timer_manager(&mut self, timer_manager: &'a mut TimerManager) {
        self.timer_manager = Some(timer_manager);
    }

    /// Sets the current alias recursion depth.
    pub fn set_alias_depth(&mut self, depth: usize) {
        self.alias_depth = depth;
    }

    /// Returns the effective builtin context (override if set, else base).
    fn effective_ctx(&self) -> &BuiltinContext {
        self.ctx_override.as_ref().unwrap_or(self.builtin_ctx)
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

            // Built-in identifiers: resolve from context
            Expression::BuiltinId { name, .. } => Ok(self.effective_ctx().resolve(name)),

            // Plain identifiers are treated as string literals (mIRC semantics)
            Expression::Identifier { name, .. } => Ok(Value::String(name.clone())),

            // Function calls
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
        let evaluated: Vec<Value> = args
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;

        self.functions.call(name, &evaluated, self.regex_state)
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

        // Full dispatch chain is only active when aliases are configured.
        // Without aliases, commands go directly to the external handler
        // (preserving backward compatibility).
        if self.aliases.is_some() {
            // 1. Built-in script commands
            if let Some(result) = self.try_builtin_command(&cmd.name, &args) {
                return result;
            }

            // 2. Registered aliases
            if let Some(aliases) = self.aliases {
                if let Some(body) = aliases.get(&cmd.name) {
                    let body = body.to_vec();
                    return self.exec_alias_call(&body, &args);
                }
            }
        }

        // 3. External handler (or direct handler when no aliases configured)
        self.cmd_handler.handle_command(&cmd.name, &args)?;
        Ok(Value::Null)
    }

    /// Checks if the command is a built-in script command and handles it.
    ///
    /// Returns `Some(result)` if handled, `None` otherwise.
    fn try_builtin_command(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Option<Result<Value, RuntimeError>> {
        match name.to_lowercase().as_str() {
            "echo" => {
                let text: String = args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(ref mut output) = self.echo_output {
                    output.push(text);
                }
                Some(Ok(Value::Null))
            }
            "noop" => Some(Ok(Value::Null)),
            "halt" => Some(Err(RuntimeError::Halt)),
            "timer" => {
                // /timer -r name — remove a timer
                if args.len() >= 2 && args[0].to_string() == "-r" {
                    if let Some(ref mut tm) = self.timer_manager {
                        tm.remove(&args[1].to_string());
                    }
                    Some(Ok(Value::Null))
                } else {
                    // Timer definitions are handled at the script loading level,
                    // not as runtime commands. Pass through to external handler.
                    None
                }
            }
            "timeoff" => {
                // /timeoff name — remove a timer
                if let Some(ref mut tm) = self.timer_manager {
                    if let Some(timer_name) = args.first() {
                        tm.remove(&timer_name.to_string());
                    }
                }
                Some(Ok(Value::Null))
            }
            "timers" => {
                // /timers — list active timers
                if let Some(ref tm) = self.timer_manager {
                    let info = tm.timer_info();
                    if info.is_empty() {
                        if let Some(ref mut output) = self.echo_output {
                            output.push("No active timers".to_string());
                        }
                    } else {
                        for line in info {
                            if let Some(ref mut output) = self.echo_output {
                                output.push(line);
                            }
                        }
                    }
                }
                Some(Ok(Value::Null))
            }
            _ => None,
        }
    }

    /// Executes an alias body with argument binding and recursion protection.
    ///
    /// Pushes a fresh scope, populates `$0`–`$9` from the alias arguments,
    /// executes the body, and pops the scope. `Return` is caught as normal
    /// alias completion.
    fn exec_alias_call(
        &mut self,
        body: &[Statement],
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if self.alias_depth >= MAX_ALIAS_DEPTH {
            return Err(RuntimeError::AliasRecursionLimit);
        }

        // Build the argument string for $0-$9 splitting
        let arg_text = args
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");

        // Create a builtin context with $0-$9 populated from alias arguments,
        // inheriting other context identifiers from the current context.
        let mut alias_ctx = self.effective_ctx().clone();
        alias_ctx.set_event_text(&arg_text);

        // Save previous state and set up alias execution context
        let prev_override = self.ctx_override.take();
        let prev_depth = self.alias_depth;

        self.ctx_override = Some(alias_ctx);
        self.alias_depth += 1;

        // Push a fresh scope for the alias body
        self.env.push_scope();

        let result = self.exec_stmts(body);

        self.env.pop_scope();

        // Restore previous state
        self.ctx_override = prev_override;
        self.alias_depth = prev_depth;

        // Catch Return as normal alias completion; propagate other errors
        match result {
            Ok(_) | Err(RuntimeError::Return(_)) => Ok(Value::Null),
            Err(e) => Err(e),
        }
    }
}

use std::cmp::Ordering;
