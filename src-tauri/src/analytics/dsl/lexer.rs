// src-tauri/src/analytics/dsl/lexer.rs
//
// Command text -> tokens with character spans.

use super::token::{Spanned, Token};
use crate::analytics::error::{CmdResult, CommandError};

pub fn tokenize(input: &str) -> CmdResult<Vec<Spanned>> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let start = i;

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        let simple = |t: Token, len: usize| Spanned { token: t, start, end: start + len };

        let tok = match c {
            '(' => simple(Token::LParen, 1),
            ')' => simple(Token::RParen, 1),
            ',' => simple(Token::Comma, 1),
            '+' => simple(Token::Plus, 1),
            '-' => simple(Token::Minus, 1),
            '*' => simple(Token::Star, 1),
            '/' => simple(Token::Slash, 1),
            '&' => simple(Token::And, 1),
            '|' => simple(Token::Or, 1),
            '=' => {
                if chars.get(i + 1) == Some(&'=') {
                    simple(Token::Eq, 2)
                } else {
                    simple(Token::Eq, 1)
                }
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    simple(Token::Ne, 2)
                } else {
                    simple(Token::Not, 1)
                }
            }
            '<' => match chars.get(i + 1) {
                Some('=') => simple(Token::Le, 2),
                Some('>') => simple(Token::Ne, 2),
                _ => simple(Token::Lt, 1),
            },
            '>' => match chars.get(i + 1) {
                Some('=') => simple(Token::Ge, 2),
                _ => simple(Token::Gt, 1),
            },
            '"' | '\'' => {
                let quote = c;
                let mut j = i + 1;
                let mut s = String::new();
                loop {
                    match chars.get(j) {
                        None => return Err(CommandError::new("Text is missing its closing quote", start, j)),
                        // A doubled quote inside the text is a literal quote: "say ""hi"""
                        Some(&q) if q == quote && chars.get(j + 1) == Some(&quote) => {
                            s.push(quote);
                            j += 2;
                        }
                        Some(&q) if q == quote => break,
                        Some(&ch) => {
                            s.push(ch);
                            j += 1;
                        }
                    }
                }
                Spanned { token: Token::Str(s), start, end: j + 1 }
            }
            '`' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '`' {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(CommandError::new("Date is missing its closing `", start, j));
                }
                let raw: String = chars[i + 1..j].iter().collect();
                let token = parse_date_literal(raw.trim())
                    .ok_or_else(|| CommandError::new(
                        format!("`{raw}` is not a valid date; use `YYYYMMDD` or `YYYYMMDD hhmmss`"),
                        start,
                        j + 1,
                    ))?;
                Spanned { token, start, end: j + 1 }
            }
            '[' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(CommandError::new("Field name is missing its closing ]", start, j));
                }
                let name: String = chars[i + 1..j].iter().collect();
                if name.trim().is_empty() {
                    return Err(CommandError::new("Empty field name []", start, j + 1));
                }
                Spanned { token: Token::QuotedIdent(name.trim().to_string()), start, end: j + 1 }
            }
            c if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let mut j = i;
                let mut seen_dot = false;
                while j < chars.len() && (chars[j].is_ascii_digit() || (chars[j] == '.' && !seen_dot)) {
                    if chars[j] == '.' {
                        seen_dot = true;
                    }
                    j += 1;
                }
                let n: String = chars[i..j].iter().collect();
                Spanned { token: Token::Number(n), start, end: j }
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let token = match word.to_ascii_uppercase().as_str() {
                    "AND" => Token::And,
                    "OR" => Token::Or,
                    "NOT" => Token::Not,
                    _ => Token::Ident(word),
                };
                Spanned { token, start, end: j }
            }
            other => {
                return Err(CommandError::new(format!("Unexpected character '{other}'"), start, start + 1));
            }
        };

        i = tok.end;
        out.push(tok);
    }

    out.push(Spanned { token: Token::Eof, start: chars.len(), end: chars.len() });
    Ok(out)
}

/// `20240131` -> Date("2024-01-31"); `20240131 143000` / `20240131T1430` -> DateTime.
fn parse_date_literal(raw: &str) -> Option<Token> {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let (date, time) = match raw.split_once(|c: char| c == ' ' || c == 'T' || c == 't') {
        Some((d, t)) => (d.trim(), Some(t.trim())),
        None => (raw, None),
    };
    if date.len() != 8 || !digits(date) {
        return None;
    }
    let (y, m, d) = (&date[0..4], &date[4..6], &date[6..8]);
    let (mi, di): (u32, u32) = (m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&mi) || !(1..=31).contains(&di) {
        return None;
    }
    let iso = format!("{y}-{m}-{d}");
    match time {
        None => Some(Token::Date(iso)),
        Some(t) if digits(t) && (t.len() == 4 || t.len() == 6) => {
            let (hh, mm) = (&t[0..2], &t[2..4]);
            let ss = if t.len() == 6 { &t[4..6] } else { "00" };
            if hh.parse::<u32>().ok()? > 23 || mm.parse::<u32>().ok()? > 59 || ss.parse::<u32>().ok()? > 59 {
                return None;
            }
            Some(Token::DateTime(format!("{iso} {hh}:{mm}:{ss}")))
        }
        _ => None,
    }
}
