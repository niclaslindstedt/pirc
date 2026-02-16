use crate::token::Span;

/// A complete script: the root of the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// Top-level items in the script.
    pub items: Vec<TopLevelItem>,
    /// Span covering the entire script.
    pub span: Span,
}

/// A top-level item in a script.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevelItem {
    /// An alias (custom command) definition.
    Alias(AliasDefinition),
    /// An IRC event handler.
    Event(EventHandler),
    /// A timer declaration.
    Timer(TimerDefinition),
}

impl TopLevelItem {
    /// Returns the span of this top-level item.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Alias(a) => a.span,
            Self::Event(e) => e.span,
            Self::Timer(t) => t.span,
        }
    }
}

/// An alias definition: `alias name { body }`.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasDefinition {
    /// The alias name (without leading `/`).
    pub name: String,
    /// The statements in the alias body.
    pub body: Vec<Statement>,
    /// Source span.
    pub span: Span,
}

/// An event handler: `on EVENT:pattern { body }`.
#[derive(Debug, Clone, PartialEq)]
pub struct EventHandler {
    /// The type of IRC event to handle.
    pub event_type: EventType,
    /// The glob pattern to match (e.g., `*`, `#channel`, `*hello*`).
    pub pattern: String,
    /// The statements in the handler body.
    pub body: Vec<Statement>,
    /// Source span.
    pub span: Span,
}

/// A timer declaration: `timer name interval repetitions { body }`.
#[derive(Debug, Clone, PartialEq)]
pub struct TimerDefinition {
    /// The timer name.
    pub name: String,
    /// The interval expression (in seconds).
    pub interval: Expression,
    /// The repetition count expression (0 = infinite).
    pub repetitions: Expression,
    /// The statements in the timer body.
    pub body: Vec<Statement>,
    /// Source span.
    pub span: Span,
}

/// IRC event types that can be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// A channel or private message.
    Text,
    /// A user joins a channel.
    Join,
    /// A user leaves a channel.
    Part,
    /// A user is kicked from a channel.
    Kick,
    /// A user quits IRC.
    Quit,
    /// The client connects to a server.
    Connect,
    /// The client disconnects from a server.
    Disconnect,
    /// A channel/user invite.
    Invite,
    /// A NOTICE message.
    Notice,
    /// A user changes their nickname.
    Nick,
    /// A channel topic change.
    Topic,
    /// A mode change.
    Mode,
    /// A CTCP request.
    Ctcp,
    /// A CTCP ACTION (`/me`).
    Action,
    /// A numeric server reply.
    Numeric,
}

/// A statement in a block body.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// A command invocation: `msg #chan "hello"`.
    Command(CommandStatement),
    /// An if/elseif/else chain.
    If(IfStatement),
    /// A while loop.
    While(WhileStatement),
    /// A local variable declaration: `var %x = expr`.
    VarDecl(VarDeclStatement),
    /// A variable assignment: `set %x expr`.
    Set(SetStatement),
    /// A return statement: `return [expr]`.
    Return(ReturnStatement),
    /// A `break` statement.
    Break(Span),
    /// A `continue` statement.
    Continue(Span),
    /// An expression used as a statement.
    ExprStatement(ExprStatement),
}

impl Statement {
    /// Returns the span of this statement.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Command(s) => s.span,
            Self::If(s) => s.span,
            Self::While(s) => s.span,
            Self::VarDecl(s) => s.span,
            Self::Set(s) => s.span,
            Self::Return(s) => s.span,
            Self::Break(span) | Self::Continue(span) => *span,
            Self::ExprStatement(s) => s.span,
        }
    }
}

/// A command invocation statement.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandStatement {
    /// The command name (e.g., `msg`, `join`, `echo`).
    pub name: String,
    /// The argument expressions.
    pub args: Vec<Expression>,
    /// Source span.
    pub span: Span,
}

/// An if/elseif/else statement.
#[derive(Debug, Clone, PartialEq)]
pub struct IfStatement {
    /// The condition expression.
    pub condition: Expression,
    /// The body executed when the condition is true.
    pub then_body: Vec<Statement>,
    /// Zero or more `elseif` branches.
    pub elseif_branches: Vec<ElseIfBranch>,
    /// Optional `else` body.
    pub else_body: Option<Vec<Statement>>,
    /// Source span.
    pub span: Span,
}

/// An `elseif` branch in an if chain.
#[derive(Debug, Clone, PartialEq)]
pub struct ElseIfBranch {
    /// The condition expression.
    pub condition: Expression,
    /// The body executed when this condition is true.
    pub body: Vec<Statement>,
    /// Source span.
    pub span: Span,
}

/// A while loop statement.
#[derive(Debug, Clone, PartialEq)]
pub struct WhileStatement {
    /// The loop condition.
    pub condition: Expression,
    /// The loop body.
    pub body: Vec<Statement>,
    /// Source span.
    pub span: Span,
}

/// A variable declaration: `var %name = expr`.
#[derive(Debug, Clone, PartialEq)]
pub struct VarDeclStatement {
    /// The variable name (without `%` / `%%` prefix).
    pub name: String,
    /// Whether this is a global (`%%`) variable.
    pub global: bool,
    /// The initializer expression.
    pub value: Expression,
    /// Source span.
    pub span: Span,
}

