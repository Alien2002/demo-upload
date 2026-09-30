// src-tauri/src/commands.rs
//
// Thin Tauri wrappers around the import and query engines.
// All commands are async so they never block the UI thread.

use tauri::{AppHandle, Emitter, State};

use crate::import::{self, ImportProgress};
use crate::query::{self, TableMeta};
use crate::state::Db;

#[tauri::command]
pub async fn import_file(
    app: AppHandle,
    db: State<'_, Db>,
    path: String,
    sheet: Option<String>,
) -> Result<TableMeta, String> {
    // Own connection, so the grid can keep paging during a long import.
    let conn = db.clone_conn()?;
    tauri::async_runtime::spawn_blocking(move || {
        import::import_file(&conn, &path, sheet.as_deref(), |p: ImportProgress| {
            let _ = app.emit("import-progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_tables(db: State<'_, Db>) -> Result<Vec<String>, String> {
    let conn = db.clone_conn()?;
    query::list_tables(&conn)
}

#[tauri::command]
pub async fn table_meta(db: State<'_, Db>, table: String) -> Result<TableMeta, String> {
    let conn = db.clone_conn()?;
    query::table_meta(&conn, &table)
}

#[tauri::command]
pub async fn fetch_rows(
    db: State<'_, Db>,
    table: String,
    offset: u64,
    limit: u64,
) -> Result<Vec<Vec<Option<String>>>, String> {
    let conn = db.clone_conn()?;
    tauri::async_runtime::spawn_blocking(move || query::fetch_rows(&conn, &table, offset, limit))
        .await
        .map_err(|e| e.to_string())?
}
