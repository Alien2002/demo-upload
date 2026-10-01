// src-tauri/src/import.rs
//
// Import engine, step 1: read a CSV/Excel/ODS file ONCE into a text-only
// staging table (see staging.rs for review, type checks and commit).
//
// Excel/ODS: calamine reads the sheet; every cell is written as text
//            (dates as ISO 8601 so they convert back without loss).
// CSV/TSV:   DuckDB's multi-threaded reader with all_varchar = true.
//
// Keeping everything as text here means one conversion path (SQL) for both
// formats, and the user decides the final types.

use calamine::{open_workbook_auto, Data, Reader};
use duckdb::types::Value;
use duckdb::{appender_params_from_iter, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::names::{quote_ident, quote_literal, unique_name};
use crate::query::TableMeta;
use crate::staging::{self, StagedImport};

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

/// Read the file into a new staging table and profile its columns.
/// On failure or cancel nothing is left behind.
pub fn stage_file(
    conn: &Connection,
    file_path: &str,
    sheet: Option<&str>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(ImportProgress),
) -> Result<StagedImport, String> {
    let kind = detect_kind(file_path)?;
    staging::ensure_schema(conn)?;
    let id = staging::new_staging_id();

    let loaded = match kind {
        Kind::Csv => stage_csv(conn, file_path, &id, &mut on_progress).map(|_| (vec![], None)),
        Kind::Excel => stage_workbook(conn, file_path, sheet, &id, cancel, &mut on_progress)
            .map(|(errs, name)| (errs, Some(name))),
    };

    let (excel_errors, sheet_name) = match loaded {
        Ok(v) if !cancel.load(Ordering::SeqCst) => v,
        outcome => {
            let _ = staging::discard(conn, &id);
            return Err(if cancel.load(Ordering::SeqCst) {
                "Import cancelled".into()
            } else {
                outcome.err().unwrap_or_else(|| "Import failed".into())
            });
        }
    };

    let result = (|| {
        let columns = staging::profile(conn, &id, &excel_errors)?;
        let row_count: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {}", staging::staged_ref(&id)), [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        Ok(StagedImport {
            staging_id: id.clone(),
            file_path: file_path.to_string(),
            sheet: sheet_name,
            row_count: row_count as u64,
            suggested_table: staging::suggest_table_name(conn, file_path)?,
            columns,
        })
    })();

    if result.is_err() {
        let _ = staging::discard(conn, &id);
    }
    result
}

