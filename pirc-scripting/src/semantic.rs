use std::collections::{HashMap, HashSet};

use crate::ast::{
    AliasDefinition, EventHandler, Expression, Script, Statement, StringPart, TimerDefinition,
    TopLevelItem,
};
use crate::error::{SemanticError, SemanticWarning, SourceLocation};
use crate::token::Span;

/// Known built-in identifiers (without the `$` prefix).
const KNOWN_BUILTINS: &[&str] = &[
    "nick", "chan", "me", "server", "time", "date", "address", "target", "text", "host", "site",
    "fulladdress", "network", "port", "ip", "version", "os", "idle", "online", "away", "null",
    "true", "false", "cr", "lf", "crlf", "tab", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0",
];

/// The result of semantic analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticResult {
    /// Fatal errors that prevent script execution.
    pub errors: Vec<SemanticError>,
    /// Non-fatal warnings (informational).
    pub warnings: Vec<SemanticWarning>,
}

impl SemanticResult {
    /// Returns `true` if there are no errors (warnings are allowed).
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns `true` if there are any errors.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns `true` if there are any warnings.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// Walks a parsed AST and collects semantic errors and warnings.
pub struct SemanticAnalyzer<'src> {
    source: &'src str,
    errors: Vec<SemanticError>,
    warnings: Vec<SemanticWarning>,
    /// Set of known built-in identifiers for fast lookup.
    known_builtins: HashSet<&'static str>,
}

