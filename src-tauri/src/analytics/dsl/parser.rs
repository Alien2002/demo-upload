// src-tauri/src/analytics/dsl/parser.rs
//
// Token stream + everything that is shared between commands:
//   - expression parser (precedence climbing)
//   - scope clauses: IF, WHILE, FIRST n, NEXT n  (any order, each at most once)
//   - TO "table", field lists, keyword helpers
// Command parsers in dsl/commands/ only handle their own keywords and
// delegate everything else here.

use super::ast::*;
use super::token::{Spanned, Token};
use crate::analytics::error::{CmdResult, CommandError};

/// Words that end an expression or a field list. A field with one of these
/// names can still be used as [Name].
pub const CLAUSE_KEYWORDS: &[&str] = &[
    "IF", "WHILE", "FIRST", "NEXT", "TO", "ON", "FIELDS", "SUBTOTAL", "OTHER", "AS", "RECORD", "PRESORT",
];

pub struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ------------------------------------------------------------ stream

    pub fn current(&self) -> &Spanned {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    pub fn advance(&mut self) -> Spanned {
        let t = self.current().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    pub fn at_end(&self) -> bool {
        matches!(self.current().token, Token::Eof)
    }

    pub fn span(&self) -> Span {
        let t = self.current();
        Span::new(t.start, t.end)
    }

    /// Is the current token the (bare) keyword `kw`? Case-insensitive.
    pub fn at_keyword(&self, kw: &str) -> bool {
        matches!(&self.current().token, Token::Ident(w) if w.eq_ignore_ascii_case(kw))
    }

    pub fn at_any_clause_keyword(&self) -> bool {
        CLAUSE_KEYWORDS.iter().any(|k| self.at_keyword(k))
    }

    /// Consume `kw` if present.
    pub fn eat_keyword(&mut self, kw: &str) -> bool {
        if self.at_keyword(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn error_here(&self, message: impl Into<String>) -> CommandError {
        let t = self.current();
        CommandError::new(message, t.start, t.end)
    }

    pub fn unexpected(&self, context: &str) -> CommandError {
        let t = self.current();
        if matches!(t.token, Token::Eof) {
            CommandError::new(format!("Command ended early: {context}"), t.start, t.end)
        } else {
            CommandError::new(format!("Unexpected {} — {context}", t.token.describe()), t.start, t.end)
        }
    }

    // ------------------------------------------------------------ shared clauses

    /// Try to parse one scope clause at the current position.
    /// Returns Ok(true) if a clause was consumed.
    pub fn parse_scope_clause(&mut self, scope: &mut Scope) -> CmdResult<bool> {
        let kw_span = self.span();
        if self.eat_keyword("IF") {
            if scope.if_.is_some() {
                return Err(CommandError::new("IF is given twice; combine conditions with AND", kw_span.start, kw_span.end));
            }
            scope.if_ = Some(self.parse_condition("IF")?);
            return Ok(true);
        }
        if self.eat_keyword("WHILE") {
            if scope.while_.is_some() {
                return Err(CommandError::new("WHILE is given twice; combine conditions with AND", kw_span.start, kw_span.end));
            }
            scope.while_ = Some(self.parse_condition("WHILE")?);
            return Ok(true);
        }
        for (kw, kind) in [("FIRST", FirstNextKind::First), ("NEXT", FirstNextKind::Next)] {
            if self.at_keyword(kw) {
                if scope.first_next.is_some() {
                    return Err(CommandError::new(
                        "Use only one of FIRST or NEXT",
                        kw_span.start,
                        kw_span.end,
                    ));
                }
                self.advance();
                let n_span = self.span();
                let n = match &self.current().token {
                    Token::Number(n) => n
                        .parse::<u64>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| CommandError::new(format!("{kw} needs a whole number above 0"), n_span.start, n_span.end))?,
                    _ => return Err(self.unexpected(&format!("{kw} needs a number of records, e.g. {kw} 100"))),
                };
                self.advance();
                scope.first_next = Some(FirstNext { kind, n, span: kw_span.to(&n_span) });
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn parse_condition(&mut self, kw: &str) -> CmdResult<Expr> {
        if self.at_end() || self.at_any_clause_keyword() {
            return Err(self.unexpected(&format!("{kw} needs a condition, e.g. {kw} Amount > 1000")));
        }
        self.parse_expr()
    }

    /// TO "Table name"  |  TO TableName
    pub fn parse_target(&mut self) -> CmdResult<Target> {
        let span = self.span();
        let name = match &self.current().token {
            Token::Str(s) => s.trim().to_string(),
            Token::Ident(s) if !CLAUSE_KEYWORDS.iter().any(|k| s.eq_ignore_ascii_case(k)) => s.clone(),
            Token::QuotedIdent(s) => s.clone(),
            _ => return Err(self.unexpected("TO needs a table name, e.g. TO \"High value\"")),
        };
        self.advance();
        if name.is_empty() {
            return Err(CommandError::new("Table name cannot be empty", span.start, span.end));
        }
        Ok(Target { name, span })
    }

    /// Field/expression list until the next clause keyword or the end.
    /// Items may be separated by spaces or commas; each may have AS "alias".
    pub fn parse_field_list(&mut self, what: &str) -> CmdResult<Vec<(Expr, Option<String>)>> {
        let mut items = Vec::new();
        loop {
            if self.at_end() || self.at_any_clause_keyword() {
                break;
            }
            if matches!(self.current().token, Token::Comma) {
                self.advance();
                continue;
            }
            let expr = self.parse_expr()?;
            let alias = if self.eat_keyword("AS") {
                match &self.current().token {
                    Token::Str(s) | Token::Ident(s) | Token::QuotedIdent(s) => {
                        let s = s.trim().to_string();
                        self.advance();
                        Some(s)
                    }
                    _ => return Err(self.unexpected("AS needs a name, e.g. AS \"Net amount\"")),
                }
            } else {
                None
            };
            items.push((expr, alias));
        }
        if items.is_empty() {
            return Err(self.unexpected(&format!("expected {what}")));
        }
        Ok(items)
    }

    pub fn expect_end(&self) -> CmdResult<()> {
        if self.at_end() {
            Ok(())
        } else {
            Err(self.unexpected("expected end of command"))
        }
    }

    // ------------------------------------------------------------ expressions
    //
    // Precedence (low -> high): OR, AND, NOT, comparison, + -, * /, unary -, primary

    pub fn parse_expr(&mut self) -> CmdResult<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> CmdResult<Expr> {
        let mut left = self.parse_and()?;
        while matches!(self.current().token, Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = binary(BinaryOp::Or, left, right);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> CmdResult<Expr> {
        let mut left = self.parse_not()?;
        while matches!(self.current().token, Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = binary(BinaryOp::And, left, right);
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> CmdResult<Expr> {
        if matches!(self.current().token, Token::Not) {
            let start = self.advance().start;
            let expr = self.parse_not()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr { kind: ExprKind::Unary { op: UnaryOp::Not, expr: Box::new(expr) }, span });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> CmdResult<Expr> {
        let left = self.parse_additive()?;
        let op = match self.current().token {
            Token::Eq => BinaryOp::Eq,
            Token::Ne => BinaryOp::Ne,
            Token::Lt => BinaryOp::Lt,
            Token::Le => BinaryOp::Le,
            Token::Gt => BinaryOp::Gt,
            Token::Ge => BinaryOp::Ge,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_additive()?;
        Ok(binary(op, left, right))
    }

    fn parse_additive(&mut self) -> CmdResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.current().token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = binary(op, left, right);
        }
    }

    fn parse_multiplicative(&mut self) -> CmdResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current().token {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.parse_unary()?;
            left = binary(op, left, right);
        }
    }

    fn parse_unary(&mut self) -> CmdResult<Expr> {
        if matches!(self.current().token, Token::Minus) {
            let start = self.advance().start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr { kind: ExprKind::Unary { op: UnaryOp::Neg, expr: Box::new(expr) }, span });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> CmdResult<Expr> {
        let t = self.current().clone();
        let span = Span::new(t.start, t.end);
        let simple = |kind| Ok(Expr { kind, span: span.clone() });

        match t.token {
            Token::Number(n) => {
                self.advance();
                simple(ExprKind::Number(n))
            }
            Token::Str(s) => {
                self.advance();
                simple(ExprKind::Str(s))
            }
            Token::Date(d) => {
                self.advance();
                simple(ExprKind::Date(d))
            }
            Token::DateTime(d) => {
                self.advance();
                simple(ExprKind::DateTime(d))
            }
            Token::QuotedIdent(name) => {
                self.advance();
                simple(ExprKind::Name(name))
            }
            Token::LParen => {
                self.advance();
                let mut inner = self.parse_expr()?;
                if !matches!(self.current().token, Token::RParen) {
                    return Err(self.unexpected("missing closing )"));
                }
                let end = self.advance().end;
                inner.span = Span::new(span.start, end);
                Ok(inner)
            }
            Token::Ident(word) => {
                if CLAUSE_KEYWORDS.iter().any(|k| word.eq_ignore_ascii_case(k)) {
                    return Err(self.unexpected("expected a field or value (use [Name] for a field called like a keyword)"));
                }
                self.advance();
                let upper = word.to_ascii_uppercase();
                if matches!(self.current().token, Token::LParen) {
                    return self.parse_call(word, span);
                }
                match upper.as_str() {
                    "TRUE" => simple(ExprKind::Bool(true)),
                    "FALSE" => simple(ExprKind::Bool(false)),
                    _ => simple(ExprKind::Name(word)),
                }
            }
            _ => Err(self.unexpected("expected a field or value")),
        }
    }

    fn parse_call(&mut self, name: String, name_span: Span) -> CmdResult<Expr> {
        self.advance(); // (
        let mut args = Vec::new();
        if !matches!(self.current().token, Token::RParen) {
            loop {
                args.push(self.parse_expr()?);
                match self.current().token {
                    Token::Comma => {
                        self.advance();
                    }
                    Token::RParen => break,
                    _ => return Err(self.unexpected(&format!("expected , or ) in {}(…)", name.to_uppercase()))),
                }
            }
        }
        let end = self.advance().end; // )
        Ok(Expr { kind: ExprKind::Call { name, args }, span: Span::new(name_span.start, end) })
    }
}

fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    let span = left.span.to(&right.span);
    Expr { kind: ExprKind::Binary { op, left: Box::new(left), right: Box::new(right) }, span }
}
