// src-tauri/src/import.rs
//
// Import engine: reads a CSV/Excel/ODS file ONCE and stores it as a new
// table in a folder's DuckDB database. After this, nothing reads the
// original file again.
//
// Excel/ODS: calamine reads the sheet -> pass 1 infers a type per column ->
//            pass 2 bulk-loads with the DuckDB Appender.
// CSV/TSV:   DuckDB's own multi-threaded reader (read_csv_auto).

use calamine::{open_workbook_auto, Data, Reader};
use duckdb::types::{TimeUnit, Value};
use duckdb::{appender_params_from_iter, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::names::{quote_ident, quote_literal, table_name_from_path, unique_name};
use crate::query::{self, TableMeta};

const PROGRESS_EVERY: usize = 25_000;
const CANCEL_CHECK_EVERY: usize = 1_000;

pub const EXCEL_EXTS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];
pub const CSV_EXTS: &[&str] = &["csv", "tsv", "txt"];

#[derive(Serialize, Clone, Debug)]
pub struct ImportProgress {
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Excel,
    Csv,
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

// ---------------------------------------------------------------- public API

/// Sheet names of a workbook, in workbook order. Empty for CSV files.
pub fn list_sheets(file_path: &str) -> Result<Vec<String>, String> {
    match detect_kind(file_path)? {
        Kind::Csv => Ok(vec![]),
        Kind::Excel => {
            let wb = open_workbook_auto(file_path).map_err(|e| unreadable(file_path, &e.to_string()))?;
            Ok(wb.sheet_names().to_vec())
        }
    }
}

/// Import a file into a NEW table in `conn`'s database.
/// The table name comes from the file name and is made unique in that database
/// (ledger, ledger_2, ...). Existing tables are never replaced.
///
/// `cancel` is polled during the load; if it becomes true the partial table is
/// dropped and "Import cancelled" is returned.
pub fn import_file(
    conn: &Connection,
    file_path: &str,
    sheet: Option<&str>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(ImportProgress),
) -> Result<TableMeta, String> {
    let kind = detect_kind(file_path)?;
    let table = unique_name(&table_name_from_path(file_path), &query::list_tables(conn)?);

    let result = match kind {
        Kind::Csv => import_csv(conn, file_path, &table, &mut on_progress),
        Kind::Excel => import_workbook(conn, file_path, sheet, &table, cancel, &mut on_progress),
    };

    match result {
        Ok(()) if !cancel.load(Ordering::SeqCst) => query::table_meta(conn, &table),
        outcome => {
            // Failed or cancelled: never leave a half-loaded table behind.
            let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(&table)));
            if cancel.load(Ordering::SeqCst) {
                Err("Import cancelled".into())
            } else {
                Err(outcome.err().unwrap_or_else(|| "Import failed".into()))
            }
        }
    }
}

// ---------------------------------------------------------------- detection

fn detect_kind(file_path: &str) -> Result<Kind, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File not found: {file_path}"));
    }
    if !path.is_file() {
        return Err(format!("Not a file: {file_path}"));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if EXCEL_EXTS.contains(&ext.as_str()) {
        Ok(Kind::Excel)
    } else if CSV_EXTS.contains(&ext.as_str()) {
        Ok(Kind::Csv)
    } else {
        let supported: Vec<String> =
            EXCEL_EXTS.iter().chain(CSV_EXTS).map(|e| format!(".{e}")).collect();
        let shown = if ext.is_empty() { "(no extension)".to_string() } else { format!(".{ext}") };
        Err(format!("Unsupported file type {shown}. Supported formats: {}", supported.join(", ")))
    }
}

fn unreadable(file_path: &str, err: &str) -> String {
    format!(
        "Could not read '{file_path}'. The file may be corrupt, password-protected, \
         or not really the format its extension says ({err})"
    )
}

// ---------------------------------------------------------------- CSV

fn import_csv(
    conn: &Connection,
    file_path: &str,
    table: &str,
    on_progress: &mut impl FnMut(ImportProgress),
) -> Result<(), String> {
    on_progress(ImportProgress { done: 0, total: 0 });
    let sql = format!(
        "CREATE TABLE {} AS SELECT * FROM read_csv_auto({})",
        quote_ident(table),
        quote_literal(file_path)
    );
    // Cancellation of this statement happens through the connection's
    // interrupt handle (see commands.rs), which makes it return an error here.
    conn.execute_batch(&sql).map_err(|e| format!("CSV import failed: {e}"))?;
    let n = query::row_count(conn, table)?;
    on_progress(ImportProgress { done: n, total: n });
    Ok(())
}

