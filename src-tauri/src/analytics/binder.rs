// src-tauri/src/analytics/binder.rs
//
// Binder: checks an expression against the table's real columns and the
// session variables, works out its type, and compiles it to a DuckDB SQL
// fragment. After this step the planner and SQL generator only ever see
// valid, typed, safely-quoted SQL.
//
//   Amount > 1000 AND [Txn Date] >= `20240101`
//   -> ("Amount" > 1000) AND ("Txn Date" >= DATE '2024-01-01')   : Bool

use std::collections::HashMap;

use super::dsl::ast::{BinaryOp, Expr, ExprKind, Span, UnaryOp};
use super::error::{CmdResult, CommandError};
use super::functions;
use crate::names::{quote_ident, quote_literal};
use crate::query::ColumnInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Num,
    Text,
    Date,
    DateTime,
    Bool,
    /// Column of a type we don't model (lists, blobs…): can be shown, not computed.
    Other,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Num => "number",
            Ty::Text => "text",
            Ty::Date => "date",
            Ty::DateTime => "date-time",
            Ty::Bool => "logical (true/false)",
            Ty::Other => "unsupported type",
        }
    }

    pub fn from_sql_type(t: &str) -> Ty {
        let t = t.to_ascii_uppercase();
        if t.starts_with("VARCHAR") || t.starts_with("TEXT") || t.starts_with("STRING") {
            Ty::Text
        } else if t == "DATE" {
            Ty::Date
        } else if t.starts_with("TIMESTAMP") {
            Ty::DateTime
        } else if t == "BOOLEAN" {
            Ty::Bool
        } else if ["TINYINT", "SMALLINT", "INTEGER", "BIGINT", "HUGEINT", "UTINYINT", "USMALLINT", "UINTEGER", "UBIGINT", "FLOAT", "REAL", "DOUBLE", "DECIMAL"]
            .iter()
            .any(|p| t.starts_with(p))
        {
            Ty::Num
        } else {
            Ty::Other
        }
    }

    fn is_temporal(self) -> bool {
        matches!(self, Ty::Date | Ty::DateTime)
    }

    /// Can values of these two types be compared?
    pub fn comparable(a: Ty, b: Ty) -> bool {
        a == b || (a.is_temporal() && b.is_temporal())
    }
}

/// A bound (checked) expression.
#[derive(Debug, Clone)]
pub struct BExpr {
    pub sql: String,
    pub ty: Ty,
    pub span: Span,
    /// The column it is, if it's a plain field reference.
    pub field: Option<String>,
}

/// Session variable (COUNT1, TOTAL1, …).
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub display: String,
    pub sql: String,
    pub ty: Ty,
}

/// Upper-cased name -> variable.
pub type Variables = HashMap<String, Variable>;

pub struct Binder<'a> {
    columns: &'a [ColumnInfo],
    vars: &'a Variables,
}

impl<'a> Binder<'a> {
    pub fn new(columns: &'a [ColumnInfo], vars: &'a Variables) -> Self {
        Self { columns, vars }
    }

    /// Bind an IF/WHILE condition: must be logical.
    pub fn condition(&self, e: &Expr, keyword: &str) -> CmdResult<BExpr> {
        let b = self.bind(e)?;
        if b.ty != Ty::Bool {
            return Err(CommandError::new(
                format!("{keyword} needs a condition (true/false), but this is {}. Example: {keyword} Amount > 1000", b.ty.name()),
                e.span.start,
                e.span.end,
            ));
        }
        Ok(b)
    }

    /// Bind an expression that must be numeric (TOTAL, SUBTOTAL).
    pub fn numeric(&self, e: &Expr, what: &str) -> CmdResult<BExpr> {
        let b = self.bind(e)?;
        if b.ty != Ty::Num {
            return Err(CommandError::new(
                format!("{what} needs a number field, but this is {}", b.ty.name()),
                e.span.start,
                e.span.end,
            ));
        }
        Ok(b)
    }

    pub fn bind(&self, e: &Expr) -> CmdResult<BExpr> {
        let span = e.span.clone();
        let out = |sql: String, ty: Ty| Ok(BExpr { sql, ty, span: span.clone(), field: None });

        match &e.kind {
            ExprKind::Name(name) => self.bind_name(name, &span),
            ExprKind::Number(n) => {
                if n.parse::<f64>().is_err() {
                    return Err(CommandError::new(format!("'{n}' is not a valid number"), span.start, span.end));
                }
                out(n.clone(), Ty::Num)
            }
            ExprKind::Str(s) => out(quote_literal(s), Ty::Text),
            ExprKind::Date(d) => out(format!("DATE {}", quote_literal(d)), Ty::Date),
            ExprKind::DateTime(d) => out(format!("TIMESTAMP {}", quote_literal(d)), Ty::DateTime),
            ExprKind::Bool(b) => out(if *b { "TRUE".into() } else { "FALSE".into() }, Ty::Bool),

            ExprKind::Unary { op, expr } => {
                let inner = self.bind(expr)?;
                match op {
                    UnaryOp::Neg if inner.ty == Ty::Num => out(format!("(-{})", inner.sql), Ty::Num),
                    UnaryOp::Neg => Err(type_error(&span, format!("Can't make {} negative", inner.ty.name()))),
                    UnaryOp::Not if inner.ty == Ty::Bool => out(format!("(NOT {})", inner.sql), Ty::Bool),
                    UnaryOp::Not => Err(type_error(&span, format!("NOT needs a condition, not {}", inner.ty.name()))),
                }
            }

            ExprKind::Binary { op, left, right } => {
                let l = self.bind(left)?;
                let r = self.bind(right)?;
                self.bind_binary(*op, l, r, &span)
            }

            ExprKind::Call { name, args } => {
                let bound: Vec<BExpr> = args.iter().map(|a| self.bind(a)).collect::<CmdResult<_>>()?;
                functions::bind_call(name, bound, &span)
            }
        }
    }

