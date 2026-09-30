// src-tauri/src/names.rs
//
// Name sanitizing and SQL quoting shared by every module.

use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SLUG_LEN: usize = 64;

/// Windows refuses these as file/folder names (with or without extension).
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Turn a display name into a safe file/folder name that works on Windows and Linux.
/// "Q3 Audit / Payroll" -> "q3-audit-payroll"
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let mut out = out.trim_end_matches('-').to_string();
    out.truncate(MAX_SLUG_LEN);
    let out = out.trim_end_matches('-').to_string();
    if RESERVED.contains(&out.as_str()) {
        format!("{out}_")
    } else {
        out
    }
}

/// True if `id` is already a clean slug. Used to reject ids like "../../etc"
/// coming from the frontend before they are joined onto a path.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && slugify(id) == id
}

/// SQL table name from a file name: "Ledger 2024.xlsx" -> "ledger_2024"
pub fn table_name_from_path(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");
    let mut name: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name = format!("t_{name}");
    }
    name
}

/// First name not in `taken` (case-insensitive): base, base_2, base_3, ...
pub fn unique_name(base: &str, taken: &[String]) -> String {
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}_{i}"))
        .find(|n| !is_taken(n))
        .expect("infinite iterator")
}

pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Milliseconds since 1970-01-01 UTC (JavaScript `new Date(ms)` friendly).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
