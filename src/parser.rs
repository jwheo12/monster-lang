use std::collections::HashSet;

use crate::ast::{BinOp, Expr, Function, Program, Stmt, StructDef, Type, UnaryOp};
use crate::token::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    known_structs: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            known_structs: HashSet::new(),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.tokens.get(self.pos + 1).map(|t| t.kind.clone())
    }

    fn at_index_assign_start(&self) -> bool {
        if !self.at(TokenKind::Ident) || self.peek_kind() != Some(TokenKind::LBracket) {
            return false;
        }

        let mut pos = self.pos + 1;

        loop {
            let mut depth = 0usize;

            while let Some(token) = self.tokens.get(pos) {
                match token.kind {
                    TokenKind::LBracket => depth += 1,
                    TokenKind::RBracket => {
                        if depth == 0 {
                            return false;
                        }
                        depth -= 1;
                        if depth == 0 {
                            pos += 1;
                            break;
                        }
                    }
                    TokenKind::Semicolon | TokenKind::Eof => return false,
                    _ => {}
                }
                pos += 1;
            }

            match self.tokens.get(pos).map(|t| t.kind.clone()) {
                Some(TokenKind::LBracket) => continue,
                Some(TokenKind::Equal) => return true,
                _ => return false,
            }
        }
    }

    fn at_field_assign_start(&self) -> bool {
        if !self.at(TokenKind::Ident) || self.peek_kind() != Some(TokenKind::Dot) {
            return false;
        }

        let mut pos = self.pos + 1;

        loop {
            if self.tokens.get(pos).map(|t| t.kind.clone()) != Some(TokenKind::Dot) {
                return false;
            }
            pos += 1;

            if self.tokens.get(pos).map(|t| t.kind.clone()) != Some(TokenKind::Ident) {
                return false;
            }
            pos += 1;

            match self.tokens.get(pos).map(|t| t.kind.clone()) {
                Some(TokenKind::Dot) => continue,
                Some(TokenKind::Equal) => return true,
                _ => return false,
            }
        }
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len().saturating_sub(1) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, String> {
        let token = self.current().clone();
        if token.kind == kind {
            self.advance();
            Ok(token)
        } else {
            Err(format!(
                "expected {:?}, found {:?} at {}:{}",
                kind, token.kind, token.line, token.column
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<Token, String> {
        let token = self.current().clone();
        if token.kind == TokenKind::Ident {
            self.advance();
            Ok(token)
        } else {
            Err(format!(
                "expected identifier, found {:?} at {}:{}",
                token.kind, token.line, token.column
            ))
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut structs = Vec::new();
        let mut functions = Vec::new();

        while !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Struct) {
                let struct_def = self.parse_struct()?;
                self.known_structs.insert(struct_def.name.clone());
                structs.push(struct_def);
            } else {
                functions.push(self.parse_function()?);
            }
        }

        Ok(Program { structs, functions })
    }

    fn parse_struct(&mut self) -> Result<StructDef, String> {
        self.expect(TokenKind::Struct)?;
        let name = self.expect_ident()?.lexeme;
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.at(TokenKind::RBrace) {
            let field_name = self.expect_ident()?.lexeme;
            self.expect(TokenKind::Colon)?;
            let field_ty = self.parse_type()?;
            fields.push((field_name, field_ty));

            if self.at(TokenKind::Comma) {
                self.expect(TokenKind::Comma)?;
            } else {
                break;
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(StructDef { name, fields })
    }

    fn parse_function(&mut self) -> Result<Function, String> {
        let is_extern = if self.at(TokenKind::Extern) {
            self.expect(TokenKind::Extern)?;
            true
        } else {
            false
        };

        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?.lexeme;

        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;

        self.expect(TokenKind::Arrow)?;
        let ret_type = self.parse_type()?;
        let body = if is_extern {
            self.expect(TokenKind::Semicolon)?;
            None
        } else {
            Some(self.parse_block()?)
        };

        Ok(Function {
            name,
            params,
            ret_type,
            body,
            is_extern,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(TokenKind::LBrace)?;

        let mut body = Vec::new();
        while !self.at(TokenKind::RBrace) {
            body.push(self.parse_stmt()?);
        }

        self.expect(TokenKind::RBrace)?;
        Ok(body)
    }

    fn parse_params(&mut self) -> Result<Vec<(String, Type)>, String> {
        let mut params = Vec::new();

        if self.at(TokenKind::RParen) {
            return Ok(params);
        }

        loop {
            let name = self.expect_ident()?.lexeme;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            params.push((name, ty));

            if self.at(TokenKind::Comma) {
                self.expect(TokenKind::Comma)?;
            } else {
                break;
            }
        }

        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Type, String> {
        if self.at(TokenKind::Star) {
            self.expect(TokenKind::Star)?;
            let inner = self.parse_type()?;
            return Ok(Type::Ptr(Box::new(inner)));
        }

        if self.at(TokenKind::LBracket) {
            self.expect(TokenKind::LBracket)?;
            let element_ty = self.parse_type()?;
            if self.at(TokenKind::Semicolon) {
                self.expect(TokenKind::Semicolon)?;
                let len_token = self.expect(TokenKind::Int)?;
                let len = len_token.lexeme.parse::<usize>().map_err(|e| {
                    format!(
                        "invalid array length '{}' at {}:{}: {}",
                        len_token.lexeme, len_token.line, len_token.column, e
                    )
                })?;
                self.expect(TokenKind::RBracket)?;
                return Ok(Type::Array(Box::new(element_ty), len));
            }

            self.expect(TokenKind::RBracket)?;
            return Ok(Type::Slice(Box::new(element_ty)));
        }

        let token = self.expect_ident()?;
        match token.lexeme.as_str() {
            "i32" => Ok(Type::I32),
            "bool" => Ok(Type::Bool),
            "str" => Ok(Type::Str),
            "void" => Ok(Type::Void),
            _ => Ok(Type::Named(token.lexeme)),
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        if self.at(TokenKind::Let) {
            self.parse_let_stmt()
        } else if self.at(TokenKind::Return) {
            self.parse_return_stmt()
        } else if self.at(TokenKind::If) {
            self.parse_if_stmt()
        } else if self.at(TokenKind::While) {
            self.parse_while_stmt()
        } else if self.at(TokenKind::Star) {
            self.parse_assign_deref_stmt()
        } else if self.at_index_assign_start() {
            self.parse_assign_index_stmt()
        } else if self.at_field_assign_start() {
            self.parse_assign_field_stmt()
        } else if self.at(TokenKind::Ident) && self.peek_kind() == Some(TokenKind::Equal) {
            self.parse_assign_stmt()
        } else if self.at(TokenKind::Ident) && self.peek_kind() == Some(TokenKind::LParen) {
            self.parse_call_stmt()
        } else {
            let token = self.current().clone();
            Err(format!(
                "unexpected token {:?} at {}:{}",
                token.kind, token.line, token.column
            ))
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Let)?;
        let mutable = if self.at(TokenKind::Mut) {
            self.expect(TokenKind::Mut)?;
            true
        } else {
            false
        };
        let name = self.expect_ident()?.lexeme;
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        self.expect(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(Stmt::Let {
            name,
            ty,
            mutable,
            value,
        })
    }

    fn parse_assign_stmt(&mut self) -> Result<Stmt, String> {
        let name = self.expect_ident()?.lexeme;
        self.expect(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(Stmt::Assign { name, value })
    }

    fn parse_assign_index_stmt(&mut self) -> Result<Stmt, String> {
        let name = self.expect_ident()?.lexeme;
        let mut indices = Vec::new();
        while self.at(TokenKind::LBracket) {
            self.expect(TokenKind::LBracket)?;
            indices.push(self.parse_expr()?);
            self.expect(TokenKind::RBracket)?;
        }
        self.expect(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(Stmt::AssignIndex {
            name,
            indices,
            value,
        })
    }

    fn parse_assign_field_stmt(&mut self) -> Result<Stmt, String> {
        let name = self.expect_ident()?.lexeme;
        let mut fields = Vec::new();

        while self.at(TokenKind::Dot) {
            self.expect(TokenKind::Dot)?;
            fields.push(self.expect_ident()?.lexeme);
        }

        self.expect(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(Stmt::AssignField {
            name,
            fields,
            value,
        })
    }

    fn parse_assign_deref_stmt(&mut self) -> Result<Stmt, String> {
        let target = self.parse_unary()?;
        self.expect(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt::AssignDeref { target, value })
    }

    fn parse_call_stmt(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_call()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt::Expr(expr))
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Return)?;
        let value = if self.at(TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::Semicolon)?;
        Ok(Stmt::Return(value))
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::If)?;
        let condition = self.parse_expr()?;
        let then_body = self.parse_block()?;
        let else_body = if self.at(TokenKind::Else) {
            self.expect(TokenKind::Else)?;
            if self.at(TokenKind::If) {
                Some(vec![self.parse_if_stmt()?])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;

        Ok(Stmt::While { condition, body })
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_logical_and()?;

        while self.at(TokenKind::OrOr) {
            self.expect(TokenKind::OrOr)?;
            let right = self.parse_logical_and()?;
            left = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_equality()?;

        while self.at(TokenKind::AndAnd) {
            self.expect(TokenKind::AndAnd)?;
            let right = self.parse_equality()?;
            left = Expr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;

        loop {
            let op = if self.at(TokenKind::EqualEqual) {
                self.expect(TokenKind::EqualEqual)?;
                Some(BinOp::Eq)
            } else if self.at(TokenKind::BangEqual) {
                self.expect(TokenKind::BangEqual)?;
                Some(BinOp::Ne)
            } else {
                None
            };

            let Some(op) = op else {
                break;
            };

            let right = self.parse_comparison()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;

        loop {
            let op = if self.at(TokenKind::Less) {
                self.expect(TokenKind::Less)?;
                Some(BinOp::Lt)
            } else if self.at(TokenKind::LessEqual) {
                self.expect(TokenKind::LessEqual)?;
                Some(BinOp::Le)
            } else if self.at(TokenKind::Greater) {
                self.expect(TokenKind::Greater)?;
                Some(BinOp::Gt)
            } else if self.at(TokenKind::GreaterEqual) {
                self.expect(TokenKind::GreaterEqual)?;
                Some(BinOp::Ge)
            } else {
                None
            };

            let Some(op) = op else {
                break;
            };

            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;

        loop {
            if self.at(TokenKind::Plus) {
                self.expect(TokenKind::Plus)?;
                let right = self.parse_multiplicative()?;
                left = Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.at(TokenKind::Minus) {
                self.expect(TokenKind::Minus)?;
                let right = self.parse_multiplicative()?;
                left = Expr::Binary {
                    op: BinOp::Sub,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;

        loop {
            if self.at(TokenKind::Star) {
                self.expect(TokenKind::Star)?;
                let right = self.parse_unary()?;
                left = Expr::Binary {
                    op: BinOp::Mul,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else if self.at(TokenKind::Slash) {
                self.expect(TokenKind::Slash)?;
                let right = self.parse_unary()?;
                left = Expr::Binary {
                    op: BinOp::Div,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.at(TokenKind::Bang) {
            self.expect(TokenKind::Bang)?;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }

        if self.at(TokenKind::Minus) {
            self.expect(TokenKind::Minus)?;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }

        if self.at(TokenKind::Amp) {
            self.expect(TokenKind::Amp)?;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::AddrOf,
                expr: Box::new(expr),
            });
        }

        if self.at(TokenKind::Star) {
            self.expect(TokenKind::Star)?;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Deref,
                expr: Box::new(expr),
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = if self.at(TokenKind::Ident) && self.peek_kind() == Some(TokenKind::LParen) {
            self.parse_call()?
        } else if self.at_struct_literal_start() {
            self.parse_struct_literal()?
        } else {
            self.parse_primary()?
        };

        loop {
            if self.at(TokenKind::Dot) {
                self.expect(TokenKind::Dot)?;
                let field = self.expect_ident()?.lexeme;
                expr = Expr::FieldAccess {
                    base: Box::new(expr),
                    field,
                };
            } else if self.at(TokenKind::LBracket) {
                self.expect(TokenKind::LBracket)?;
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket)?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn at_struct_literal_start(&self) -> bool {
        self.at(TokenKind::Ident)
            && self.peek_kind() == Some(TokenKind::LBrace)
            && self.known_structs.contains(&self.current().lexeme)
    }

    fn parse_call(&mut self) -> Result<Expr, String> {
        let name = self.expect_ident()?.lexeme;
        self.expect(TokenKind::LParen)?;

        let mut args = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);

                if self.at(TokenKind::Comma) {
                    self.expect(TokenKind::Comma)?;
                } else {
                    break;
                }
            }
        }

        self.expect(TokenKind::RParen)?;

        Ok(Expr::Call { name, args })
    }

    fn parse_struct_literal(&mut self) -> Result<Expr, String> {
        let name = self.expect_ident()?.lexeme;
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        if !self.at(TokenKind::RBrace) {
            loop {
                let field_name = self.expect_ident()?.lexeme;
                self.expect(TokenKind::Colon)?;
                let value = self.parse_expr()?;
                fields.push((field_name, value));

                if self.at(TokenKind::Comma) {
                    self.expect(TokenKind::Comma)?;
                } else {
                    break;
                }
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Expr::StructLiteral { name, fields })
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let token = self.current().clone();

        match token.kind {
            TokenKind::Int => {
                self.advance();
                let value = token.lexeme.parse::<i32>().map_err(|e| {
                    format!(
                        "invalid integer '{}' at {}:{}: {}",
                        token.lexeme, token.line, token.column, e
                    )
                })?;
                Ok(Expr::Int(value))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            TokenKind::Str => {
                self.advance();
                Ok(Expr::Str(token.lexeme))
            }
            TokenKind::LBracket => self.parse_array_literal(),
            TokenKind::Ident => {
                self.advance();
                Ok(Expr::Var(token.lexeme))
            }
            TokenKind::LParen => {
                self.expect(TokenKind::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(format!(
                "expected expression, found {:?} at {}:{}",
                token.kind, token.line, token.column
            )),
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expr, String> {
        self.expect(TokenKind::LBracket)?;

        let mut elements = Vec::new();
        if !self.at(TokenKind::RBracket) {
            loop {
                elements.push(self.parse_expr()?);

                if self.at(TokenKind::Comma) {
                    self.expect(TokenKind::Comma)?;
                } else {
                    break;
                }
            }
        }

        self.expect(TokenKind::RBracket)?;
        Ok(Expr::ArrayLiteral(elements))
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::ast::{BinOp, Expr, Stmt, Type, UnaryOp};
    use crate::lexer::Lexer;

    fn parse_source(source: &str) -> crate::ast::Program {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().expect("tokenize should succeed");
        let mut parser = Parser::new(tokens);
        parser.parse_program().expect("parse should succeed")
    }

    #[test]
    fn parses_control_flow_and_assignment() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                let mut i: i32 = 0;
                let flag: bool = true;

                while i < 3 {
                    if flag {
                        i = i + 1;
                    } else {
                        i = i + 2;
                    }
                }

                return i;
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        assert!(matches!(
            body[0],
            Stmt::Let {
                mutable: true,
                ty: Type::I32,
                ..
            }
        ));
        assert!(matches!(
            body[1],
            Stmt::Let {
                mutable: false,
                ty: Type::Bool,
                ..
            }
        ));

        let Stmt::While { condition, body } = &body[2] else {
            panic!("expected while statement");
        };
        assert!(matches!(condition, Expr::Binary { op: BinOp::Lt, .. }));
        assert!(matches!(body[0], Stmt::If { .. }));
    }

    #[test]
    fn comparison_precedence_stays_below_arithmetic() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                return 2 + 3 * 4 < 20;
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        let Stmt::Return(Some(expr)) = &body[0] else {
            panic!("expected return statement");
        };

        let Expr::Binary { op, left, right } = expr else {
            panic!("expected binary expression");
        };
        assert_eq!(*op, BinOp::Lt);
        assert!(matches!(left.as_ref(), Expr::Binary { op: BinOp::Add, .. }));
        assert!(matches!(right.as_ref(), Expr::Int(20)));
    }

    #[test]
    fn parses_logical_and_unary_precedence() {
        let program = parse_source(
            r#"
            fn main() -> bool {
                return !false || -1 + 2 == 1 && true;
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        let Stmt::Return(Some(expr)) = &body[0] else {
            panic!("expected return statement");
        };

        let Expr::Binary { op, left, right } = expr else {
            panic!("expected binary expression");
        };
        assert_eq!(*op, BinOp::Or);
        assert!(matches!(
            left.as_ref(),
            Expr::Unary {
                op: UnaryOp::Not,
                ..
            }
        ));
        assert!(matches!(
            right.as_ref(),
            Expr::Binary { op: BinOp::And, .. }
        ));
    }

    #[test]
    fn parses_else_if_chain() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                if false {
                    return 0;
                } else if true {
                    return 1;
                } else {
                    return 2;
                }
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        let Stmt::If { else_body, .. } = &body[0] else {
            panic!("expected if statement");
        };

        let else_body = else_body.as_ref().expect("expected else body");
        assert!(matches!(else_body.as_slice(), [Stmt::If { .. }]));
    }

    #[test]
    fn parses_call_statement() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                print_str("Hello, World!");
                return 0;
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        let Stmt::Expr(Expr::Call { args, .. }) = &body[0] else {
            panic!("expected call statement");
        };

        assert!(matches!(args.as_slice(), [Expr::Str(value)] if value == "Hello, World!"));
    }

    #[test]
    fn parses_void_function_with_bare_return() {
        let program = parse_source(
            r#"
            fn log_message() -> void {
                print_str("Hello");
                return;
            }
            "#,
        );

        assert_eq!(program.functions[0].ret_type, Type::Void);
        assert!(!program.functions[0].is_extern);
        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        assert!(matches!(body[1], Stmt::Return(None)));
    }

    #[test]
    fn parses_extern_function_declaration() {
        let program = parse_source(
            r#"
            extern fn abs(value: i32) -> i32;

            fn main() -> i32 {
                return abs(-7);
            }
            "#,
        );

        assert!(program.functions[0].is_extern);
        assert!(program.functions[0].body.is_none());
        assert_eq!(program.functions[0].name, "abs");
    }

    #[test]
    fn parses_struct_definition_and_field_access() {
        let program = parse_source(
            r#"
            struct Pair {
                left: i32,
                right: i32,
            }

            fn main() -> i32 {
                let pair: Pair = Pair { left: 10, right: 20 };
                return pair.left + pair.right;
            }
            "#,
        );

        assert_eq!(program.structs[0].name, "Pair");
        assert_eq!(program.structs[0].fields.len(), 2);

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");
        assert!(matches!(
            body[0],
            Stmt::Let {
                ty: Type::Named(ref struct_name),
                value: Expr::StructLiteral {
                    name: ref literal_name,
                    ..
                },
                ..
            } if struct_name == "Pair" && literal_name == "Pair"
        ));

        let Stmt::Return(Some(Expr::Binary { left, right, .. })) = &body[1] else {
            panic!("expected return with binary field access");
        };

        assert!(matches!(
            left.as_ref(),
            Expr::FieldAccess { field, .. } if field == "left"
        ));
        assert!(matches!(
            right.as_ref(),
            Expr::FieldAccess { field, .. } if field == "right"
        ));
    }

    #[test]
    fn parses_struct_field_assignment() {
        let program = parse_source(
            r#"
            struct Inner {
                value: i32,
            }

            struct Pair {
                inner: Inner,
                right: i32,
            }

            fn main() -> i32 {
                let mut pair: Pair = Pair { inner: Inner { value: 10 }, right: 20 };
                pair.inner.value = 42;
                return pair.inner.value;
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");

        let Stmt::AssignField {
            name,
            fields,
            value,
        } = &body[1]
        else {
            panic!("expected field assignment");
        };

        assert_eq!(name, "pair");
        assert_eq!(fields, &vec!["inner".to_string(), "value".to_string()]);
        assert!(matches!(value, Expr::Int(42)));
    }

    #[test]
    fn parses_array_type_literal_and_index() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                return values[1];
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");

        assert!(matches!(
            body[0],
            Stmt::Let {
                ty: Type::Array(_, 3),
                value: Expr::ArrayLiteral(_),
                ..
            }
        ));

        let Stmt::Return(Some(Expr::Index { base, index })) = &body[1] else {
            panic!("expected indexed return");
        };

        assert!(matches!(base.as_ref(), Expr::Var(name) if name == "values"));
        assert!(matches!(index.as_ref(), Expr::Int(1)));
    }

    #[test]
    fn parses_slice_type_and_slice_call() {
        let program = parse_source(
            r#"
            fn head(values: [i32]) -> i32 {
                return values[0] + len(values);
            }

            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                return head(slice(values));
            }
            "#,
        );

        assert!(matches!(
            program.functions[0].params[0].1,
            Type::Slice(ref inner) if **inner == Type::I32
        ));

        let body = program.functions[1]
            .body
            .as_ref()
            .expect("expected function body");
        let Stmt::Return(Some(Expr::Call { name, args })) = &body[1] else {
            panic!("expected call return");
        };

        assert_eq!(name, "head");
        assert_eq!(args.len(), 1);
        assert!(matches!(
            &args[0],
            Expr::Call { name, args } if name == "slice" && matches!(args.as_slice(), [Expr::Var(var)] if var == "values")
        ));
    }

    #[test]
    fn parses_pointer_type_and_deref_assignment() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                let mut x: i32 = 10;
                let p: *i32 = &x;
                *p = 42;
                return p[0];
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");

        assert!(matches!(
            body[1],
            Stmt::Let {
                ty: Type::Ptr(_),
                value: Expr::Unary {
                    op: UnaryOp::AddrOf,
                    ..
                },
                ..
            }
        ));

        assert!(matches!(
            body[2],
            Stmt::AssignDeref {
                target: Expr::Unary {
                    op: UnaryOp::Deref,
                    ..
                },
                value: Expr::Int(42),
            }
        ));

        let Stmt::Return(Some(Expr::Index { base, index })) = &body[3] else {
            panic!("expected pointer indexing return");
        };
        assert!(matches!(base.as_ref(), Expr::Var(name) if name == "p"));
        assert!(matches!(index.as_ref(), Expr::Int(0)));
    }

    #[test]
    fn parses_array_index_assignment_and_len_call() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                let mut values: [i32; 3] = [10, 20, 30];
                values[1] = 99;
                return len(values);
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");

        assert!(matches!(
            body[0],
            Stmt::Let {
                mutable: true,
                ty: Type::Array(_, 3),
                ..
            }
        ));

        let Stmt::AssignIndex {
            name,
            indices,
            value,
        } = &body[1]
        else {
            panic!("expected index assignment");
        };
        assert_eq!(name, "values");
        assert_eq!(indices.len(), 1);
        assert!(matches!(indices[0], Expr::Int(1)));
        assert!(matches!(value, Expr::Int(99)));

        let Stmt::Return(Some(Expr::Call { name, args })) = &body[2] else {
            panic!("expected return len(...)");
        };
        assert_eq!(name, "len");
        assert_eq!(args.len(), 1);
        assert!(matches!(args[0], Expr::Var(ref name) if name == "values"));
    }

    #[test]
    fn parses_nested_array_index_assignment() {
        let program = parse_source(
            r#"
            fn main() -> i32 {
                let mut matrix: [[i32; 2]; 2] = [[1, 2], [3, 4]];
                matrix[1][0] = 99;
                return matrix[1][0];
            }
            "#,
        );

        let body = program.functions[0]
            .body
            .as_ref()
            .expect("expected function body");

        let Stmt::AssignIndex {
            name,
            indices,
            value,
        } = &body[1]
        else {
            panic!("expected nested index assignment");
        };

        assert_eq!(name, "matrix");
        assert_eq!(indices.len(), 2);
        assert!(matches!(indices[0], Expr::Int(1)));
        assert!(matches!(indices[1], Expr::Int(0)));
        assert!(matches!(value, Expr::Int(99)));

        let Stmt::Return(Some(Expr::Index { base, index })) = &body[2] else {
            panic!("expected nested return indexing");
        };
        assert!(matches!(index.as_ref(), Expr::Int(0)));
        assert!(matches!(base.as_ref(), Expr::Index { .. }));
    }
}