// ---------------------------------------------------------------- Excel

fn import_workbook(
    conn: &Connection,
    file_path: &str,
    sheet: Option<&str>,
    table: &str,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(ImportProgress),
) -> Result<(), String> {
    let mut workbook = open_workbook_auto(file_path).map_err(|e| unreadable(file_path, &e.to_string()))?;
    let sheets = workbook.sheet_names().to_vec();

    let sheet_name = match sheet {
        Some(s) if sheets.iter().any(|x| x == s) => s.to_string(),
        Some(s) => {
            return Err(format!("Sheet '{s}' not found. Available sheets: {}", sheets.join(", ")))
        }
        None => sheets.first().cloned().ok_or("Workbook has no sheets")?,
    };

    // The ONE full read of the sheet.
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| unreadable(file_path, &format!("sheet '{sheet_name}': {e}")))?;

    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mut rows = range.rows();
    let header = rows.next().ok_or_else(|| format!("Sheet '{sheet_name}' is empty"))?;
    let width = range.width();
    let columns = column_names(header, width);
    // Excel row number of the header, for error messages (1-based).
    let first_excel_row = range.start().map(|(r, _)| r as usize + 1).unwrap_or(1);

    // Pass 1: infer a type per column from every data row.
    let mut types = vec![ColType::Unknown; width];
    for row in range.rows().skip(1) {
        for (i, cell) in row.iter().enumerate() {
            types[i] = merge_type(types[i], cell);
        }
    }

    let col_defs: Vec<String> = columns
        .iter()
        .zip(&types)
        .map(|(name, t)| format!("{} {}", quote_ident(name), t.sql()))
        .collect();
    conn.execute_batch(&format!("CREATE TABLE {} ({})", quote_ident(table), col_defs.join(", ")))
        .map_err(|e| format!("Failed to create table: {e}"))?;

    // Pass 2: bulk-load.
    let total = range.height().saturating_sub(1) as u64;
    let mut appender = conn.appender(table).map_err(|e| format!("Failed to start loading: {e}"))?;
    let mut done = 0usize;
    let mut values: Vec<Value> = Vec::with_capacity(width);

    for row in range.rows().skip(1) {
        values.clear();
        for i in 0..width {
            values.push(to_value(row.get(i).unwrap_or(&Data::Empty), types[i]));
        }
        if let Err(e) = appender.append_row(appender_params_from_iter(values.iter())) {
            let excel_row = first_excel_row + 1 + done;
            return Err(format!(
                "Row {excel_row} (sheet '{sheet_name}') could not be stored: {e}. Values: {}",
                preview_row(&columns, row)
            ));
        }

        done += 1;
        if done % CANCEL_CHECK_EVERY == 0 && cancel.load(Ordering::SeqCst) {
            return Ok(()); // caller sees the flag and drops the table
        }
        if done % PROGRESS_EVERY == 0 {
            on_progress(ImportProgress { done: done as u64, total });
        }
    }
    appender.flush().map_err(|e| format!("Failed to finish loading: {e}"))?;
    on_progress(ImportProgress { done: done as u64, total });
    Ok(())
}

/// "Amount='12.5', Date='2024-01-01', ..." (first 8 cells, each shortened)
fn preview_row(columns: &[String], row: &[Data]) -> String {
    columns
        .iter()
        .zip(row)
        .take(8)
        .map(|(c, v)| {
            let mut s = display(v);
            if s.len() > 40 {
                s.truncate(40);
                s.push('…');
            }
            format!("{c}='{s}'")
        })
        .collect::<Vec<_>>()
        .join(", ")
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
        _ => Varchar,
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
                dt.as_f64().to_string()
            }
        }
    }
}

/// Days-from-civil (Howard Hinnant) -> Unix microseconds.
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

fn column_names(header: &[Data], width: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    for i in 0..width {
        let raw = header.get(i).map(display).unwrap_or_default();
        let base = if raw.trim().is_empty() { format!("col_{}", i + 1) } else { raw.trim().to_string() };
        names.push(unique_name(&base, &names));
    }
    names
}
