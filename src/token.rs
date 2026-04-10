#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Extern,
    Import,
    Fn,
    Struct,
    Enum,
    Match,
    SizeOf,
    Let,
    Mut,
    As,
    Return,
    If,
    Else,
    While,
    Break,
    Continue,
    True,
    False,
    Arrow,        // ->
    Dot,          // .
    Colon,        // :
    Comma,        // ,
    Semicolon,    // ;
    LBracket,     // [
    RBracket,     // ]
    LParen,       // (
    RParen,       // )
    LBrace,       // {
    RBrace,       // }
    Equal,        // =
    FatArrow,     // =>
    Bang,         // !
    Amp,          // &
    EqualEqual,   // ==
    BangEqual,    // !=
    AndAnd,       // &&
    OrOr,         // ||
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Less,         // <
    LessEqual,    // <=
    Greater,      // >
    GreaterEqual, // >=
    Ident,
    Int,
    Str,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, lexeme: String, line: usize, column: usize) -> Self {
        Self {
            kind,
            lexeme,
            line,
            column,
        }
    }
}
