// src-tauri/src/analytics/dsl/ast.rs
//
// Shapes shared by every command: expressions and the scope clauses
// (IF / WHILE / FIRST / NEXT) that almost every ACL command accepts.
// Command-specific structs live next to their parsers in dsl/commands/.

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    pub fn to(&self, other: &Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Field or variable name (resolved by the binder).
    Name(String),
    Number(String),
    Str(String),
    Date(String),     // ISO yyyy-mm-dd
    DateTime(String), // ISO yyyy-mm-dd hh:mm:ss
    Bool(bool),
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FirstNextKind {
    First,
    Next,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstNext {
    pub kind: FirstNextKind,
    pub n: u64,
    pub span: Span,
}

/// IF / WHILE / FIRST / NEXT — written in any order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scope {
    pub first_next: Option<FirstNext>,
    pub while_: Option<Expr>,
    pub if_: Option<Expr>,
}

/// TO "name" — output to a permanent table.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub name: String,
    pub span: Span,
}