/// One-step import: stage, accept every suggestion, commit.
pub fn import_file(
    conn: &Connection,
    file_path: &str,
    sheet: Option<&str>,
    cancel: &AtomicBool,
    on_progress: impl FnMut(ImportProgress),
) -> Result<TableMeta, String> {
    let staged = stage_file(conn, file_path, sheet, cancel, on_progress)?;
    let specs = staging::default_specs(&staged.columns);
    staging::commit(conn, &staged.staging_id, &staged.suggested_table, &specs).inspect_err(|_| {
        let _ = staging::discard(conn, &staged.staging_id);
    })
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
        let supported: Vec<String> = EXCEL_EXTS.iter().chain(CSV_EXTS).map(|e| format!(".{e}")).collect();
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

fn stage_csv(
    conn: &Connection,
    file_path: &str,
    id: &str,
    on_progress: &mut impl FnMut(ImportProgress),
) -> Result<(), String> {
    on_progress(ImportProgress { done: 0, total: 0 });
    // all_varchar: keep the text exactly as written ("00123", "31/01/2024").
    // header = true: with all-text columns the sniffer can't tell a header row
    // from data, so we state it.
    let sql = format!(
        "CREATE TABLE {} AS SELECT * FROM read_csv({}, all_varchar = true, header = true)",
        staging::staged_ref(id),
        quote_literal(file_path)
    );
    // Cancelled through the connection's interrupt handle (commands.rs).
    conn.execute_batch(&sql).map_err(|e| format!("CSV import failed: {e}"))?;
    let n: i64 = conn
        .query_row(&format!("SELECT count(*) FROM {}", staging::staged_ref(id)), [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    on_progress(ImportProgress { done: n as u64, total: n as u64 });
    Ok(())
}

// ---------------------------------------------------------------- Excel

/// Returns (Excel error cells per column, sheet name used).
fn stage_workbook(
    conn: &Connection,
    file_path: &str,
    sheet: Option<&str>,
    id: &str,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(ImportProgress),
) -> Result<(Vec<u64>, String), String> {
    let mut workbook = open_workbook_auto(file_path).map_err(|e| unreadable(file_path, &e.to_string()))?;
    let sheets = workbook.sheet_names().to_vec();

    let sheet_name = match sheet {
        Some(s) if sheets.iter().any(|x| x == s) => s.to_string(),
        Some(s) => return Err(format!("Sheet '{s}' not found. Available sheets: {}", sheets.join(", "))),
        None => sheets.first().cloned().ok_or("Workbook has no sheets")?,
    };

    // The ONE full read of the sheet.
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| unreadable(file_path, &format!("sheet '{sheet_name}': {e}")))?;
    if cancel.load(Ordering::SeqCst) {
        return Ok((vec![], sheet_name));
    }

    let mut rows = range.rows();
    let header = rows.next().ok_or_else(|| format!("Sheet '{sheet_name}' is empty"))?;
    let width = range.width();
    let columns = column_names(header, width);
    let first_excel_row = range.start().map(|(r, _)| r as usize + 1).unwrap_or(1);

    let col_defs: Vec<String> = columns.iter().map(|c| format!("{} VARCHAR", quote_ident(c))).collect();
    conn.execute_batch(&format!("CREATE TABLE {} ({})", staging::staged_ref(id), col_defs.join(", ")))
        .map_err(|e| format!("Failed to create staging table: {e}"))?;

    let total = range.height().saturating_sub(1) as u64;
    let mut appender = conn
        .appender_to_db(id, staging::SCHEMA)
        .map_err(|e| format!("Failed to start loading: {e}"))?;
    let mut errors = vec![0u64; width];
    let mut values: Vec<Value> = Vec::with_capacity(width);
    let mut done = 0usize;

    for row in range.rows().skip(1) {
        values.clear();
        for i in 0..width {
            let cell = row.get(i).unwrap_or(&Data::Empty);
            values.push(match cell {
                Data::Empty => Value::Null,
                Data::Error(_) => {
                    errors[i] += 1;
                    Value::Null
                }
                other => Value::Text(to_text(other)),
            });
        }
        appender.append_row(appender_params_from_iter(values.iter())).map_err(|e| {
            format!("Row {} (sheet '{sheet_name}') could not be stored: {e}", first_excel_row + 1 + done)
        })?;

        done += 1;
        if done % CANCEL_CHECK_EVERY == 0 && cancel.load(Ordering::SeqCst) {
            return Ok((errors, sheet_name));
        }
        if done % PROGRESS_EVERY == 0 {
            on_progress(ImportProgress { done: done as u64, total });
        }
    }
    appender.flush().map_err(|e| format!("Failed to finish loading: {e}"))?;
    on_progress(ImportProgress { done: done as u64, total });
    Ok((errors, sheet_name))
}

/// Cell -> text that DuckDB can convert back exactly.
fn to_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) if dt.is_datetime() => {
            let (y, m, d, hh, mm, ss, ms) = dt.to_ymd_hms_milli();
            match (hh, mm, ss, ms) {
                (0, 0, 0, 0) => format!("{y:04}-{m:02}-{d:02}"),
                (_, _, _, 0) => format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}"),
                _ => format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{ms:03}"),
            }
        }
        Data::DateTime(dt) => dt.as_f64().to_string(), // duration, in days
    }
}

fn column_names(header: &[Data], width: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    for i in 0..width {
        let raw = header.get(i).map(to_text).unwrap_or_default();
        let base = if raw.trim().is_empty() { format!("col_{}", i + 1) } else { raw.trim().to_string() };
        names.push(unique_name(&base, &names));
    }
    names
}
