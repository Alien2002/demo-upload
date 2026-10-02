// src-tauri/src/query.rs
//
// Table catalog helpers: list tables and read a table's columns and row count.
// Row fetching lives in query_engine.rs.
//
// Table references are strings so the frontend can pass them around as-is:
//   "ledger"           -> main.ledger        (permanent table, in the sidebar)
//   "_results.r_12"    -> _results.r_12      (session result of an ACL command)

use duckdb::Connection;
use serde::Serialize;

use crate::names::quote_ident;

pub const RESULTS_SCHEMA: &str = "_results";

#[derive(Serialize, Clone, Debug)]
pub struct ColumnInfo {
    pub name: String,
    pub sql_type: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct TableMeta {
    pub table: String,
    pub row_count: u64,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

impl TableRef {
    pub fn main(name: &str) -> Self {
        Self { schema: "main".into(), name: name.into() }
    }

    pub fn result(id: &str) -> Self {
        Self { schema: RESULTS_SCHEMA.into(), name: id.into() }
    }

    /// "_results.r_12" -> result r_12; anything else -> main table.
    pub fn parse(s: &str) -> Self {
        match s.strip_prefix("_results.") {
            Some(id) if is_result_id(id) => Self::result(id),
            _ => Self::main(s),
        }
    }

    /// The string form used by the frontend.
    pub fn id(&self) -> String {
        if self.schema == "main" {
            self.name.clone()
        } else {
            format!("{}.{}", self.schema, self.name)
        }
    }

    /// Quoted SQL reference.
    pub fn sql(&self) -> String {
        format!("{}.{}", quote_ident(&self.schema), quote_ident(&self.name))
    }

    pub fn is_result(&self) -> bool {
        self.schema == RESULTS_SCHEMA
    }
}

/// Result table ids look like r_12.
pub fn is_result_id(id: &str) -> bool {
    id.strip_prefix("r_").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn list_tables(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'main' ORDER BY lower(table_name)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Columns in table order. Errors if the table doesn't exist, which makes
/// this the validation step before any table name is used in SQL.
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>, String> {
    let t = TableRef::parse(table);
    let mut stmt = conn
        .prepare(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = ? AND table_name = ?
             ORDER BY ordinal_position",
        )
        .map_err(|e| e.to_string())?;
    let cols: Vec<ColumnInfo> = stmt
        .query_map([&t.schema, &t.name], |r| Ok(ColumnInfo { name: r.get(0)?, sql_type: r.get(1)? }))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if cols.is_empty() {
        return Err(if t.is_result() {
            format!("Result '{}' is no longer available (results are cleared when the app restarts)", t.name)
        } else {
            format!("Table '{table}' not found")
        });
    }
    Ok(cols)
}

pub fn row_count(conn: &Connection, table: &str) -> Result<u64, String> {
    let n: i64 = conn
        .query_row(&format!("SELECT count(*) FROM {}", TableRef::parse(table).sql()), [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(n as u64)
}

pub fn table_meta(conn: &Connection, table: &str) -> Result<TableMeta, String> {
    let columns = columns(conn, table)?;
    let row_count = row_count(conn, table)?;
    Ok(TableMeta { table: table.to_string(), row_count, columns })
}
