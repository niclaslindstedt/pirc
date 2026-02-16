/// A byte-offset span in the source text, used for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the first character.
    pub offset: usize,
    /// Length in bytes.
    pub length: usize,
}

impl Span {
    /// Creates a new span from an offset and length.
    #[must_use]
    pub fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }

    /// Returns the exclusive end byte offset.
    #[must_use]
    pub fn end(&self) -> usize {
        self.offset + self.length
    }

    /// Merges two spans into one that covers both.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        let start = self.offset.min(other.offset);
        let end = self.end().max(other.end());
        Self {
            offset: start,
            length: end - start,
        }
    }
}

/// The kind of a lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // --- Keywords ---
    /// `alias`
    Alias,
    /// `on`
    On,
    /// `if`
    If,
    /// `elseif`
    ElseIf,
    /// `else`
    Else,
    /// `while`
    While,
    /// `var`
    Var,
    /// `set`
    Set,
    /// `timer`
    Timer,
    /// `return`
    Return,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `true`
    True,
    /// `false`
    False,

    // --- Literals ---
    /// A string literal (contents without quotes).
    StringLiteral(String),
    /// A floating-point number literal.
    NumberLiteral(f64),
    /// An integer number literal.
    IntLiteral(i64),

    // --- Identifiers ---
    /// A plain identifier (e.g. `greet`, `msg`).
    Identifier(String),
    /// A local variable reference (`%name`).
    Variable(String),
    /// A global variable reference (`%%name`).
    GlobalVariable(String),
    /// A built-in identifier (`$nick`, `$1`, etc.).
    BuiltinIdentifier(String),

    // --- Operators ---
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `=`
    Equal,
    /// `==`
    EqualEqual,
    /// `!=`
    BangEqual,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessEqual,
    /// `>=`
    GreaterEqual,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `!`
    Bang,

    // --- Delimiters ---
    /// `{`
    LeftBrace,
    /// `}`
    RightBrace,
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `;` (used as comment start in source, but also a delimiter)
    Semicolon,

    // --- Special ---
    /// A newline (statement terminator).
    Newline,
    /// End of file.
    Eof,
    /// A comment (contents after `;` to end of line).
    Comment(String),
}

impl TokenKind {
    /// Returns `true` if this token is a keyword.
    #[must_use]
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Self::Alias
                | Self::On
                | Self::If
                | Self::ElseIf
                | Self::Else
                | Self::While
                | Self::Var
                | Self::Set
                | Self::Timer
                | Self::Return
                | Self::Break
                | Self::Continue
                | Self::True
                | Self::False
        )
    }

    /// Looks up a keyword by name, returning `None` for non-keywords.
    #[must_use]
    pub fn keyword(name: &str) -> Option<Self> {
        match name {
            "alias" => Some(Self::Alias),
            "on" => Some(Self::On),
            "if" => Some(Self::If),
            "elseif" => Some(Self::ElseIf),
            "else" => Some(Self::Else),
            "while" => Some(Self::While),
            "var" => Some(Self::Var),
            "set" => Some(Self::Set),
            "timer" => Some(Self::Timer),
            "return" => Some(Self::Return),
            "break" => Some(Self::Break),
            "continue" => Some(Self::Continue),
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            _ => None,
        }
    }
}

/// A lexical token with its kind and source location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The kind of token.
    pub kind: TokenKind,
    /// The source location of this token.
    pub span: Span,
}

impl Token {
    /// Creates a new token.
    #[must_use]
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_new_and_end() {
        let span = Span::new(10, 5);
        assert_eq!(span.offset, 10);
        assert_eq!(span.length, 5);
        assert_eq!(span.end(), 15);
    }

    #[test]
    fn span_merge() {
        let a = Span::new(5, 3);
        let b = Span::new(10, 4);
        let merged = a.merge(b);
        assert_eq!(merged.offset, 5);
        assert_eq!(merged.end(), 14);
    }

    #[test]
    fn keyword_lookup() {
        assert_eq!(TokenKind::keyword("alias"), Some(TokenKind::Alias));
        assert_eq!(TokenKind::keyword("if"), Some(TokenKind::If));
        assert_eq!(TokenKind::keyword("elseif"), Some(TokenKind::ElseIf));
        assert_eq!(TokenKind::keyword("return"), Some(TokenKind::Return));
        assert_eq!(TokenKind::keyword("notakeyword"), None);
    }

    #[test]
    fn keyword_check() {
        assert!(TokenKind::Alias.is_keyword());
        assert!(TokenKind::While.is_keyword());
        assert!(!TokenKind::Plus.is_keyword());
        assert!(!TokenKind::Identifier("foo".to_string()).is_keyword());
    }

    #[test]
    fn token_construction() {
        let token = Token::new(TokenKind::Plus, Span::new(0, 1));
        assert_eq!(token.kind, TokenKind::Plus);
        assert_eq!(token.span, Span::new(0, 1));
    }
}
