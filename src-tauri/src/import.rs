// src-tauri/src/import.rs
//
// Import engine: reads an Excel/ODS/CSV file ONCE and stores it as a
// DuckDB table. After this, the grid never touches the original file.

use calamine::{open_workbook_auto, Data, Reader};
use duckdb::types::{TimeUnit, Value};
use duckdb::{appender_params_from_iter, Connection};
use serde::Serialize;
use std::path::Path;

use crate::query::{table_meta, TableMeta};

const PROGRESS_EVERY: usize = 25_000;

#[derive(Serialize, Clone)]
pub struct ImportProgress {
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ColType {
    Unknown, // only empty cells seen so far
    Bool,
    BigInt,
    Double,
    Timestamp,
    Varchar,
}

impl ColType {
    fn sql(self) -> &'static str {
        match self {
            ColType::Bool => "BOOLEAN",
            ColType::BigInt => "BIGINT",
            ColType::Double => "DOUBLE",
            ColType::Timestamp => "TIMESTAMP",
            ColType::Unknown | ColType::Varchar => "VARCHAR",
        }
    }
}

/// Entry point used by the Tauri command.
pub fn import_file(
    conn: &Connection,
    path: &str,
    sheet: Option<&str>,
    mut on_progress: impl FnMut(ImportProgress),
) -> Result<TableMeta, String> {
    let table = table_name_from_path(path);
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "csv" | "tsv" | "txt" => import_csv(conn, path, &table)?,
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            import_workbook(conn, path, sheet, &table, &mut on_progress)?
        }
        other => return Err(format!("Unsupported file type: .{other}")),
    }

    table_meta(conn, &table)
}

// ---------------------------------------------------------------- CSV

fn import_csv(conn: &Connection, path: &str, table: &str) -> Result<(), String> {
    // DuckDB's own CSV reader is multi-threaded and infers types itself.
    let sql = format!(
        "CREATE OR REPLACE TABLE {} AS SELECT * FROM read_csv_auto({})",
        quote_ident(table),
        quote_literal(path)
    );
    conn.execute_batch(&sql).map_err(|e| format!("CSV import failed: {e}"))
}

// ---------------------------------------------------------------- Excel

fn import_workbook(
    conn: &Connection,
    path: &str,
    sheet: Option<&str>,
    table: &str,
    on_progress: &mut impl FnMut(ImportProgress),
) -> Result<(), String> {
    let mut workbook =
        open_workbook_auto(path).map_err(|e| format!("Failed to open workbook: {e}"))?;

    let sheet_name = match sheet {
        Some(s) => s.to_string(),
        None => workbook
            .sheet_names()
            .first()
            .cloned()
            .ok_or("Workbook has no sheets")?,
    };

    // The ONE full read of the sheet. Everything after this is from DuckDB.
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Failed to read sheet '{sheet_name}': {e}"))?;

    let mut rows = range.rows();
    let header = rows.next().ok_or("Sheet is empty")?;
    let width = range.width();
    let columns = column_names(header, width);

    // Pass 1: infer a type per column from every data row.
    let mut types = vec![ColType::Unknown; width];
    for row in range.rows().skip(1) {
        for (i, cell) in row.iter().enumerate() {
            types[i] = merge_type(types[i], cell);
        }
    }

    // Create the table.
    let col_defs: Vec<String> = columns
        .iter()
        .zip(&types)
        .map(|(name, t)| format!("{} {}", quote_ident(name), t.sql()))
        .collect();
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TABLE {} ({})",
        quote_ident(table),
        col_defs.join(", ")
    ))
    .map_err(|e| format!("Failed to create table: {e}"))?;

    // Pass 2: bulk-load with the Appender (much faster than INSERTs).
    let total = (range.height().saturating_sub(1)) as u64;
    let mut appender = conn
        .appender(table)
        .map_err(|e| format!("Failed to create appender: {e}"))?;

    let mut done = 0usize;
    let mut values: Vec<Value> = Vec::with_capacity(width);
    for row in range.rows().skip(1) {
        values.clear();
        for i in 0..width {
            let cell = row.get(i).unwrap_or(&Data::Empty);
            values.push(to_value(cell, types[i]));
        }
        appender
            .append_row(appender_params_from_iter(values.iter()))
            .map_err(|e| format!("Row {} failed: {e}", done + 2))?;

        done += 1;
        if done % PROGRESS_EVERY == 0 {
            on_progress(ImportProgress { done: done as u64, total });
        }
    }
    appender.flush().map_err(|e| e.to_string())?;
    on_progress(ImportProgress { done: done as u64, total });
    Ok(())
}

/// Widen a column's type to fit a new cell. Conflicts fall back to VARCHAR,
/// so nothing is ever lost (e.g. account numbers like "00123" stay text).
fn merge_type(current: ColType, cell: &Data) -> ColType {
    use ColType::*;
    let seen = match cell {
        Data::Empty | Data::Error(_) => return current,
        Data::Bool(_) => Bool,
        Data::Int(_) => BigInt,
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 9.0e15 {
                BigInt
            } else {
                Double
            }
        }
        Data::DateTime(dt) if dt.is_datetime() => Timestamp,
        _ => Varchar, // strings, durations, ISO strings
    };
    match (current, seen) {
        (Unknown, s) => s,
        (c, s) if c == s => c,
        (BigInt, Double) | (Double, BigInt) => Double,
        _ => Varchar,
    }
}

fn to_value(cell: &Data, ty: ColType) -> Value {
    match (cell, ty) {
        (Data::Empty | Data::Error(_), _) => Value::Null,
        (Data::Bool(b), ColType::Bool) => Value::Boolean(*b),
        (Data::Int(i), ColType::BigInt) => Value::BigInt(*i),
        (Data::Float(f), ColType::BigInt) => Value::BigInt(*f as i64),
        (Data::Int(i), ColType::Double) => Value::Double(*i as f64),
        (Data::Float(f), ColType::Double) => Value::Double(*f),
        (Data::DateTime(dt), ColType::Timestamp) => {
            let (y, m, d, hh, mm, ss, ms) = dt.to_ymd_hms_milli();
            Value::Timestamp(TimeUnit::Microsecond, to_unix_micros(y, m, d, hh, mm, ss, ms))
        }
        // Anything else lands in a VARCHAR column as display text.
        (other, _) => Value::Text(display(other)),
    }
}

fn display(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => {
            let (y, m, d, hh, mm, ss, _) = dt.to_ymd_hms_milli();
            if dt.is_datetime() {
                if hh == 0 && mm == 0 && ss == 0 {
                    format!("{y:04}-{m:02}-{d:02}")
                } else {
                    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
                }
            } else {
                dt.as_f64().to_string() // duration in days
            }
        }
    }
}

/// Days-from-civil (Howard Hinnant) → Unix microseconds.
fn to_unix_micros(y: u16, m: u8, d: u8, hh: u8, mm: u8, ss: u8, ms: u16) -> i64 {
    let (y, m, d) = (y as i64, m as i64, d as i64);
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh as i64 * 3_600 + mm as i64 * 60 + ss as i64;
    secs * 1_000_000 + ms as i64 * 1_000
}

// ---------------------------------------------------------------- names

fn column_names(header: &[Data], width: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    for i in 0..width {
        let raw = header.get(i).map(display).unwrap_or_default();
        let base = if raw.trim().is_empty() {
            format!("col_{}", i + 1)
        } else {
            raw.trim().to_string()
        };
        let mut name = base.clone();
        let mut n = 2;
        while names.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names.push(name);
    }
    names
}

pub fn table_name_from_path(path: &str) -> String {
    let stem = Path::new(path)
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

pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
