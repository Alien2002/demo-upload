// src-tauri/src/query.rs
//
// Table catalog helpers: list tables and read a table's columns and row count.
// Row fetching lives in query_engine.rs.

use duckdb::Connection;
use serde::Serialize;

use crate::names::quote_ident;

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
    let mut stmt = conn
        .prepare(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = 'main' AND table_name = ?
             ORDER BY ordinal_position",
        )
        .map_err(|e| e.to_string())?;
    let cols: Vec<ColumnInfo> = stmt
        .query_map([table], |r| Ok(ColumnInfo { name: r.get(0)?, sql_type: r.get(1)? }))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if cols.is_empty() {
        return Err(format!("Table '{table}' not found"));
    }
    Ok(cols)
}

pub fn row_count(conn: &Connection, table: &str) -> Result<u64, String> {
    let n: i64 = conn
        .query_row(&format!("SELECT count(*) FROM {}", quote_ident(table)), [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(n as u64)
}

pub fn table_meta(conn: &Connection, table: &str) -> Result<TableMeta, String> {
    let columns = columns(conn, table)?;
    let row_count = row_count(conn, table)?;
    Ok(TableMeta { table: table.to_string(), row_count, columns })
}
