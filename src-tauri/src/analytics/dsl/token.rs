// src-tauri/src/analytics/dsl/token.rs
//
// Keywords are NOT separate tokens: they come out of the lexer as Ident and
// the parser recognises them by position. That way a field can still be
// called "Next" or "Total" ([Next] in brackets inside expressions).

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Bare word: keyword, field, variable or function name.
    Ident(String),
    /// [Field Name With Spaces] — always a field/variable, never a keyword.
    QuotedIdent(String),
    /// Number as written (kept as text so nothing is lost before SQL).
    Number(String),
    /// "text" or 'text'
    Str(String),
    /// `20240131` or `20240131 143000` (ACL date literal), normalised to ISO.
    Date(String),
    DateTime(String),

    Eq,  // = or ==
    Ne,  // <> or !=
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    And, // AND or &
    Or,  // OR or |
    Not, // NOT or !  (also recognised as Ident "NOT")
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    /// Character offsets (end exclusive).
    pub start: usize,
    pub end: usize,
}

impl Token {
    /// Human-readable form for error messages.
    pub fn describe(&self) -> String {
        match self {
            Token::Ident(s) => format!("'{s}'"),
            Token::QuotedIdent(s) => format!("[{s}]"),
            Token::Number(n) => n.clone(),
            Token::Str(s) => format!("\"{s}\""),
            Token::Date(d) | Token::DateTime(d) => format!("`{d}`"),
            Token::Eq => "=".into(),
            Token::Ne => "<>".into(),
            Token::Lt => "<".into(),
            Token::Le => "<=".into(),
            Token::Gt => ">".into(),
            Token::Ge => ">=".into(),
            Token::Plus => "+".into(),
            Token::Minus => "-".into(),
            Token::Star => "*".into(),
            Token::Slash => "/".into(),
            Token::LParen => "(".into(),
            Token::RParen => ")".into(),
            Token::Comma => ",".into(),
            Token::And => "AND".into(),
            Token::Or => "OR".into(),
            Token::Not => "NOT".into(),
            Token::Eof => "end of command".into(),
        }
    }
}
