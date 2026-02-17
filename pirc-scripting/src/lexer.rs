use crate::error::{LexError, SourceLocation};
use crate::token::{Span, Token, TokenKind};

/// A lexer that converts source text into a stream of tokens.
pub struct Lexer<'src> {
    /// The full source text.
    source: &'src str,
    /// The source as bytes for efficient access.
    bytes: &'src [u8],
    /// Current byte position in the source.
    pos: usize,
}

impl<'src> Lexer<'src> {
    /// Creates a new lexer for the given source text.
    #[must_use]
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    /// Tokenizes the entire source, returning all tokens (ending with `Eof`).
    ///
    /// # Errors
    ///
    /// Returns a [`LexError`] if the source contains invalid tokens
    /// (unterminated strings, invalid escapes, unexpected characters).
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Returns the next token from the source.
    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace();

        if self.pos >= self.bytes.len() {
            return Ok(Token::new(TokenKind::Eof, Span::new(self.pos, 0)));
        }

        let start = self.pos;
        let ch = self.bytes[start];

        match ch {
            b'\n' => {
                self.pos += 1;
                Ok(Token::new(TokenKind::Newline, Span::new(start, 1)))
            }
            b'\r' => {
                self.pos += 1;
                // Handle \r\n as a single newline
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                    self.pos += 1;
                }
                Ok(Token::new(
                    TokenKind::Newline,
                    Span::new(start, self.pos - start),
                ))
            }
            b';' => Ok(self.lex_comment()),
            b'"' => self.lex_string(),
            b'%' if self.peek_at(1).is_some_and(|c| c == b'%') => {
                Ok(self.lex_global_variable())
            }
            b'%' if self.peek_at(1).is_some_and(is_ident_start) => Ok(self.lex_variable()),
            b'%' => Ok(self.single_token(TokenKind::Percent)),
            b'$' if self.peek_at(1).is_some_and(|c| is_ident_start(c) || c.is_ascii_digit()) => {
                Ok(self.lex_builtin_identifier())
            }
            b'/' if self.peek_at(1).is_some_and(is_ident_start) => {
                Ok(self.single_token(TokenKind::Slash))
            }
            b'0'..=b'9' => self.lex_number(),
            c if is_ident_start(c) => Ok(self.lex_identifier_or_keyword()),
            // Operators and delimiters
            b'+' => Ok(self.single_token(TokenKind::Plus)),
            b'-' => Ok(self.single_token(TokenKind::Minus)),
            b'*' => Ok(self.single_token(TokenKind::Star)),
            b'/' => Ok(self.single_token(TokenKind::Slash)),
            b'{' => Ok(self.single_token(TokenKind::LeftBrace)),
            b'}' => Ok(self.single_token(TokenKind::RightBrace)),
            b'(' => Ok(self.single_token(TokenKind::LeftParen)),
            b')' => Ok(self.single_token(TokenKind::RightParen)),
            b',' => Ok(self.single_token(TokenKind::Comma)),
            b':' => Ok(self.single_token(TokenKind::Colon)),
            b'=' if self.peek_at(1) == Some(b'=') => Ok(self.double_token(TokenKind::EqualEqual)),
            b'=' => Ok(self.single_token(TokenKind::Equal)),
            b'!' if self.peek_at(1) == Some(b'=') => Ok(self.double_token(TokenKind::BangEqual)),
            b'!' => Ok(self.single_token(TokenKind::Bang)),
            b'<' if self.peek_at(1) == Some(b'=') => Ok(self.double_token(TokenKind::LessEqual)),
            b'<' => Ok(self.single_token(TokenKind::Less)),
            b'>' if self.peek_at(1) == Some(b'=') => {
                Ok(self.double_token(TokenKind::GreaterEqual))
            }
            b'>' => Ok(self.single_token(TokenKind::Greater)),
            b'&' if self.peek_at(1) == Some(b'&') => Ok(self.double_token(TokenKind::AmpAmp)),
            b'|' if self.peek_at(1) == Some(b'|') => Ok(self.double_token(TokenKind::PipePipe)),
            _ => {
                // Decode the actual character for the error message
                let ch = self.source[self.pos..].chars().next().unwrap_or('\0');
                let len = ch.len_utf8();
                self.pos += len;
                Err(LexError::UnexpectedCharacter {
                    ch,
                    span: Span::new(start, len),
                    location: SourceLocation::from_offset(self.source, start),
                })
            }
        }
    }

    /// Skips spaces and tabs (but not newlines).
    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' => self.pos += 1,
                _ => break,
            }
        }
    }

    /// Peeks at the byte at `self.pos + offset`.
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    /// Produces a single-character token and advances the position.
    fn single_token(&mut self, kind: TokenKind) -> Token {
        let start = self.pos;
        self.pos += 1;
        Token::new(kind, Span::new(start, 1))
    }

    /// Produces a two-character token and advances the position.
    fn double_token(&mut self, kind: TokenKind) -> Token {
        let start = self.pos;
        self.pos += 2;
        Token::new(kind, Span::new(start, 2))
    }

    /// Lexes a line comment starting with `;`.
    fn lex_comment(&mut self) -> Token {
        let start = self.pos;
        self.pos += 1; // skip `;`
        let content_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        let content = self.source[content_start..self.pos].trim_end().to_string();
        Token::new(
            TokenKind::Comment(content),
            Span::new(start, self.pos - start),
        )
    }

    /// Lexes a string literal enclosed in double quotes.
    fn lex_string(&mut self) -> Result<Token, LexError> {
        let start = self.pos;
        self.pos += 1; // skip opening `"`
        let mut value = String::new();

        loop {
            if self.pos >= self.bytes.len() {
                return Err(LexError::UnterminatedString {
                    span: Span::new(start, self.pos - start),
                    location: SourceLocation::from_offset(self.source, start),
                });
            }

            match self.bytes[self.pos] {
                b'"' => {
                    self.pos += 1; // skip closing `"`
                    return Ok(Token::new(
                        TokenKind::StringLiteral(value),
                        Span::new(start, self.pos - start),
                    ));
                }
                b'\\' => {
                    let esc_start = self.pos;
                    self.pos += 1; // skip `\`
                    if self.pos >= self.bytes.len() {
                        return Err(LexError::UnterminatedString {
                            span: Span::new(start, self.pos - start),
                            location: SourceLocation::from_offset(self.source, start),
                        });
                    }
                    match self.bytes[self.pos] {
                        b'n' => {
                            value.push('\n');
                            self.pos += 1;
                        }
                        b't' => {
                            value.push('\t');
                            self.pos += 1;
                        }
                        b'r' => {
                            value.push('\r');
                            self.pos += 1;
                        }
                        b'\\' => {
                            value.push('\\');
                            self.pos += 1;
                        }
                        b'"' => {
                            value.push('"');
                            self.pos += 1;
                        }
                        _ => {
                            let ch =
                                self.source[self.pos..].chars().next().unwrap_or('\0');
                            self.pos += ch.len_utf8();
                            return Err(LexError::InvalidEscape {
                                ch,
                                span: Span::new(esc_start, self.pos - esc_start),
                                location: SourceLocation::from_offset(
                                    self.source, esc_start,
                                ),
                            });
                        }
                    }
                }
                b'\n' => {
                    return Err(LexError::UnterminatedString {
                        span: Span::new(start, self.pos - start),
                        location: SourceLocation::from_offset(self.source, start),
                    });
                }
                _ => {
                    let ch = self.source[self.pos..].chars().next().unwrap_or('\0');
                    value.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// Lexes a local variable `%name`.
    fn lex_variable(&mut self) -> Token {
        let start = self.pos;
        self.pos += 1; // skip `%`
        let name_start = self.pos;
        self.advance_while_ident();
        let name = self.source[name_start..self.pos].to_string();
        Token::new(
            TokenKind::Variable(name),
            Span::new(start, self.pos - start),
        )
    }

    /// Lexes a global variable `%%name`.
    fn lex_global_variable(&mut self) -> Token {
        let start = self.pos;
        self.pos += 2; // skip `%%`
        let name_start = self.pos;
        self.advance_while_ident();
        let name = self.source[name_start..self.pos].to_string();
        Token::new(
            TokenKind::GlobalVariable(name),
            Span::new(start, self.pos - start),
        )
    }

    /// Lexes a built-in identifier `$name` or `$1`.
    fn lex_builtin_identifier(&mut self) -> Token {
        let start = self.pos;
        self.pos += 1; // skip `$`
        let name_start = self.pos;
        self.advance_while_ident();
        let name = self.source[name_start..self.pos].to_string();
        Token::new(
            TokenKind::BuiltinIdentifier(name),
            Span::new(start, self.pos - start),
        )
    }

    /// Lexes a number literal (integer or float).
    fn lex_number(&mut self) -> Result<Token, LexError> {
        let start = self.pos;
        self.advance_while(|b| b.is_ascii_digit());

        // Check for a decimal point followed by digit(s)
        let is_float = self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self.peek_at(1).is_some_and(|b| b.is_ascii_digit());

        if is_float {
            self.pos += 1; // skip `.`
            self.advance_while(|b| b.is_ascii_digit());
        }

        let text = &self.source[start..self.pos];
        let span = Span::new(start, self.pos - start);

        if is_float {
            match text.parse::<f64>() {
                Ok(val) => Ok(Token::new(TokenKind::NumberLiteral(val), span)),
                Err(e) => Err(LexError::InvalidNumber {
                    message: e.to_string(),
                    span,
                    location: SourceLocation::from_offset(self.source, start),
                }),
            }
        } else {
            match text.parse::<i64>() {
                Ok(val) => Ok(Token::new(TokenKind::IntLiteral(val), span)),
                Err(e) => Err(LexError::InvalidNumber {
                    message: e.to_string(),
                    span,
                    location: SourceLocation::from_offset(self.source, start),
                }),
            }
        }
    }

    /// Lexes an identifier or keyword.
    fn lex_identifier_or_keyword(&mut self) -> Token {
        let start = self.pos;
        self.advance_while_ident();
        let text = &self.source[start..self.pos];
        let span = Span::new(start, self.pos - start);

        let kind =
            TokenKind::keyword(text).unwrap_or_else(|| TokenKind::Identifier(text.to_string()));

        Token::new(kind, span)
    }

    /// Advances while the current byte matches `predicate`.
    fn advance_while(&mut self, predicate: impl Fn(u8) -> bool) {
        while self.pos < self.bytes.len() && predicate(self.bytes[self.pos]) {
            self.pos += 1;
        }
    }

    /// Advances while the current byte is an identifier character.
    fn advance_while_ident(&mut self) {
        self.advance_while(is_ident_continue);
    }
}

/// Returns true if the byte can start an identifier (`[a-zA-Z_]`).
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Returns true if the byte can continue an identifier (`[a-zA-Z0-9_-]`).
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: tokenize and return just the kinds.
    fn token_kinds(source: &str) -> Result<Vec<TokenKind>, LexError> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        Ok(tokens.into_iter().map(|t| t.kind).collect())
    }

    #[test]
    fn empty_input() {
        let kinds = token_kinds("").unwrap();
        assert_eq!(kinds, vec![TokenKind::Eof]);
    }

    #[test]
    fn whitespace_only() {
        let kinds = token_kinds("   \t  ").unwrap();
        assert_eq!(kinds, vec![TokenKind::Eof]);
    }

    #[test]
    fn newlines() {
        let kinds = token_kinds("\n\n").unwrap();
        assert_eq!(
            kinds,
            vec![TokenKind::Newline, TokenKind::Newline, TokenKind::Eof]
        );
    }

    #[test]
    fn simple_alias() {
        let kinds = token_kinds("alias greet { echo hello }").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Alias,
                TokenKind::Identifier("greet".into()),
                TokenKind::LeftBrace,
                TokenKind::Identifier("echo".into()),
                TokenKind::Identifier("hello".into()),
                TokenKind::RightBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn variables() {
        let kinds = token_kinds("%x %%global $nick").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Variable("x".into()),
                TokenKind::GlobalVariable("global".into()),
                TokenKind::BuiltinIdentifier("nick".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn builtin_numeric() {
        let kinds = token_kinds("$1 $2").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::BuiltinIdentifier("1".into()),
                TokenKind::BuiltinIdentifier("2".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_literal() {
        let kinds = token_kinds(r#""hello world""#).unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLiteral("hello world".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_with_escapes() {
        let kinds = token_kinds(r#""line1\nline2\t\"quoted\\""#).unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLiteral("line1\nline2\t\"quoted\\".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_with_interpolation_marker() {
        let kinds = token_kinds(r#""Hello $nick""#).unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLiteral("Hello $nick".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn integer_literal() {
        let kinds = token_kinds("42 0 999").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLiteral(42),
                TokenKind::IntLiteral(0),
                TokenKind::IntLiteral(999),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn float_literal() {
        let kinds = token_kinds("3.14 0.5").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::NumberLiteral(3.14),
                TokenKind::NumberLiteral(0.5),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn operators() {
        let kinds = token_kinds("+ - * / % = == != < > <= >= && || !").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Equal,
                TokenKind::EqualEqual,
                TokenKind::BangEqual,
                TokenKind::Less,
                TokenKind::Greater,
                TokenKind::LessEqual,
                TokenKind::GreaterEqual,
                TokenKind::AmpAmp,
                TokenKind::PipePipe,
                TokenKind::Bang,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn delimiters() {
        let kinds = token_kinds("{ } ( ) , :").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::Comma,
                TokenKind::Colon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn keywords() {
        let kinds = token_kinds(
            "alias on if elseif else while var set timer return break continue true false",
        )
        .unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Alias,
                TokenKind::On,
                TokenKind::If,
                TokenKind::ElseIf,
                TokenKind::Else,
                TokenKind::While,
                TokenKind::Var,
                TokenKind::Set,
                TokenKind::Timer,
                TokenKind::Return,
                TokenKind::Break,
                TokenKind::Continue,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn comment() {
        let kinds = token_kinds("; this is a comment\nalias").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Comment(" this is a comment".into()),
                TokenKind::Newline,
                TokenKind::Alias,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn command_slash() {
        let kinds = token_kinds("/msg").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Slash,
                TokenKind::Identifier("msg".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn spans_are_correct() {
        let mut lexer = Lexer::new("alias greet");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].span, Span::new(0, 5)); // "alias"
        assert_eq!(tokens[1].span, Span::new(6, 5)); // "greet"
        assert_eq!(tokens[2].span, Span::new(11, 0)); // Eof
    }

    #[test]
    fn multiline_span_tracking() {
        let src = "alias greet\n  %x";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].span, Span::new(0, 5));
        assert_eq!(tokens[1].span, Span::new(6, 5));
        assert_eq!(tokens[2].span, Span::new(11, 1));
        assert_eq!(tokens[3].span, Span::new(14, 2));
    }

    #[test]
    fn error_unterminated_string() {
        let result = token_kinds(r#""hello"#);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, LexError::UnterminatedString { .. }));
    }

    #[test]
    fn error_unterminated_string_newline() {
        let result = token_kinds("\"hello\nworld\"");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, LexError::UnterminatedString { .. }));
    }

    #[test]
    fn error_invalid_escape() {
        let result = token_kinds(r#""hello\x""#);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, LexError::InvalidEscape { ch: 'x', .. }));
    }

    #[test]
    fn error_unexpected_character() {
        let result = token_kinds("@");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            LexError::UnexpectedCharacter { ch: '@', .. }
        ));
    }

    #[test]
    fn percent_as_operator() {
        let kinds = token_kinds("5 % 3").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLiteral(5),
                TokenKind::Percent,
                TokenKind::IntLiteral(3),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn complex_expression() {
        let kinds = token_kinds("if (%x + 1 >= 10) && !%done").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::If,
                TokenKind::LeftParen,
                TokenKind::Variable("x".into()),
                TokenKind::Plus,
                TokenKind::IntLiteral(1),
                TokenKind::GreaterEqual,
                TokenKind::IntLiteral(10),
                TokenKind::RightParen,
                TokenKind::AmpAmp,
                TokenKind::Bang,
                TokenKind::Variable("done".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn full_script_snippet() {
        let src = r#"alias greet {
  var %name = $1
  if (%name == "") {
    /msg $chan "Hello everyone!"
  } else {
    /msg $chan "Hello, %name!"
  }
}"#;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
        assert_eq!(tokens[0].kind, TokenKind::Alias);
        assert_eq!(tokens[1].kind, TokenKind::Identifier("greet".into()));
        assert_eq!(tokens[2].kind, TokenKind::LeftBrace);
    }

    #[test]
    fn crlf_newlines() {
        let kinds = token_kinds("alias\r\ngreet").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Alias,
                TokenKind::Newline,
                TokenKind::Identifier("greet".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn dollar_alone_is_unexpected() {
        let result = token_kinds("$ ");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            LexError::UnexpectedCharacter { ch: '$', .. }
        ));
    }

    #[test]
    fn hyphenated_identifier() {
        let kinds = token_kinds("my-variable on-connect").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier("my-variable".into()),
                TokenKind::Identifier("on-connect".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn hyphenated_variable() {
        let kinds = token_kinds("%my-var %%global-var").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Variable("my-var".into()),
                TokenKind::GlobalVariable("global-var".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_with_carriage_return_escape() {
        let kinds = token_kinds(r#""line1\rline2""#).unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLiteral("line1\rline2".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_with_all_escape_sequences() {
        let kinds = token_kinds(r#""a\nb\tc\rd\\e\"""#).unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLiteral("a\nb\tc\rd\\e\"".into()),
                TokenKind::Eof,
            ]
        );
    }
}
