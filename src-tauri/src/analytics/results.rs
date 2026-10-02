// src-tauri/src/analytics/results.rs
//
// Session results and the command log, stored inside each folder's database:
//
//   _results.r_1, r_2, …     tables produced by commands without TO
//   _meta.results            registry: label, command, source, row count
//   _meta.command_log        every command run (ok or failed), with its SQL
//
// Neither schema is listed in the sidebar (list_tables only shows `main`).
// Results are cleared the first time a folder is used after the app starts,
// and only the newest MAX_RESULTS are kept.

use duckdb::Connection;
use serde::Serialize;

use crate::names::quote_ident;
use crate::query::{self, is_result_id, TableMeta, TableRef, RESULTS_SCHEMA};

pub const META_SCHEMA: &str = "_meta";
pub const MAX_RESULTS: usize = 30;

#[derive(Serialize, Clone, Debug)]
pub struct ResultInfo {
    /// "_results.r_7" — pass this as the table to the grid or to the next command.
    pub table: String,
    pub label: String,
    pub command: String,
    pub source: String,
    pub row_count: u64,
    pub created_at: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LogEntry {
    pub id: i64,
    pub at: String,
    pub command: String,
    pub input: String,
    pub ok: bool,
    /// Result table, or "COUNT1 = 972" for scalars, or the error message.
    pub output: String,
    pub row_count: Option<i64>,
    pub duration_ms: i64,
    pub sql: String,
}

fn e(err: duckdb::Error) -> String {
    err.to_string()
}

pub fn ensure(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&format!(
        "CREATE SCHEMA IF NOT EXISTS {r};
         CREATE SCHEMA IF NOT EXISTS {m};
         CREATE TABLE IF NOT EXISTS {m}.results (
             id VARCHAR PRIMARY KEY, seq BIGINT, label VARCHAR, command VARCHAR,
             source VARCHAR, row_count BIGINT, created_at TIMESTAMP DEFAULT current_timestamp);
         CREATE TABLE IF NOT EXISTS {m}.command_log (
             id BIGINT, logged_at TIMESTAMP DEFAULT current_timestamp, command VARCHAR, input VARCHAR,
             ok BOOLEAN, output VARCHAR, row_count BIGINT, duration_ms BIGINT, sql VARCHAR);",
        r = quote_ident(RESULTS_SCHEMA),
        m = quote_ident(META_SCHEMA),
    ))
    .map_err(e)
}

/// Drop every session result (start of a new app session).
pub fn clear_session(conn: &Connection) -> Result<(), String> {
    for id in result_tables(conn)? {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", TableRef::result(&id).sql())).map_err(e)?;
    }
    conn.execute_batch(&format!("DELETE FROM {}.results", quote_ident(META_SCHEMA))).map_err(e)
}

fn result_tables(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT table_name FROM information_schema.tables WHERE table_schema = ?")
        .map_err(e)?;
    let rows = stmt.query_map([RESULTS_SCHEMA], |r| r.get::<_, String>(0)).map_err(e)?;
    let all: Vec<String> = rows.collect::<Result<_, _>>().map_err(e)?;
    Ok(all.into_iter().filter(|t| is_result_id(t)).collect())
}

/// Next free result id: r_<n>.
pub fn next_result(conn: &Connection) -> Result<(TableRef, i64), String> {
    let max_registered: i64 = conn
        .query_row(&format!("SELECT coalesce(max(seq), 0) FROM {}.results", quote_ident(META_SCHEMA)), [], |r| r.get(0))
        .map_err(e)?;
    let max_existing = result_tables(conn)?
        .iter()
        .filter_map(|t| t.strip_prefix("r_").and_then(|n| n.parse::<i64>().ok()))
        .max()
        .unwrap_or(0);
    let seq = max_registered.max(max_existing) + 1;
    Ok((TableRef::result(&format!("r_{seq}")), seq))
}

pub fn register(
    conn: &Connection,
    table: &TableRef,
    seq: i64,
    label: &str,
    command: &str,
    source: &str,
    row_count: u64,
) -> Result<(), String> {
    conn.execute(
        &format!(
            "INSERT INTO {}.results (id, seq, label, command, source, row_count) VALUES (?, ?, ?, ?, ?, ?)",
            quote_ident(META_SCHEMA)
        ),
        duckdb::params![table.name, seq, label, command, source, row_count as i64],
    )
    .map_err(e)?;
    prune(conn)
}

