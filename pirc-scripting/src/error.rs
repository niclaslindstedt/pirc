use crate::token::Span;

/// A source location (line and column, 1-based) for human-readable error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl SourceLocation {
    /// Creates a new source location.
    #[must_use]
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Computes the source location for a byte offset in the given source text.
    #[must_use]
    pub fn from_offset(source: &str, offset: usize) -> Self {
        let mut line = 1;
        let mut col = 1;
        for (i, ch) in source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        Self { line, column: col }
    }
}

/// Errors produced by the scripting engine.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ScriptError {
    /// An error during lexical analysis.
    #[error("{0}")]
    Lex(#[from] LexError),

    /// An error during parsing.
    #[error("{0}")]
    Parse(#[from] ParseError),

    /// An error during semantic analysis.
    #[error("{0}")]
    Semantic(#[from] SemanticError),
}

/// Errors that occur during lexical analysis (tokenization).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LexError {
    /// An unexpected character was encountered.
    #[error("unexpected character '{ch}' at {location}")]
    UnexpectedCharacter {
        /// The unexpected character.
        ch: char,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// A string literal was not terminated.
    #[error("unterminated string literal at {location}")]
    UnterminatedString {
        /// Source span (from opening quote).
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// An invalid escape sequence in a string.
    #[error("invalid escape sequence '\\{ch}' at {location}")]
    InvalidEscape {
        /// The character after the backslash.
        ch: char,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// A number literal could not be parsed.
    #[error("invalid number literal at {location}: {message}")]
    InvalidNumber {
        /// Description of what went wrong.
        message: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },
}

impl LexError {
    /// Returns the span of this error.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::UnexpectedCharacter { span, .. }
            | Self::UnterminatedString { span, .. }
            | Self::InvalidEscape { span, .. }
            | Self::InvalidNumber { span, .. } => *span,
        }
    }
}

/// Errors that occur during parsing.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseError {
    /// An unexpected token was encountered.
    #[error("unexpected token at {location}: expected {expected}, found {found}")]
    UnexpectedToken {
        /// Description of what was expected.
        expected: String,
        /// Description of what was found.
        found: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// An unexpected end of input.
    #[error("unexpected end of input at {location}: expected {expected}")]
    UnexpectedEof {
        /// Description of what was expected.
        expected: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// An invalid event type string.
    #[error("invalid event type '{name}' at {location}")]
    InvalidEventType {
        /// The unrecognized event type name.
        name: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },
}

impl ParseError {
    /// Returns the span of this error.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::UnexpectedToken { span, .. }
            | Self::UnexpectedEof { span, .. }
            | Self::InvalidEventType { span, .. } => *span,
        }
    }
}

/// Errors that occur during semantic analysis.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SemanticError {
    /// A variable was used before being declared.
    #[error("undefined variable '%{name}' at {location}")]
    UndefinedVariable {
        /// The variable name.
        name: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// A duplicate definition was found.
    #[error("duplicate definition '{name}' at {location}")]
    DuplicateDefinition {
        /// The duplicated name.
        name: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },

    /// A `break` or `continue` was used outside a loop.
    #[error("{keyword} outside of loop at {location}")]
    BreakOutsideLoop {
        /// The keyword (`break` or `continue`).
        keyword: String,
        /// Source span.
        span: Span,
        /// Human-readable location.
        location: SourceLocation,
    },
}

impl SemanticError {
    /// Returns the span of this error.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::UndefinedVariable { span, .. }
            | Self::DuplicateDefinition { span, .. }
            | Self::BreakOutsideLoop { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_location_display() {
        let loc = SourceLocation::new(3, 7);
        assert_eq!(loc.to_string(), "3:7");
    }

    #[test]
    fn source_location_from_offset() {
        let src = "line1\nline2\nline3";
        assert_eq!(SourceLocation::from_offset(src, 0), SourceLocation::new(1, 1));
        assert_eq!(SourceLocation::from_offset(src, 5), SourceLocation::new(1, 6));
        assert_eq!(SourceLocation::from_offset(src, 6), SourceLocation::new(2, 1));
        assert_eq!(SourceLocation::from_offset(src, 12), SourceLocation::new(3, 1));
    }

    #[test]
    fn lex_error_display() {
        let err = LexError::UnexpectedCharacter {
            ch: '@',
            span: Span::new(5, 1),
            location: SourceLocation::new(1, 6),
        };
        assert_eq!(err.to_string(), "unexpected character '@' at 1:6");
        assert_eq!(err.span(), Span::new(5, 1));
    }

    #[test]
    fn parse_error_display() {
        let err = ParseError::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "'+'".to_string(),
            span: Span::new(10, 1),
            location: SourceLocation::new(2, 5),
        };
        assert_eq!(
            err.to_string(),
            "unexpected token at 2:5: expected identifier, found '+'"
        );
    }

    #[test]
    fn semantic_error_display() {
        let err = SemanticError::UndefinedVariable {
            name: "x".to_string(),
            span: Span::new(0, 2),
            location: SourceLocation::new(1, 1),
        };
        assert_eq!(err.to_string(), "undefined variable '%x' at 1:1");
    }

    #[test]
    fn script_error_from_lex() {
        let lex_err = LexError::UnterminatedString {
            span: Span::new(0, 5),
            location: SourceLocation::new(1, 1),
        };
        let err: ScriptError = lex_err.into();
        assert!(matches!(err, ScriptError::Lex(_)));
    }

    #[test]
    fn script_error_from_parse() {
        let parse_err = ParseError::UnexpectedEof {
            expected: "'}'".to_string(),
            span: Span::new(20, 0),
            location: SourceLocation::new(5, 1),
        };
        let err: ScriptError = parse_err.into();
        assert!(matches!(err, ScriptError::Parse(_)));
    }

    #[test]
    fn script_error_from_semantic() {
        let sem_err = SemanticError::BreakOutsideLoop {
            keyword: "break".to_string(),
            span: Span::new(15, 5),
            location: SourceLocation::new(3, 3),
        };
        let err: ScriptError = sem_err.into();
        assert!(matches!(err, ScriptError::Semantic(_)));
    }

    #[test]
    fn script_error_implements_std_error() {
        let err: ScriptError = LexError::UnexpectedCharacter {
            ch: '~',
            span: Span::new(0, 1),
            location: SourceLocation::new(1, 1),
        }
        .into();
        // Verify std::error::Error is implemented
        let _: &dyn std::error::Error = &err;
    }
}