    fn bind_name(&self, name: &str, span: &Span) -> CmdResult<BExpr> {
        // Fields first (exact, then case-insensitive), then variables.
        let col = self
            .columns
            .iter()
            .find(|c| c.name == name)
            .or_else(|| self.columns.iter().find(|c| c.name.eq_ignore_ascii_case(name)));
        if let Some(c) = col {
            return Ok(BExpr {
                sql: quote_ident(&c.name),
                ty: Ty::from_sql_type(&c.sql_type),
                span: span.clone(),
                field: Some(c.name.clone()),
            });
        }
        if let Some(v) = self.vars.get(&name.to_ascii_uppercase()) {
            return Ok(BExpr { sql: v.sql.clone(), ty: v.ty, span: span.clone(), field: None });
        }

        let mut candidates: Vec<&str> = self.columns.iter().map(|c| c.name.as_str()).collect();
        candidates.extend(self.vars.keys().map(String::as_str));
        let hint = suggest(name, &candidates).map(|s| format!(". Did you mean '{s}'?")).unwrap_or_default();
        let spaces = if name.contains(' ') { "" } else { " (field names with spaces go in brackets: [Txn Date])" };
        Err(CommandError::new(format!("Unknown field or variable '{name}'{hint}{}", if hint.is_empty() { spaces } else { "" }), span.start, span.end))
    }

    fn bind_binary(&self, op: BinaryOp, l: BExpr, r: BExpr, span: &Span) -> CmdResult<BExpr> {
        let out = |sql: String, ty: Ty| Ok(BExpr { sql, ty, span: span.clone(), field: None });
        use BinaryOp::*;
        match op {
            And | Or => {
                for side in [&l, &r] {
                    if side.ty != Ty::Bool {
                        return Err(type_error(&side.span, format!("AND/OR join conditions, but this is {}", side.ty.name())));
                    }
                }
                let kw = if op == And { "AND" } else { "OR" };
                out(format!("({} {kw} {})", l.sql, r.sql), Ty::Bool)
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                if !Ty::comparable(l.ty, r.ty) {
                    let hint = if l.ty.is_temporal() && r.ty == Ty::Text || r.ty.is_temporal() && l.ty == Ty::Text {
                        " — write dates as `YYYYMMDD`, e.g. `20240131`"
                    } else {
                        ""
                    };
                    return Err(type_error(span, format!("Can't compare {} with {}{hint}", l.ty.name(), r.ty.name())));
                }
                let sym = match op {
                    Eq => "=",
                    Ne => "<>",
                    Lt => "<",
                    Le => "<=",
                    Gt => ">",
                    _ => ">=",
                };
                out(format!("({} {sym} {})", l.sql, r.sql), Ty::Bool)
            }
            Add | Sub | Mul | Div => match (op, l.ty, r.ty) {
                (_, Ty::Num, Ty::Num) => {
                    let sym = match op {
                        Add => "+",
                        Sub => "-",
                        Mul => "*",
                        _ => "/",
                    };
                    let sql = if op == Div {
                        // Division by zero gives a blank value instead of failing the whole command.
                        format!("({} / NULLIF({}, 0))", l.sql, r.sql)
                    } else {
                        format!("({} {sym} {})", l.sql, r.sql)
                    };
                    out(sql, Ty::Num)
                }
                (Add, Ty::Text, Ty::Text) => out(format!("({} || {})", l.sql, r.sql), Ty::Text),
                (Add | Sub, Ty::Date, Ty::Num) => {
                    let sym = if op == Add { "+" } else { "-" };
                    out(format!("({} {sym} CAST({} AS INTEGER))", l.sql, r.sql), Ty::Date)
                }
                (Sub, Ty::Date, Ty::Date) => out(format!("({} - {})", l.sql, r.sql), Ty::Num),
                _ => Err(type_error(span, format!("Can't use {} with {} and {}", op_name(op), l.ty.name(), r.ty.name()))),
            },
        }
    }
}

fn op_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        _ => "this operator",
    }
}

pub fn type_error(span: &Span, message: String) -> CommandError {
    CommandError::new(message, span.start, span.end)
}

/// Closest candidate within a small edit distance.
fn suggest<'b>(name: &str, candidates: &[&'b str]) -> Option<&'b str> {
    let n = name.to_ascii_lowercase();
    candidates
        .iter()
        .map(|c| (levenshtein(&n, &c.to_ascii_lowercase()), *c))
        .filter(|(d, c)| *d <= 2.max(c.len() / 4))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}
