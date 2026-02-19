use crate::ast::{
    AliasDefinition, CommandStatement, ElseIfBranch, EventHandler, EventType, Expression,
    IfStatement, Script, SetStatement, Statement, TimerDefinition, TopLevelItem, VarDeclStatement,
    WhileStatement,
};
use crate::error::{ParseError, SourceLocation};
use crate::token::{Span, Token, TokenKind};

/// A recursive descent parser that consumes tokens and produces an AST.
///
/// The parser takes a `Vec<Token>` (produced by the lexer) and the original
/// source text (for error location computation). Its entry point is
/// [`Parser::parse`], which returns a [`Script`] containing all top-level
/// items.
pub struct Parser<'src> {
    /// The token stream.
    tokens: Vec<Token>,
    /// Current position in the token stream.
    pos: usize,
    /// The original source text (for computing error locations).
    source: &'src str,
}

impl<'src> Parser<'src> {
    /// Creates a new parser for the given token stream and source text.
    #[must_use]
    pub fn new(tokens: Vec<Token>, source: &'src str) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
        }
    }

    /// Parses the token stream into a [`Script`] AST node.
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] if the tokens do not form a valid script.
    pub fn parse(&mut self) -> Result<Script, ParseError> {
        let start_span = self.current_span();
        let mut items = Vec::new();

        self.skip_trivia();
        while !self.at_end() {
            let item = self.parse_top_level_item()?;
            items.push(item);
            self.skip_trivia();
        }

        let end_span = self.current_span();
        let span = if items.is_empty() {
            start_span
        } else {
            start_span.merge(end_span)
        };

        Ok(Script { items, span })
    }

    // -----------------------------------------------------------------------
    // Helper methods
    // -----------------------------------------------------------------------

    /// Returns a reference to the current token without consuming it.
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// Returns the kind of the current token.
    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    /// Returns the span of the current token.
    fn current_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    /// Consumes and returns the current token, advancing the position.
    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    /// Returns `true` if the current token is `Eof`.
    fn at_end(&self) -> bool {
        self.tokens[self.pos].kind == TokenKind::Eof
    }

    /// Checks if the current token matches the given kind (without consuming).
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.tokens[self.pos].kind) == std::mem::discriminant(kind)
    }

    /// Consumes the current token if it matches `kind`, otherwise returns an error.
    fn expect(&mut self, kind: &TokenKind, expected_desc: &str) -> Result<&Token, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof {
                expected: expected_desc.to_string(),
                span: self.current_span(),
                location: SourceLocation::from_offset(self.source, self.current_span().offset),
            });
        }
        if self.check(kind) {
            Ok(self.advance())
        } else {
            let found = Self::describe_token(self.peek());
            Err(ParseError::UnexpectedToken {
                expected: expected_desc.to_string(),
                found,
                span: self.current_span(),
                location: SourceLocation::from_offset(self.source, self.current_span().offset),
            })
        }
    }

    /// Skips newlines and comments (trivia tokens).
    fn skip_trivia(&mut self) {
        while !self.at_end() {
            match &self.tokens[self.pos].kind {
                TokenKind::Newline | TokenKind::Comment(_) => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
    }

    /// Returns a human-readable description of a token for error messages.
    fn describe_token(token: &Token) -> String {
        match &token.kind {
            TokenKind::Alias => "'alias'".to_string(),
            TokenKind::On => "'on'".to_string(),
            TokenKind::If => "'if'".to_string(),
            TokenKind::ElseIf => "'elseif'".to_string(),
            TokenKind::Else => "'else'".to_string(),
            TokenKind::While => "'while'".to_string(),
            TokenKind::Var => "'var'".to_string(),
            TokenKind::Set => "'set'".to_string(),
            TokenKind::Timer => "'timer'".to_string(),
            TokenKind::Return => "'return'".to_string(),
            TokenKind::Break => "'break'".to_string(),
            TokenKind::Continue => "'continue'".to_string(),
            TokenKind::True => "'true'".to_string(),
            TokenKind::False => "'false'".to_string(),
            TokenKind::StringLiteral(_) => "string literal".to_string(),
            TokenKind::NumberLiteral(_) => "number literal".to_string(),
            TokenKind::IntLiteral(_) => "integer literal".to_string(),
            TokenKind::Identifier(name) => format!("identifier '{name}'"),
            TokenKind::Variable(name) => format!("variable '%{name}'"),
            TokenKind::GlobalVariable(name) => format!("variable '%%{name}'"),
            TokenKind::BuiltinIdentifier(name) => format!("builtin '${name}'"),
            TokenKind::Plus => "'+'".to_string(),
            TokenKind::Minus => "'-'".to_string(),
            TokenKind::Star => "'*'".to_string(),
            TokenKind::Slash => "'/'".to_string(),
            TokenKind::Percent => "'%'".to_string(),
            TokenKind::Equal => "'='".to_string(),
            TokenKind::EqualEqual => "'=='".to_string(),
            TokenKind::BangEqual => "'!='".to_string(),
            TokenKind::Less => "'<'".to_string(),
            TokenKind::Greater => "'>'".to_string(),
            TokenKind::LessEqual => "'<='".to_string(),
            TokenKind::GreaterEqual => "'>='".to_string(),
            TokenKind::AmpAmp => "'&&'".to_string(),
            TokenKind::PipePipe => "'||'".to_string(),
            TokenKind::Bang => "'!'".to_string(),
            TokenKind::LeftBrace => "'{'".to_string(),
            TokenKind::RightBrace => "'}'".to_string(),
            TokenKind::LeftParen => "'('".to_string(),
            TokenKind::RightParen => "')'".to_string(),
            TokenKind::Comma => "','".to_string(),
            TokenKind::Colon => "':'".to_string(),
            TokenKind::Semicolon => "';'".to_string(),
            TokenKind::Newline => "newline".to_string(),
            TokenKind::Eof => "end of input".to_string(),
            TokenKind::Comment(_) => "comment".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Top-level parsing
    // -----------------------------------------------------------------------

    /// Parses a single top-level item (`alias`, `on`, or `timer`).
    fn parse_top_level_item(&mut self) -> Result<TopLevelItem, ParseError> {
        match self.peek_kind() {
            TokenKind::Alias => self.parse_alias().map(TopLevelItem::Alias),
            TokenKind::On => self.parse_event().map(TopLevelItem::Event),
            TokenKind::Timer => self.parse_timer().map(TopLevelItem::Timer),
            _ => {
                let found = Self::describe_token(self.peek());
                Err(ParseError::UnexpectedToken {
                    expected: "'alias', 'on', or 'timer'".to_string(),
                    found,
                    span: self.current_span(),
                    location: SourceLocation::from_offset(
                        self.source,
                        self.current_span().offset,
                    ),
                })
            }
        }
    }

    /// Parses an alias definition: `alias IDENT block` or `alias IDENT statement NEWLINE`.
    fn parse_alias(&mut self) -> Result<AliasDefinition, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'alias'

        // Parse the alias name
        let name = self.expect_identifier("alias name")?;

        self.skip_trivia();

        // Determine if block form or single-line form
        let body = if self.check(&TokenKind::LeftBrace) {
            self.parse_block()?
        } else {
            // Single-line form: parse one statement until newline
            let stmt = self.parse_statement()?;
            vec![stmt]
        };

        let end_span = if body.is_empty() {
            start_span
        } else {
            body.last().map_or(start_span, Statement::span)
        };

        Ok(AliasDefinition {
            name,
            body,
            span: start_span.merge(end_span),
        })
    }

    /// Parses an event handler: `on EVENT_TYPE:pattern block` or `on EVENT_TYPE block`.
    fn parse_event(&mut self) -> Result<EventHandler, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'on'

        // Parse the event type (an identifier like TEXT, JOIN, etc.)
        let event_type_token = self.peek().clone();
        let event_type_name = self.expect_identifier("event type")?;
        let event_type = Self::parse_event_type(&event_type_name, &event_type_token, self.source)?;

        // Check for optional `:pattern`
        let pattern = if self.check(&TokenKind::Colon) {
            self.advance(); // consume ':'
            self.parse_pattern()
        } else {
            "*".to_string()
        };

        self.skip_trivia();

        let body = self.parse_block()?;

        let end_span = self.current_span();
        Ok(EventHandler {
            event_type,
            pattern,
            body,
            span: start_span.merge(end_span),
        })
    }

    /// Parses a timer definition: `timer IDENT expression expression block`.
    fn parse_timer(&mut self) -> Result<TimerDefinition, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'timer'

        let name = self.expect_identifier("timer name")?;

        // Parse interval expression
        let interval = self.parse_primary_expression()?;

        // Parse repetitions expression
        let repetitions = self.parse_primary_expression()?;

        self.skip_trivia();

        let body = self.parse_block()?;

        let end_span = self.current_span();
        Ok(TimerDefinition {
            name,
            interval,
            repetitions,
            body,
            span: start_span.merge(end_span),
        })
    }

    /// Maps an event type name string to an [`EventType`] enum variant.
    fn parse_event_type(
        name: &str,
        token: &Token,
        source: &str,
    ) -> Result<EventType, ParseError> {
        match name.to_uppercase().as_str() {
            "TEXT" => Ok(EventType::Text),
            "JOIN" => Ok(EventType::Join),
            "PART" => Ok(EventType::Part),
            "KICK" => Ok(EventType::Kick),
            "QUIT" => Ok(EventType::Quit),
            "CONNECT" => Ok(EventType::Connect),
            "DISCONNECT" => Ok(EventType::Disconnect),
            "INVITE" => Ok(EventType::Invite),
            "NOTICE" => Ok(EventType::Notice),
            "NICK" => Ok(EventType::Nick),
            "TOPIC" => Ok(EventType::Topic),
            "MODE" => Ok(EventType::Mode),
            "CTCP" => Ok(EventType::Ctcp),
            "ACTION" => Ok(EventType::Action),
            "NUMERIC" => Ok(EventType::Numeric),
            _ => Err(ParseError::InvalidEventType {
                name: name.to_string(),
                span: token.span,
                location: SourceLocation::from_offset(source, token.span.offset),
            }),
        }
    }

    /// Parses a glob pattern after the `:` in an event handler.
    ///
    /// Consumes tokens until a `{`, newline, or EOF is reached,
    /// combining them into a pattern string.
    fn parse_pattern(&mut self) -> String {
        let mut pattern = String::new();

        loop {
            match self.peek_kind() {
                TokenKind::LeftBrace | TokenKind::Newline | TokenKind::Eof => break,
                TokenKind::Star => {
                    pattern.push('*');
                    self.advance();
                }
                TokenKind::Identifier(name) => {
                    pattern.push_str(name);
                    self.advance();
                }
                TokenKind::StringLiteral(s) => {
                    pattern.push_str(s);
                    self.advance();
                }
                TokenKind::IntLiteral(n) => {
                    pattern.push_str(&n.to_string());
                    self.advance();
                }
                TokenKind::Colon => {
                    pattern.push(':');
                    self.advance();
                }
                TokenKind::Minus => {
                    pattern.push('-');
                    self.advance();
                }
                _ => {
                    // Treat any other token as pattern text using the source span
                    let span = self.current_span();
                    let text = &self.source[span.offset..span.end()];
                    pattern.push_str(text);
                    self.advance();
                }
            }
        }

        // Trim trailing whitespace that might have been included
        let trimmed = pattern.trim().to_string();
        if trimmed.is_empty() {
            "*".to_string()
        } else {
            trimmed
        }
    }

    // -----------------------------------------------------------------------
    // Block and statement parsing
    // -----------------------------------------------------------------------

    /// Parses a block: `{ statement* }`.
    fn parse_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        self.expect(&TokenKind::LeftBrace, "'{'")
            .map(|t| t.span)?;

        let mut stmts = Vec::new();
        self.skip_trivia();

        while !self.check(&TokenKind::RightBrace) && !self.at_end() {
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            self.skip_trivia();
        }

        self.expect(&TokenKind::RightBrace, "'}'")
            .map(|t| t.span)?;

        Ok(stmts)
    }

    /// Parses a single statement.
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Var => self.parse_var_statement(),
            TokenKind::Set => self.parse_set_statement(),
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::Break => {
                let span = self.current_span();
                self.advance();
                Ok(Statement::Break(span))
            }
            TokenKind::Continue => {
                let span = self.current_span();
                self.advance();
                Ok(Statement::Continue(span))
            }
            TokenKind::Slash | TokenKind::Identifier(_) => self.parse_command_statement(),
            _ => {
                let found = Self::describe_token(self.peek());
                Err(ParseError::UnexpectedToken {
                    expected: "statement".to_string(),
                    found,
                    span: self.current_span(),
                    location: SourceLocation::from_offset(
                        self.source,
                        self.current_span().offset,
                    ),
                })
            }
        }
    }

    /// Parses a return statement: `return [expr]`.
    fn parse_return_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'return'

        // Check if there's an expression following (before newline/rbrace/eof)
        let value = match self.peek_kind() {
            TokenKind::Newline | TokenKind::RightBrace | TokenKind::Eof | TokenKind::Comment(_) => {
                None
            }
            _ => Some(self.parse_primary_expression()?),
        };

        let end_span = value.as_ref().map_or(start_span, Expression::span);

        Ok(Statement::Return(crate::ast::ReturnStatement {
            value,
            span: start_span.merge(end_span),
        }))
    }

    /// Parses an if/elseif/else statement.
    fn parse_if_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'if'

        self.expect(&TokenKind::LeftParen, "'('")?;
        let condition = self.parse_expression_until_rparen()?;
        self.expect(&TokenKind::RightParen, "')'")?;

        self.skip_trivia();
        let then_body = self.parse_block()?;

        let mut elseif_branches = Vec::new();
        let mut else_body = None;

        loop {
            self.skip_trivia();
            if self.check(&TokenKind::ElseIf) {
                let branch_start = self.current_span();
                self.advance(); // consume 'elseif'

                self.expect(&TokenKind::LeftParen, "'('")?;
                let branch_cond = self.parse_expression_until_rparen()?;
                self.expect(&TokenKind::RightParen, "')'")?;

                self.skip_trivia();
                let branch_body = self.parse_block()?;

                let branch_end = self.current_span();
                elseif_branches.push(ElseIfBranch {
                    condition: branch_cond,
                    body: branch_body,
                    span: branch_start.merge(branch_end),
                });
            } else if self.check(&TokenKind::Else) {
                self.advance(); // consume 'else'
                self.skip_trivia();
                else_body = Some(self.parse_block()?);
                break;
            } else {
                break;
            }
        }

        let end_span = self.current_span();
        Ok(Statement::If(IfStatement {
            condition,
            then_body,
            elseif_branches,
            else_body,
            span: start_span.merge(end_span),
        }))
    }

    /// Parses a while loop: `while (<expr>) { <stmts> }`.
    fn parse_while_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'while'

        self.expect(&TokenKind::LeftParen, "'('")?;
        let condition = self.parse_expression_until_rparen()?;
        self.expect(&TokenKind::RightParen, "')'")?;

        self.skip_trivia();
        let body = self.parse_block()?;

        let end_span = self.current_span();
        Ok(Statement::While(WhileStatement {
            condition,
            body,
            span: start_span.merge(end_span),
        }))
    }

    /// Parses a variable declaration: `var %name = <expr>`.
    fn parse_var_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'var'

        let (name, global) = self.expect_variable("variable name")?;

        self.expect(&TokenKind::Equal, "'='")?;

        let value = self.parse_primary_expression()?;
        let end_span = value.span();

        Ok(Statement::VarDecl(VarDeclStatement {
            name,
            global,
            value,
            span: start_span.merge(end_span),
        }))
    }

    /// Parses a set statement: `set %name <expr>`.
    fn parse_set_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();
        self.advance(); // consume 'set'

        let (name, global) = self.expect_variable("variable name")?;

        let value = self.parse_primary_expression()?;
        let end_span = value.span();

        Ok(Statement::Set(SetStatement {
            name,
            global,
            value,
            span: start_span.merge(end_span),
        }))
    }

    /// Parses a command statement: `[/]identifier args...`.
    fn parse_command_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.current_span();

        // Optional leading slash
        if self.check(&TokenKind::Slash) {
            self.advance();
        }

        // Command name
        let name = self.expect_identifier("command name")?;

        // Parse arguments until end of line or block delimiter
        let mut args = Vec::new();
        loop {
            match self.peek_kind() {
                TokenKind::Newline
                | TokenKind::RightBrace
                | TokenKind::Eof
                | TokenKind::Comment(_) => break,
                _ => {
                    let expr = self.parse_primary_expression()?;
                    args.push(expr);
                }
            }
        }

        let end_span = args
            .last()
            .map_or(Span::new(start_span.offset, name.len()), Expression::span);

        Ok(Statement::Command(CommandStatement {
            name,
            args,
            span: start_span.merge(end_span),
        }))
    }

    // -----------------------------------------------------------------------
    // Expression parsing (primary only — full expression parsing in T235)
    // -----------------------------------------------------------------------

    /// Parses a primary expression (literals, variables, identifiers).
    ///
    /// This is a stub for the initial parser. Full expression parsing
    /// with operator precedence will be implemented in a later ticket.
    fn parse_primary_expression(&mut self) -> Result<Expression, ParseError> {
        let token = self.peek().clone();
        match &token.kind {
            TokenKind::IntLiteral(value) => {
                let value = *value;
                let span = self.current_span();
                self.advance();
                Ok(Expression::IntLiteral { value, span })
            }
            TokenKind::NumberLiteral(value) => {
                let value = *value;
                let span = self.current_span();
                self.advance();
                Ok(Expression::NumberLiteral { value, span })
            }
            TokenKind::StringLiteral(value) => {
                let value = value.clone();
                let span = self.current_span();
                self.advance();
                Ok(Expression::StringLiteral { value, span })
            }
            TokenKind::True => {
                let span = self.current_span();
                self.advance();
                Ok(Expression::BoolLiteral { value: true, span })
            }
            TokenKind::False => {
                let span = self.current_span();
                self.advance();
                Ok(Expression::BoolLiteral { value: false, span })
            }
            TokenKind::Variable(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.advance();
                Ok(Expression::Variable { name, span })
            }
            TokenKind::GlobalVariable(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.advance();
                Ok(Expression::GlobalVariable { name, span })
            }
            TokenKind::BuiltinIdentifier(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.advance();
                Ok(Expression::BuiltinId { name, span })
            }
            TokenKind::Identifier(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.advance();
                Ok(Expression::Identifier { name, span })
            }
            TokenKind::LeftParen => {
                let start_span = self.current_span();
                self.advance(); // consume '('
                let inner = self.parse_primary_expression()?;
                let end_span = self
                    .expect(&TokenKind::RightParen, "')'")
                    .map(|t| t.span)?;
                Ok(Expression::Grouped {
                    expr: Box::new(inner),
                    span: start_span.merge(end_span),
                })
            }
            _ => {
                let found = Self::describe_token(&token);
                Err(ParseError::UnexpectedToken {
                    expected: "expression".to_string(),
                    found,
                    span: self.current_span(),
                    location: SourceLocation::from_offset(
                        self.source,
                        self.current_span().offset,
                    ),
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Utility helpers
    // -----------------------------------------------------------------------

    /// Expects the current token to be a variable (`%name` or `%%name`).
    ///
    /// Returns `(name, is_global)`.
    fn expect_variable(&mut self, desc: &str) -> Result<(String, bool), ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof {
                expected: desc.to_string(),
                span: self.current_span(),
                location: SourceLocation::from_offset(self.source, self.current_span().offset),
            });
        }
        match &self.tokens[self.pos].kind {
            TokenKind::Variable(name) => {
                let name = name.clone();
                self.advance();
                Ok((name, false))
            }
            TokenKind::GlobalVariable(name) => {
                let name = name.clone();
                self.advance();
                Ok((name, true))
            }
            _ => {
                let found = Self::describe_token(self.peek());
                Err(ParseError::UnexpectedToken {
                    expected: desc.to_string(),
                    found,
                    span: self.current_span(),
                    location: SourceLocation::from_offset(
                        self.source,
                        self.current_span().offset,
                    ),
                })
            }
        }
    }

    /// Parses expressions inside parentheses for conditions.
    ///
    /// Collects primary expressions until `)` is reached. For this basic
    /// version, if multiple tokens appear (e.g., `%x == 1`), they are
    /// combined into a flat list with the first as the "main" expression.
    /// Full operator-precedence parsing comes in T235.
    fn parse_expression_until_rparen(&mut self) -> Result<Expression, ParseError> {
        // For now, parse a single primary expression.
        // T235 will replace this with proper operator-precedence parsing.
        self.parse_primary_expression()
    }

    /// Expects the current token to be an identifier and returns its name.
    fn expect_identifier(&mut self, desc: &str) -> Result<String, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof {
                expected: desc.to_string(),
                span: self.current_span(),
                location: SourceLocation::from_offset(self.source, self.current_span().offset),
            });
        }
        if let TokenKind::Identifier(name) = &self.tokens[self.pos].kind {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            let found = Self::describe_token(self.peek());
            Err(ParseError::UnexpectedToken {
                expected: desc.to_string(),
                found,
                span: self.current_span(),
                location: SourceLocation::from_offset(
                    self.source,
                    self.current_span().offset,
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests;
