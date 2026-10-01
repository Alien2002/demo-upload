// src-tauri/src/commands.rs
//
// Tauri commands: the only functions JavaScript can call.
// Each one validates ids, gets a connection from the ConnectionManager, and
// runs the engine function on a blocking thread so the window never freezes.
//
// All commands use snake_case argument names on the JS side too
// (rename_all = "snake_case"), matching the TypeScript types in src/lib/api.ts.

use duckdb::InterruptHandle;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

use crate::connections::ConnectionManager;
use crate::folder::{self, Folder};
use crate::import::{self, ImportProgress};
use crate::query::{self, TableMeta};
use crate::query_engine::{self, CountCache, QueryModel, QueryResult};
use crate::staging::{self, ColumnCheck, ColumnKind, ColumnSpec, StagedImport};
use crate::workspace::{Workspace, WorkspaceStore};

/// Run blocking work off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------- workspaces

#[tauri::command(rename_all = "snake_case")]
pub async fn create_workspace(ws: State<'_, WorkspaceStore>, name: String) -> Result<Workspace, String> {
    let ws = ws.inner().clone();
    blocking(move || ws.create_workspace(&name)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_workspaces(ws: State<'_, WorkspaceStore>) -> Result<Vec<Workspace>, String> {
    let ws = ws.inner().clone();
    blocking(move || ws.list_workspaces()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_workspace(ws: State<'_, WorkspaceStore>, id: String) -> Result<Workspace, String> {
    let ws = ws.inner().clone();
    blocking(move || ws.get_workspace(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_workspace(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    id: String,
) -> Result<(), String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || {
        let dir = ws.dir(&id)?;
        conns.close_under(&dir); // release file locks first (Windows)
        ws.delete_workspace(&id)
    })
    .await
}

// ---------------------------------------------------------------- folders

#[tauri::command(rename_all = "snake_case")]
pub async fn create_folder(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
    name: String,
) -> Result<Folder, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || folder::create_folder(&ws, &conns, &workspace_id, &name)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_folders(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
) -> Result<Vec<Folder>, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || folder::list_folders(&ws, &conns, &workspace_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_folder(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
    folder_id: String,
) -> Result<Folder, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || folder::get_folder(&ws, &conns, &workspace_id, &folder_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_folder(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    counts: State<'_, CountCache>,
    workspace_id: String,
    folder_id: String,
) -> Result<(), String> {
    let (ws, conns, counts) = (ws.inner().clone(), conns.inner().clone(), counts.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        counts.invalidate(&path.display().to_string());
        folder::delete_folder(&ws, &conns, &workspace_id, &folder_id)
    })
    .await
}

// ---------------------------------------------------------------- tables

#[tauri::command(rename_all = "snake_case")]
pub async fn list_tables(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
    folder_id: String,
) -> Result<Vec<String>, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        query::list_tables(&conns.get_connection(&path)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn table_meta(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
    folder_id: String,
    table: String,
) -> Result<TableMeta, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        query::table_meta(&conns.get_connection(&path)?, &table)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn execute_query(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    counts: State<'_, CountCache>,
    workspace_id: String,
    folder_id: String,
    table: String,
    query_model: QueryModel,
) -> Result<QueryResult, String> {
    let (ws, conns, counts) = (ws.inner().clone(), conns.inner().clone(), counts.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        let conn = conns.get_connection(&path)?;
        let scope = path.display().to_string();
        query_engine::execute_query_cached(&conn, &table, &query_model, Some((&counts, &scope)))
    })
    .await
}

// ---------------------------------------------------------------- import

/// Tracks the running import so it can be cancelled (one at a time), and the
/// staging tables still waiting for the user to commit or discard.
#[derive(Clone, Default)]
pub struct ImportControl {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    interrupt: Arc<Mutex<Option<Arc<InterruptHandle>>>>,
    active_staging: Arc<Mutex<HashSet<String>>>,
}

impl ImportControl {
    fn track(&self, id: &str) {
        if let Ok(mut s) = self.active_staging.lock() {
            s.insert(id.to_string());
        }
    }

    fn untrack(&self, id: &str) {
        if let Ok(mut s) = self.active_staging.lock() {
            s.remove(id);
        }
    }

    fn active(&self) -> Vec<String> {
        self.active_staging.lock().map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    fn start(&self, handle: Arc<InterruptHandle>) -> Result<(), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Another import is already running".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        if let Ok(mut h) = self.interrupt.lock() {
            *h = Some(handle);
        }
        Ok(())
    }

    fn finish(&self) {
        if let Ok(mut h) = self.interrupt.lock() {
            *h = None;
        }
        self.running.store(false, Ordering::SeqCst);
    }

    fn request_cancel(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }
        self.cancel.store(true, Ordering::SeqCst);
        // Stops a long DuckDB statement (CSV import) immediately.
        if let Ok(h) = self.interrupt.lock() {
            if let Some(h) = h.as_ref() {
                h.interrupt();
            }
        }
        true
    }
}

#[derive(Serialize, Clone)]
struct ImportComplete {
    workspace_id: String,
    folder_id: String,
    table: String,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_sheets(file_path: String) -> Result<Vec<String>, String> {
    blocking(move || import::list_sheets(&file_path)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn import_file_to_folder(
    app: AppHandle,
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    counts: State<'_, CountCache>,
    control: State<'_, ImportControl>,
    workspace_id: String,
    folder_id: String,
    file_path: String,
    sheet: Option<String>,
) -> Result<TableMeta, String> {
    let (ws, conns, counts, control) =
        (ws.inner().clone(), conns.inner().clone(), counts.inner().clone(), control.inner().clone());

    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        let conn = conns.get_connection(&path)?;
        control.start(conn.interrupt_handle())?;

        let progress_app = app.clone();
        let result = import::import_file(&conn, &file_path, sheet.as_deref(), &control.cancel, |p: ImportProgress| {
            let _ = progress_app.emit("import-progress", p);
        });
        control.finish();

        let meta = result?;
        counts.invalidate(&path.display().to_string());
        let _ = app.emit(
            "import-complete",
            ImportComplete { workspace_id, folder_id, table: meta.table.clone() },
        );
        Ok(meta)
    })
    .await
}

// ---------------------------------------------------------------- staged import

/// Step 1: read the file into a staging table and suggest names/types.
/// Emits "import-progress" while reading; cancellable with cancel_import.
#[tauri::command(rename_all = "snake_case")]
pub async fn stage_import(
    app: AppHandle,
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    control: State<'_, ImportControl>,
    workspace_id: String,
    folder_id: String,
    file_path: String,
    sheet: Option<String>,
) -> Result<StagedImport, String> {
    let (ws, conns, control) = (ws.inner().clone(), conns.inner().clone(), control.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        let conn = conns.get_connection(&path)?;
        // Leftovers from a crash or a closed wizard.
        let _ = staging::cleanup(&conn, &control.active());

        control.start(conn.interrupt_handle())?;
        let result = import::stage_file(&conn, &file_path, sheet.as_deref(), &control.cancel, |p: ImportProgress| {
            let _ = app.emit("import-progress", p);
        });
        control.finish();

        let staged = result?;
        control.track(&staged.staging_id);
        Ok(staged)
    })
    .await
}

/// Step 2 (repeatable): how many values won't convert to the chosen type.
#[tauri::command(rename_all = "snake_case")]
pub async fn check_import_column(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    workspace_id: String,
    folder_id: String,
    staging_id: String,
    source: String,
    kind: ColumnKind,
    format: Option<String>,
) -> Result<ColumnCheck, String> {
    let (ws, conns) = (ws.inner().clone(), conns.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        let conn = conns.get_connection(&path)?;
        staging::check_column(&conn, &staging_id, &source, kind, format.as_deref())
    })
    .await
}

/// Step 3: create the real table from the user's column choices.
#[tauri::command(rename_all = "snake_case")]
pub async fn commit_import(
    app: AppHandle,
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    counts: State<'_, CountCache>,
    control: State<'_, ImportControl>,
    workspace_id: String,
    folder_id: String,
    staging_id: String,
    table_name: String,
    columns: Vec<ColumnSpec>,
) -> Result<TableMeta, String> {
    let (ws, conns, counts, control) =
        (ws.inner().clone(), conns.inner().clone(), counts.inner().clone(), control.inner().clone());
    blocking(move || {
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        let conn = conns.get_connection(&path)?;
        let meta = staging::commit(&conn, &staging_id, &table_name, &columns)?;
        control.untrack(&staging_id);
        counts.invalidate(&path.display().to_string());
        let _ = app.emit(
            "import-complete",
            ImportComplete { workspace_id, folder_id, table: meta.table.clone() },
        );
        Ok(meta)
    })
    .await
}

/// Throw a staged import away (wizard closed, Back, or Cancel).
#[tauri::command(rename_all = "snake_case")]
pub async fn discard_import(
    ws: State<'_, WorkspaceStore>,
    conns: State<'_, ConnectionManager>,
    control: State<'_, ImportControl>,
    workspace_id: String,
    folder_id: String,
    staging_id: String,
) -> Result<(), String> {
    let (ws, conns, control) = (ws.inner().clone(), conns.inner().clone(), control.inner().clone());
    blocking(move || {
        control.untrack(&staging_id);
        let path = folder::existing_db_path(&ws, &workspace_id, &folder_id)?;
        staging::discard(&conns.get_connection(&path)?, &staging_id)
    })
    .await
}

/// Returns true if an import was running and has been asked to stop.
#[tauri::command(rename_all = "snake_case")]
pub async fn cancel_import(control: State<'_, ImportControl>) -> Result<bool, String> {
    Ok(control.request_cancel())
}
