// src-tauri/src/folder.rs
//
// Folders: one .duckdb file each, inside a workspace's databases/ folder.
// A folder holds tables (imported files, and later the output of commands).
// This is the equivalent of an ACL project.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;

use crate::connections::ConnectionManager;
use crate::names::{is_valid_id, now_ms, slugify};
use crate::query;
use crate::workspace::{FolderEntry, WorkspaceStore, DATABASES_DIR, DB_EXT};

#[derive(Serialize, Clone, Debug)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub workspace_id: String,
    pub path: String,
    pub created_at: u64,
    pub table_count: usize,
}

/// Validated path of a folder's .duckdb file (it may or may not exist yet).
pub fn db_path(ws: &WorkspaceStore, workspace_id: &str, folder_id: &str) -> Result<PathBuf, String> {
    if !is_valid_id(folder_id) {
        return Err(format!("Invalid folder id '{folder_id}'"));
    }
    Ok(ws.dir(workspace_id)?.join(DATABASES_DIR).join(format!("{folder_id}.{DB_EXT}")))
}

/// Validated path of an EXISTING folder database.
pub fn existing_db_path(
    ws: &WorkspaceStore,
    workspace_id: &str,
    folder_id: &str,
) -> Result<PathBuf, String> {
    let path = db_path(ws, workspace_id, folder_id)?;
    if !path.is_file() {
        return Err(format!("Folder '{folder_id}' not found in workspace '{workspace_id}'"));
    }
    Ok(path)
}

pub fn create_folder(
    ws: &WorkspaceStore,
    conns: &ConnectionManager,
    workspace_id: &str,
    name: &str,
) -> Result<Folder, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Folder name cannot be empty".into());
    }
    let id = slugify(name);
    if id.is_empty() {
        return Err(format!("'{name}' has no usable characters; use letters or numbers"));
    }
    let path = db_path(ws, workspace_id, &id)?;
    if path.exists() {
        return Err(format!("A folder named '{name}' already exists in this workspace"));
    }

    // Opening creates the file; CHECKPOINT makes sure it is written to disk now.
    let conn = conns.get_or_create(&path)?;
    conn.execute_batch("CHECKPOINT").map_err(|e| format!("Cannot initialise database: {e}"))?;
    drop(conn);

    let created_at = now_ms();
    ws.update_manifest(workspace_id, |m| {
        m.folders.retain(|f| f.id != id);
        m.folders.push(FolderEntry { id: id.clone(), name: name.to_string(), created_at });
    })?;

    Ok(Folder {
        id,
        name: name.to_string(),
        workspace_id: workspace_id.to_string(),
        path: path.display().to_string(),
        created_at,
        table_count: 0,
    })
}

pub fn list_folders(
    ws: &WorkspaceStore,
    conns: &ConnectionManager,
    workspace_id: &str,
) -> Result<Vec<Folder>, String> {
    let dir = ws.dir(workspace_id)?;
    let manifest = ws.load_synced(&dir)?;
    manifest
        .folders
        .iter()
        .map(|entry| build_folder(ws, conns, workspace_id, entry))
        .collect()
}

pub fn get_folder(
    ws: &WorkspaceStore,
    conns: &ConnectionManager,
    workspace_id: &str,
    folder_id: &str,
) -> Result<Folder, String> {
    let dir = ws.dir(workspace_id)?;
    let manifest = ws.load_synced(&dir)?;
    let entry = manifest
        .folders
        .iter()
        .find(|f| f.id == folder_id)
        .ok_or_else(|| format!("Folder '{folder_id}' not found in workspace '{workspace_id}'"))?;
    build_folder(ws, conns, workspace_id, entry)
}

pub fn delete_folder(
    ws: &WorkspaceStore,
    conns: &ConnectionManager,
    workspace_id: &str,
    folder_id: &str,
) -> Result<(), String> {
    let path = existing_db_path(ws, workspace_id, folder_id)?;
    conns.close_connection(&path);

    fs::remove_file(&path).map_err(|e| format!("Cannot delete folder '{folder_id}': {e}"))?;
    let wal = path.with_extension(format!("{DB_EXT}.wal"));
    if wal.exists() {
        let _ = fs::remove_file(wal);
    }

    ws.update_manifest(workspace_id, |m| m.folders.retain(|f| f.id != folder_id))?;
    Ok(())
}

fn build_folder(
    ws: &WorkspaceStore,
    conns: &ConnectionManager,
    workspace_id: &str,
    entry: &FolderEntry,
) -> Result<Folder, String> {
    let path = existing_db_path(ws, workspace_id, &entry.id)?;
    // A corrupt or locked file shouldn't hide the whole folder list: report 0 tables.
    let table_count = conns
        .get_connection(&path)
        .and_then(|c| query::list_tables(&c))
        .map(|t| t.len())
        .unwrap_or(0);
    Ok(Folder {
        id: entry.id.clone(),
        name: entry.name.clone(),
        workspace_id: workspace_id.to_string(),
        path: path.display().to_string(),
        created_at: entry.created_at,
        table_count,
    })
}