/// A set (assignment) statement: `set %name expr`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetStatement {
    /// The variable name (without `%` / `%%` prefix).
    pub name: String,
    /// Whether this is a global (`%%`) variable.
    pub global: bool,
    /// The value expression.
    pub value: Expression,
    /// Source span.
    pub span: Span,
}

/// A return statement: `return [expr]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnStatement {
    /// Optional return value.
    pub value: Option<Expression>,
    /// Source span.
    pub span: Span,
}

/// An expression used as a statement.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprStatement {
    /// The expression.
    pub expr: Expression,
    /// Source span.
    pub span: Span,
}

/// An expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// A binary operation: `a + b`, `x == y`, etc.
    BinaryOp {
        /// Left-hand operand.
        left: Box<Expression>,
        /// The operator.
        op: BinaryOp,
        /// Right-hand operand.
        right: Box<Expression>,
        /// Source span.
        span: Span,
    },
    /// A unary operation: `!x`, `-n`.
    UnaryOp {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expression>,
        /// Source span.
        span: Span,
    },
    /// A string literal (possibly with interpolation segments).
    StringLiteral {
        /// The string value (after escape processing, before interpolation).
        value: String,
        /// Source span.
        span: Span,
    },
    /// A floating-point number literal.
    NumberLiteral {
        /// The numeric value.
        value: f64,
        /// Source span.
        span: Span,
    },
    /// An integer literal.
    IntLiteral {
        /// The integer value.
        value: i64,
        /// Source span.
        span: Span,
    },
    /// A boolean literal (`true` / `false`).
    BoolLiteral {
        /// The boolean value.
        value: bool,
        /// Source span.
        span: Span,
    },
    /// A local variable reference: `%name`.
    Variable {
        /// The variable name (without `%`).
        name: String,
        /// Source span.
        span: Span,
    },
    /// A global variable reference: `%%name`.
    GlobalVariable {
        /// The variable name (without `%%`).
        name: String,
        /// Source span.
        span: Span,
    },
    /// A built-in identifier: `$nick`, `$1`, etc.
    BuiltinId {
        /// The identifier name (without `$`).
        name: String,
        /// Source span.
        span: Span,
    },
    /// A plain identifier reference.
    Identifier {
        /// The identifier name.
        name: String,
        /// Source span.
        span: Span,
    },
    /// A function call: `$name(args)`.
    FunctionCall {
        /// The function name (without `$`).
        name: String,
        /// The argument expressions.
        args: Vec<Expression>,
        /// Source span.
        span: Span,
    },
    /// An interpolated string with mixed literal and expression segments.
    Interpolated {
        /// The segments of the interpolated string.
        parts: Vec<StringPart>,
        /// Source span.
        span: Span,
    },
    /// A parenthesized expression.
    Grouped {
        /// The inner expression.
        expr: Box<Expression>,
        /// Source span.
        span: Span,
    },
}

impl Expression {
    /// Returns the span of this expression.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::BinaryOp { span, .. }
            | Self::UnaryOp { span, .. }
            | Self::StringLiteral { span, .. }
            | Self::NumberLiteral { span, .. }
            | Self::IntLiteral { span, .. }
            | Self::BoolLiteral { span, .. }
            | Self::Variable { span, .. }
            | Self::GlobalVariable { span, .. }
            | Self::BuiltinId { span, .. }
            | Self::Identifier { span, .. }
            | Self::FunctionCall { span, .. }
            | Self::Interpolated { span, .. }
            | Self::Grouped { span, .. } => *span,
        }
    }
}

/// A segment of an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// A literal text segment.
    Literal(String),
    /// An embedded expression (e.g., `$nick` or `$(expr)`).
    Expr(Expression),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `==`
    Eq,
    /// `!=`
    Neq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Lte,
    /// `>=`
    Gte,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `!` (logical negation)
    Not,
    /// `-` (numeric negation)
    Neg,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_item_span() {
        let alias = TopLevelItem::Alias(AliasDefinition {
            name: "test".to_string(),
            body: vec![],
            span: Span::new(0, 10),
        });
        assert_eq!(alias.span(), Span::new(0, 10));
    }

    #[test]
    fn statement_span() {
        let brk = Statement::Break(Span::new(5, 5));
        assert_eq!(brk.span(), Span::new(5, 5));

        let cont = Statement::Continue(Span::new(10, 8));
        assert_eq!(cont.span(), Span::new(10, 8));
    }

    #[test]
    fn expression_span() {
        let expr = Expression::IntLiteral {
            value: 42,
            span: Span::new(0, 2),
        };
        assert_eq!(expr.span(), Span::new(0, 2));
    }

    #[test]
    fn binary_op_variants() {
        // Ensure all binary ops are distinct
        let ops = [
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Mod,
            BinaryOp::Eq,
            BinaryOp::Neq,
            BinaryOp::Lt,
            BinaryOp::Gt,
            BinaryOp::Lte,
            BinaryOp::Gte,
            BinaryOp::And,
            BinaryOp::Or,
        ];
        for (i, a) in ops.iter().enumerate() {
            for (j, b) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn event_type_copy() {
        let et = EventType::Text;
        let et2 = et;
        assert_eq!(et, et2);
    }
}
