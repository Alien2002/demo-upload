// src-tauri/src/query.rs
//
// Query engine: paging and table metadata for the grid.
// The grid asks for small row windows; DuckDB answers in milliseconds
// no matter how big the table is.

use duckdb::Connection;
use serde::Serialize;

use crate::import::quote_ident;

const MAX_PAGE: u64 = 5_000;

#[derive(Serialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub sql_type: String,
}

#[derive(Serialize, Clone)]
pub struct TableMeta {
    pub table: String,
    pub row_count: u64,
    pub columns: Vec<ColumnInfo>,
}

pub fn list_tables(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'main' ORDER BY table_name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn table_meta(conn: &Connection, table: &str) -> Result<TableMeta, String> {
    let mut stmt = conn
        .prepare(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = 'main' AND table_name = ?
             ORDER BY ordinal_position",
        )
        .map_err(|e| e.to_string())?;
    let columns: Vec<ColumnInfo> = stmt
        .query_map([table], |r| {
            Ok(ColumnInfo { name: r.get(0)?, sql_type: r.get(1)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    // This also acts as validation: unknown tables have no columns.
    if columns.is_empty() {
        return Err(format!("Table '{table}' not found"));
    }

    let row_count: i64 = conn
        .query_row(&format!("SELECT count(*) FROM {}", quote_ident(table)), [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    Ok(TableMeta { table: table.to_string(), row_count: row_count as u64, columns })
}

/// Rows [offset, offset + limit) as display strings (NULL → None).
pub fn fetch_rows(
    conn: &Connection,
    table: &str,
    offset: u64,
    limit: u64,
) -> Result<Vec<Vec<Option<String>>>, String> {
    let meta = table_meta(conn, table)?; // validates the table name
    let limit = limit.min(MAX_PAGE);

    let select_list: Vec<String> = meta
        .columns
        .iter()
        .map(|c| format!("CAST({} AS VARCHAR)", quote_ident(&c.name)))
        .collect();

    // rowid is contiguous for imported tables, so a range filter on it is
    // much cheaper than OFFSET (which scans and discards rows).
    let sql = format!(
        "SELECT {} FROM {} WHERE rowid >= ? AND rowid < ? ORDER BY rowid",
        select_list.join(", "),
        quote_ident(table)
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let ncols = meta.columns.len();
    let rows = stmt
        .query_map([offset as i64, (offset + limit) as i64], |r| {
            (0..ncols).map(|i| r.get::<_, Option<String>>(i)).collect()
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}
