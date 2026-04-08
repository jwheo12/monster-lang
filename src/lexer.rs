use crate::token::{Token, TokenKind};

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            input: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current()?;
        self.pos += 1;

        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.current() {
            self.advance();
            if ch == '\n' {
                break;
            }
        }
    }

    fn read_number(&mut self, start_line: usize, start_col: usize) -> Token {
        let mut s = String::new();

        while let Some(ch) = self.current() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        Token::new(TokenKind::Int, s, start_line, start_col)
    }

    fn read_ident_or_keyword(&mut self, start_line: usize, start_col: usize) -> Token {
        let mut s = String::new();

        while let Some(ch) = self.current() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let kind = match s.as_str() {
            "extern" => TokenKind::Extern,
            "fn" => TokenKind::Fn,
            "struct" => TokenKind::Struct,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Ident,
        };

        Token::new(kind, s, start_line, start_col)
    }

    fn read_string(&mut self, start_line: usize, start_col: usize) -> Result<Token, String> {
        let mut s = String::new();
        self.advance();

        while let Some(ch) = self.current() {
            match ch {
                '"' => {
                    self.advance();
                    return Ok(Token::new(TokenKind::Str, s, start_line, start_col));
                }
                '\\' => {
                    self.advance();
                    let escaped = match self.current() {
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some(other) => {
                            return Err(format!(
                                "Invalid escape sequence '\\{}' at {}:{}",
                                other, self.line, self.column
                            ));
                        }
                        None => {
                            return Err(format!(
                                "Unterminated string literal at {}:{}",
                                start_line, start_col
                            ));
                        }
                    };
                    s.push(escaped);
                    self.advance();
                }
                '\n' => {
                    return Err(format!(
                        "Unterminated string literal at {}:{}",
                        start_line, start_col
                    ));
                }
                _ => {
                    s.push(ch);
                    self.advance();
                }
            }
        }

        Err(format!(
            "Unterminated string literal at {}:{}",
            start_line, start_col
        ))
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();

            if self.current() == Some('/') && self.peek() == Some('/') {
                self.skip_line_comment();
                continue;
            }

            let start_line = self.line;
            let start_col = self.column;

            let token = match self.current() {
                Some('(') => {
                    self.advance();
                    Token::new(TokenKind::LParen, "(".into(), start_line, start_col)
                }
                Some(')') => {
                    self.advance();
                    Token::new(TokenKind::RParen, ")".into(), start_line, start_col)
                }
                Some('{') => {
                    self.advance();
                    Token::new(TokenKind::LBrace, "{".into(), start_line, start_col)
                }
                Some('}') => {
                    self.advance();
                    Token::new(TokenKind::RBrace, "}".into(), start_line, start_col)
                }
                Some(':') => {
                    self.advance();
                    Token::new(TokenKind::Colon, ":".into(), start_line, start_col)
                }
                Some('.') => {
                    self.advance();
                    Token::new(TokenKind::Dot, ".".into(), start_line, start_col)
                }
                Some(',') => {
                    self.advance();
                    Token::new(TokenKind::Comma, ",".into(), start_line, start_col)
                }
                Some(';') => {
                    self.advance();
                    Token::new(TokenKind::Semicolon, ";".into(), start_line, start_col)
                }
                Some('[') => {
                    self.advance();
                    Token::new(TokenKind::LBracket, "[".into(), start_line, start_col)
                }
                Some(']') => {
                    self.advance();
                    Token::new(TokenKind::RBracket, "]".into(), start_line, start_col)
                }
                Some('=') => {
                    if self.peek() == Some('=') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::EqualEqual, "==".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Equal, "=".into(), start_line, start_col)
                    }
                }
                Some('!') => {
                    if self.peek() == Some('=') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::BangEqual, "!=".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Bang, "!".into(), start_line, start_col)
                    }
                }
                Some('&') => {
                    if self.peek() == Some('&') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::AndAnd, "&&".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Amp, "&".into(), start_line, start_col)
                    }
                }
                Some('|') => {
                    if self.peek() == Some('|') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::OrOr, "||".into(), start_line, start_col)
                    } else {
                        return Err(format!(
                            "Unexpected character '{}' at {}:{}",
                            '|', start_line, start_col
                        ));
                    }
                }
                Some('+') => {
                    self.advance();
                    Token::new(TokenKind::Plus, "+".into(), start_line, start_col)
                }
                Some('*') => {
                    self.advance();
                    Token::new(TokenKind::Star, "*".into(), start_line, start_col)
                }
                Some('/') => {
                    self.advance();
                    Token::new(TokenKind::Slash, "/".into(), start_line, start_col)
                }
                Some('<') => {
                    if self.peek() == Some('=') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::LessEqual, "<=".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Less, "<".into(), start_line, start_col)
                    }
                }
                Some('>') => {
                    if self.peek() == Some('=') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::GreaterEqual, ">=".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Greater, ">".into(), start_line, start_col)
                    }
                }
                Some('-') => {
                    if self.peek() == Some('>') {
                        self.advance();
                        self.advance();
                        Token::new(TokenKind::Arrow, "->".into(), start_line, start_col)
                    } else {
                        self.advance();
                        Token::new(TokenKind::Minus, "-".into(), start_line, start_col)
                    }
                }
                Some('"') => self.read_string(start_line, start_col)?,
                Some(ch) if ch.is_ascii_digit() => self.read_number(start_line, start_col),
                Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                    self.read_ident_or_keyword(start_line, start_col)
                }
                None => {
                    tokens.push(Token::new(TokenKind::Eof, "".into(), start_line, start_col));
                    break;
                }
                Some(ch) => {
                    return Err(format!(
                        "Unexpected character '{}' at {}:{}",
                        ch, start_line, start_col
                    ));
                }
            };

            tokens.push(token);
        }

        Ok(tokens)
    }
}