/// Keep only the newest MAX_RESULTS results.
fn prune(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id FROM {}.results ORDER BY seq DESC OFFSET {MAX_RESULTS}",
            quote_ident(META_SCHEMA)
        ))
        .map_err(e)?;
    let old: Vec<String> = stmt.query_map([], |r| r.get(0)).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
    for id in old {
        delete(conn, &id)?;
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<ResultInfo>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, label, command, source, row_count, CAST(created_at AS VARCHAR)
             FROM {}.results ORDER BY seq DESC",
            quote_ident(META_SCHEMA)
        ))
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ResultInfo {
                table: TableRef::result(&r.get::<_, String>(0)?).id(),
                label: r.get(1)?,
                command: r.get(2)?,
                source: r.get(3)?,
                row_count: r.get::<_, i64>(4)? as u64,
                created_at: r.get(5)?,
            })
        })
        .map_err(e)?;
    rows.collect::<Result<_, _>>().map_err(e)
}

/// Remove one result (table + registry row). `id` is "r_7".
pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
    if !is_result_id(id) {
        return Err(format!("Invalid result id '{id}'"));
    }
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", TableRef::result(id).sql())).map_err(e)?;
    conn.execute(&format!("DELETE FROM {}.results WHERE id = ?", quote_ident(META_SCHEMA)), [id])
        .map_err(e)?;
    Ok(())
}

/// Checks a name for a new permanent table.
pub fn validate_new_table_name(conn: &Connection, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Table name cannot be empty".into());
    }
    if name.starts_with('_') {
        return Err("Table names starting with _ are reserved".into());
    }
    if query::list_tables(conn)?.iter().any(|t| t.eq_ignore_ascii_case(name)) {
        return Err(format!("A table named '{name}' already exists in this folder"));
    }
    Ok(name.to_string())
}

/// Copy a session result into a permanent table.
pub fn save_as_table(conn: &Connection, result: &str, name: &str) -> Result<TableMeta, String> {
    let r = TableRef::parse(result);
    if !r.is_result() {
        return Err(format!("'{result}' is not a session result"));
    }
    query::columns(conn, &r.id())?; // exists?
    let name = validate_new_table_name(conn, name)?;
    conn.execute_batch(&format!(
        "CREATE TABLE {} AS SELECT * FROM {} ORDER BY rowid",
        TableRef::main(&name).sql(),
        r.sql()
    ))
    .map_err(e)?;
    query::table_meta(conn, &name)
}

// ---------------------------------------------------------------- command log

pub struct NewLogEntry<'a> {
    pub command: &'a str,
    pub input: &'a str,
    pub ok: bool,
    pub output: &'a str,
    pub row_count: Option<i64>,
    pub duration_ms: i64,
    pub sql: &'a str,
}

pub fn log(conn: &Connection, entry: NewLogEntry) -> Result<i64, String> {
    let m = quote_ident(META_SCHEMA);
    let id: i64 = conn
        .query_row(&format!("SELECT coalesce(max(id), 0) + 1 FROM {m}.command_log"), [], |r| r.get(0))
        .map_err(e)?;
    conn.execute(
        &format!(
            "INSERT INTO {m}.command_log (id, command, input, ok, output, row_count, duration_ms, sql)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        ),
        duckdb::params![id, entry.command, entry.input, entry.ok, entry.output, entry.row_count, entry.duration_ms, entry.sql],
    )
    .map_err(e)?;
    Ok(id)
}

pub fn history(conn: &Connection, limit: u64) -> Result<Vec<LogEntry>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, CAST(logged_at AS VARCHAR), command, input, ok, output, row_count, duration_ms, sql
             FROM {}.command_log ORDER BY id DESC LIMIT {}",
            quote_ident(META_SCHEMA),
            limit.min(1000)
        ))
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(LogEntry {
                id: r.get(0)?,
                at: r.get(1)?,
                command: r.get(2)?,
                input: r.get(3)?,
                ok: r.get(4)?,
                output: r.get(5)?,
                row_count: r.get(6)?,
                duration_ms: r.get(7)?,
                sql: r.get(8)?,
            })
        })
        .map_err(e)?;
    rows.collect::<Result<_, _>>().map_err(e)
}