impl<'src> SemanticAnalyzer<'src> {
    /// Creates a new analyzer for the given source text.
    #[must_use]
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            errors: Vec::new(),
            warnings: Vec::new(),
            known_builtins: KNOWN_BUILTINS.iter().copied().collect(),
        }
    }

    /// Analyzes the given script AST and returns the result.
    #[must_use]
    pub fn analyze(mut self, script: &Script) -> SemanticResult {
        self.check_duplicates(script);

        for item in &script.items {
            match item {
                TopLevelItem::Alias(alias) => self.analyze_alias(alias),
                TopLevelItem::Event(event) => self.analyze_event(event),
                TopLevelItem::Timer(timer) => self.analyze_timer(timer),
            }
        }

        SemanticResult {
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    /// Checks for duplicate alias names and duplicate event handler
    /// (`event_type` + pattern) pairs across the entire script.
    fn check_duplicates(&mut self, script: &Script) {
        let mut alias_names: HashMap<&str, Span> = HashMap::new();
        let mut event_keys: HashMap<String, Span> = HashMap::new();

        for item in &script.items {
            match item {
                TopLevelItem::Alias(alias) => {
                    if let Some(&_prev_span) = alias_names.get(alias.name.as_str()) {
                        self.warnings.push(SemanticWarning::DuplicateDefinition {
                            name: alias.name.clone(),
                            span: alias.span,
                            location: SourceLocation::from_offset(self.source, alias.span.offset),
                        });
                    } else {
                        alias_names.insert(&alias.name, alias.span);
                    }
                }
                TopLevelItem::Event(event) => {
                    let key = format!("{:?}:{}", event.event_type, event.pattern);
                    if let Some(&_prev_span) = event_keys.get(&key) {
                        let name = format!("{:?}:{}", event.event_type, event.pattern);
                        self.warnings.push(SemanticWarning::DuplicateDefinition {
                            name,
                            span: event.span,
                            location: SourceLocation::from_offset(self.source, event.span.offset),
                        });
                    } else {
                        event_keys.insert(key, event.span);
                    }
                }
                TopLevelItem::Timer(_) => {}
            }
        }
    }

    fn analyze_alias(&mut self, alias: &AliasDefinition) {
        let mut declared_locals: HashSet<String> = HashSet::new();
        self.analyze_body(&alias.body, &mut declared_locals, false);
    }

    fn analyze_event(&mut self, event: &EventHandler) {
        let mut declared_locals: HashSet<String> = HashSet::new();
        self.analyze_body(&event.body, &mut declared_locals, false);
    }

    fn analyze_timer(&mut self, timer: &TimerDefinition) {
        let mut declared_locals: HashSet<String> = HashSet::new();
        self.analyze_expression(&timer.interval, &declared_locals);
        self.analyze_expression(&timer.repetitions, &declared_locals);
        self.analyze_body(&timer.body, &mut declared_locals, false);
    }

    /// Analyzes a list of statements within a body context.
    ///
    /// `in_loop` tracks whether we are inside a while loop (for break/continue
    /// validation).
    fn analyze_body(
        &mut self,
        stmts: &[Statement],
        declared_locals: &mut HashSet<String>,
        in_loop: bool,
    ) {
        for stmt in stmts {
            self.analyze_statement(stmt, declared_locals, in_loop);
        }
    }

    fn analyze_statement(
        &mut self,
        stmt: &Statement,
        declared_locals: &mut HashSet<String>,
        in_loop: bool,
    ) {
        match stmt {
            Statement::VarDecl(decl) => {
                self.analyze_expression(&decl.value, declared_locals);
                if !decl.global {
                    declared_locals.insert(decl.name.clone());
                }
            }
            Statement::Set(set) => {
                self.analyze_expression(&set.value, declared_locals);
                // `set` on a local var that was never declared is also fine
                // (implicitly declares it via assignment, like mIRC)
                if !set.global {
                    declared_locals.insert(set.name.clone());
                }
            }
            Statement::If(if_stmt) => {
                self.analyze_expression(&if_stmt.condition, declared_locals);
                self.analyze_body(&if_stmt.then_body, declared_locals, in_loop);
                for branch in &if_stmt.elseif_branches {
                    self.analyze_expression(&branch.condition, declared_locals);
                    self.analyze_body(&branch.body, declared_locals, in_loop);
                }
                if let Some(else_body) = &if_stmt.else_body {
                    self.analyze_body(else_body, declared_locals, in_loop);
                }
            }
            Statement::While(while_stmt) => {
                self.analyze_expression(&while_stmt.condition, declared_locals);
                self.analyze_body(&while_stmt.body, declared_locals, true);
            }
            Statement::Break(span) => {
                if !in_loop {
                    self.errors.push(SemanticError::BreakOutsideLoop {
                        keyword: "break".to_string(),
                        span: *span,
                        location: SourceLocation::from_offset(self.source, span.offset),
                    });
                }
            }
            Statement::Continue(span) => {
                if !in_loop {
                    self.errors.push(SemanticError::BreakOutsideLoop {
                        keyword: "continue".to_string(),
                        span: *span,
                        location: SourceLocation::from_offset(self.source, span.offset),
                    });
                }
            }
            Statement::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.analyze_expression(value, declared_locals);
                }
            }
            Statement::Command(cmd) => {
                for arg in &cmd.args {
                    self.analyze_expression(arg, declared_locals);
                }
            }
            Statement::ExprStatement(expr_stmt) => {
                self.analyze_expression(&expr_stmt.expr, declared_locals);
            }
        }
    }

    fn analyze_expression(&mut self, expr: &Expression, declared_locals: &HashSet<String>) {
        match expr {
            Expression::Variable { name, span } => {
                if !declared_locals.contains(name) {
                    self.warnings.push(SemanticWarning::UndeclaredLocal {
                        name: name.clone(),
                        span: *span,
                        location: SourceLocation::from_offset(self.source, span.offset),
                    });
                }
            }
            Expression::BuiltinId { name, span } => {
                if !self.known_builtins.contains(name.as_str()) {
                    self.warnings.push(SemanticWarning::UnknownBuiltin {
                        name: name.clone(),
                        span: *span,
                        location: SourceLocation::from_offset(self.source, span.offset),
                    });
                }
            }
            Expression::FunctionCall { name, args, span } => {
                // Validate function name as a built-in
                if !self.known_builtins.contains(name.as_str()) {
                    self.warnings.push(SemanticWarning::UnknownBuiltin {
                        name: name.clone(),
                        span: *span,
                        location: SourceLocation::from_offset(self.source, span.offset),
                    });
                }
                for arg in args {
                    self.analyze_expression(arg, declared_locals);
                }
            }
            Expression::BinaryOp { left, right, .. } => {
                self.analyze_expression(left, declared_locals);
                self.analyze_expression(right, declared_locals);
            }
            Expression::UnaryOp { operand, .. } => {
                self.analyze_expression(operand, declared_locals);
            }
            Expression::Grouped { expr, .. } => {
                self.analyze_expression(expr, declared_locals);
            }
            Expression::Interpolated { parts, .. } => {
                for part in parts {
                    if let StringPart::Expr(inner) = part {
                        self.analyze_expression(inner, declared_locals);
                    }
                }
            }
            // Global variables, literals, and plain identifiers need no validation.
            Expression::GlobalVariable { .. }
            | Expression::StringLiteral { .. }
            | Expression::NumberLiteral { .. }
            | Expression::IntLiteral { .. }
            | Expression::BoolLiteral { .. }
            | Expression::Identifier { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// Helper: parse source and run semantic analysis.
    fn analyze(source: &str) -> SemanticResult {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().expect("lexer should succeed");
        let mut parser = Parser::new(tokens, source);
        let script = parser.parse().expect("parser should succeed");
        let analyzer = SemanticAnalyzer::new(source);
        analyzer.analyze(&script)
    }

    #[test]
    fn valid_script_no_diagnostics() {
        let src = r#"
alias greet {
    var %name = $nick
    msg $chan "Hello %name"
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn undeclared_local_variable_warns() {
        let src = r#"
alias test {
    msg $chan %x
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            SemanticWarning::UndeclaredLocal { name, .. } if name == "x"
        ));
    }

    #[test]
    fn declared_local_variable_no_warning() {
        let src = r#"
alias test {
    var %x = 10
    msg $chan %x
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn set_local_declares_implicitly() {
        let src = r#"
alias test {
    set %x 10
    msg $chan %x
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn global_variable_no_warning() {
        let src = r#"
alias test {
    msg $chan %%count
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn break_outside_loop_errors() {
        let src = r#"
alias test {
    break
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            &result.errors[0],
            SemanticError::BreakOutsideLoop { keyword, .. } if keyword == "break"
        ));
    }

    #[test]
    fn continue_outside_loop_errors() {
        let src = r#"
alias test {
    continue
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            &result.errors[0],
            SemanticError::BreakOutsideLoop { keyword, .. } if keyword == "continue"
        ));
    }

    #[test]
    fn break_inside_loop_ok() {
        let src = r#"
alias test {
    var %i = 0
    while (%i < 10) {
        break
    }
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn continue_inside_loop_ok() {
        let src = r#"
alias test {
    var %i = 0
    while (%i < 10) {
        continue
    }
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn duplicate_alias_warns() {
        let src = r#"
alias greet {
    msg $chan "hello"
}
alias greet {
    msg $chan "hi"
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            SemanticWarning::DuplicateDefinition { name, .. } if name == "greet"
        ));
    }

    #[test]
    fn duplicate_event_handler_warns() {
        let src = r#"
on JOIN:* {
    msg $chan "welcome"
}
on JOIN:* {
    msg $chan "hi there"
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            SemanticWarning::DuplicateDefinition { .. }
        ));
    }

    #[test]
    fn unknown_builtin_warns() {
        let src = r#"
alias test {
    msg $chan $foobar
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            SemanticWarning::UnknownBuiltin { name, .. } if name == "foobar"
        ));
    }

    #[test]
    fn known_builtins_no_warning() {
        let src = r#"
alias test {
    msg $chan $nick
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn multiple_errors_collected() {
        let src = r#"
alias test {
    break
    continue
    msg $chan %undeclared
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 2);
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn nested_while_break_ok() {
        let src = r#"
alias test {
    var %i = 0
    while (%i < 10) {
        var %j = 0
        while (%j < 5) {
            break
        }
        continue
    }
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn interpolated_string_checks_vars() {
        let src = r#"
alias test {
    msg $chan "hello %undeclared and $unknown_id"
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(result.has_warnings());
        // Should warn about both %undeclared and $unknown_id
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn if_else_body_analyzed() {
        let src = r#"
alias test {
    if ($nick == "admin") {
        break
    } else {
        continue
    }
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn return_in_alias_ok() {
        let src = r#"
alias calc {
    var %result = 42
    return %result
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn event_handler_body_analyzed() {
        let src = r#"
on TEXT:*hello* {
    msg $chan "Hi $nick!"
    break
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(
            &result.errors[0],
            SemanticError::BreakOutsideLoop { keyword, .. } if keyword == "break"
        ));
    }

    #[test]
    fn timer_expressions_analyzed() {
        let src = r#"
timer mytimer 60 0 {
    msg $chan "tick"
}
"#;
        let result = analyze(src);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn semantic_result_methods() {
        let result = SemanticResult {
            errors: vec![],
            warnings: vec![],
        };
        assert!(result.is_ok());
        assert!(!result.has_errors());
        assert!(!result.has_warnings());
    }

    #[test]
    fn warning_display_and_span() {
        let warning = SemanticWarning::UndeclaredLocal {
            name: "x".to_string(),
            span: Span::new(5, 2),
            location: SourceLocation::new(1, 6),
        };
        assert_eq!(warning.to_string(), "undeclared local variable '%x' at 1:6");
        assert_eq!(warning.span(), Span::new(5, 2));

        let warning = SemanticWarning::UnknownBuiltin {
            name: "foo".to_string(),
            span: Span::new(10, 4),
            location: SourceLocation::new(2, 3),
        };
        assert_eq!(
            warning.to_string(),
            "unknown built-in identifier '$foo' at 2:3"
        );

        let warning = SemanticWarning::DuplicateDefinition {
            name: "greet".to_string(),
            span: Span::new(20, 5),
            location: SourceLocation::new(3, 1),
        };
        assert_eq!(
            warning.to_string(),
            "duplicate definition 'greet' at 3:1"
        );

        let warning = SemanticWarning::UnrecognizedEventType {
            name: "FOOBAR".to_string(),
            span: Span::new(30, 6),
            location: SourceLocation::new(4, 1),
        };
        assert_eq!(
            warning.to_string(),
            "unrecognized event type 'FOOBAR' at 4:1"
        );
        assert_eq!(warning.span(), Span::new(30, 6));
    }

    #[test]
    fn elseif_branch_analyzed() {
        let src = r#"
alias test {
    if ($nick == "a") {
        msg $chan "a"
    } elseif ($nick == "b") {
        break
    } else {
        msg $chan "other"
    }
}
"#;
        let result = analyze(src);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
    }
}
