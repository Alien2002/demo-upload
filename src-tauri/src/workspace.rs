// src-tauri/src/workspace.rs
//
// Workspaces: top-level containers, one folder on disk each.
//
//   ~/workspaces/
//   └── q3-audit/                  <- workspace (id = "q3-audit")
//       ├── .manifest.json         <- display name, timestamps, folder list
//       └── databases/
//           ├── payroll.duckdb     <- folder (see folder.rs)
//           └── vendors.duckdb
//
// The filesystem is the source of truth. The manifest only adds what the
// filesystem can't store (display names, created dates) and is re-synced
// with the databases/ folder on every list/get. If it is missing or
// unreadable it is rebuilt.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::names::{is_valid_id, now_ms, slugify};

pub const MANIFEST_FILE: &str = ".manifest.json";
pub const DATABASES_DIR: &str = "databases";
pub const DB_EXT: &str = "duckdb";

#[derive(Serialize, Clone, Debug)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: u64,  // ms since epoch
    pub modified_at: u64, // ms since epoch
    pub path: String,
    pub folder_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub modified_at: u64,
    #[serde(default)]
    pub folders: Vec<FolderEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FolderEntry {
    pub id: String,
    pub name: String,
    pub created_at: u64,
}

/// Knows where workspaces live. Cheap to clone.
#[derive(Clone)]
pub struct WorkspaceStore {
    root: PathBuf,
}

impl WorkspaceStore {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|e| format!("Cannot create workspaces folder {}: {e}", root.display()))?;
        Ok(Self { root })
    }

    pub fn create_workspace(&self, name: &str) -> Result<Workspace, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Workspace name cannot be empty".into());
        }
        let id = slugify(name);
        if id.is_empty() {
            return Err(format!("'{name}' has no usable characters; use letters or numbers"));
        }
        let dir = self.root.join(&id);
        if dir.exists() {
            return Err(format!("A workspace named '{name}' already exists"));
        }
        fs::create_dir_all(dir.join(DATABASES_DIR))
            .map_err(|e| format!("Cannot create workspace folder: {e}"))?;

        let now = now_ms();
        let manifest = Manifest {
            version: 1,
            id: id.clone(),
            name: name.to_string(),
            created_at: now,
            modified_at: now,
            folders: vec![],
        };
        write_manifest(&dir, &manifest)?;
        Ok(to_workspace(&dir, &manifest))
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, String> {
        let entries = fs::read_dir(&self.root)
            .map_err(|e| format!("Cannot read {}: {e}", self.root.display()))?;
        let mut list = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path.file_name().and_then(|n| n.to_str()) else { continue };
            // Only folders that look like workspaces (skip stray files, hidden dirs).
            if !path.is_dir() || !is_valid_id(id) {
                continue;
            }
            if let Ok(manifest) = self.load_synced(&path) {
                list.push(to_workspace(&path, &manifest));
            }
        }
        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(list)
    }

    pub fn get_workspace(&self, id: &str) -> Result<Workspace, String> {
        let dir = self.dir(id)?;
        let manifest = self.load_synced(&dir)?;
        Ok(to_workspace(&dir, &manifest))
    }

    /// Deletes the workspace folder and everything in it.
    /// The caller must close its database connections first (Windows keeps files locked).
    pub fn delete_workspace(&self, id: &str) -> Result<(), String> {
        let dir = self.dir(id)?;
        fs::remove_dir_all(&dir).map_err(|e| format!("Cannot delete workspace '{id}': {e}"))
    }

    /// Validated path of an existing workspace.
    pub fn dir(&self, id: &str) -> Result<PathBuf, String> {
        if !is_valid_id(id) {
            return Err(format!("Invalid workspace id '{id}'"));
        }
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(format!("Workspace '{id}' not found"));
        }
        Ok(dir)
    }

    /// Load the manifest and reconcile it with databases/*.duckdb.
    /// Writes it back only if something changed.
    pub fn load_synced(&self, dir: &Path) -> Result<Manifest, String> {
        let db_dir = dir.join(DATABASES_DIR);
        fs::create_dir_all(&db_dir).map_err(|e| format!("Cannot create {}: {e}", db_dir.display()))?;

        let (mut manifest, mut changed) = match read_manifest(dir) {
            Some(m) => (m, false),
            None => (rebuild_manifest(dir), true),
        };

        let on_disk = scan_databases(&db_dir);

        // Drop entries whose .duckdb file is gone.
        let before = manifest.folders.len();
        manifest.folders.retain(|f| on_disk.iter().any(|(id, _)| id == &f.id));
        changed |= manifest.folders.len() != before;

        // Add files that appeared without going through the app.
        for (id, created) in &on_disk {
            if !manifest.folders.iter().any(|f| &f.id == id) {
                manifest.folders.push(FolderEntry { id: id.clone(), name: id.clone(), created_at: *created });
                changed = true;
            }
        }

        if changed {
            manifest.folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            manifest.modified_at = now_ms();
            write_manifest(dir, &manifest)?;
        }
        Ok(manifest)
    }

    /// Apply a change to a workspace's manifest and save it.
    pub fn update_manifest(&self, id: &str, f: impl FnOnce(&mut Manifest)) -> Result<Manifest, String> {
        let dir = self.dir(id)?;
        let mut manifest = self.load_synced(&dir)?;
        f(&mut manifest);
        manifest.folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        manifest.modified_at = now_ms();
        write_manifest(&dir, &manifest)?;
        Ok(manifest)
    }
}

fn to_workspace(dir: &Path, m: &Manifest) -> Workspace {
    Workspace {
        id: m.id.clone(),
        name: m.name.clone(),
        created_at: m.created_at,
        modified_at: m.modified_at,
        path: dir.display().to_string(),
        folder_count: m.folders.len(),
    }
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    let mut m: Manifest = serde_json::from_str(&text).ok()?;
    // The folder name on disk is the id, whatever the file says.
    m.id = dir.file_name()?.to_str()?.to_string();
    Some(m)
}

fn rebuild_manifest(dir: &Path) -> Manifest {
    let id = dir.file_name().and_then(|n| n.to_str()).unwrap_or("workspace").to_string();
    let created = file_created_ms(dir).unwrap_or_else(now_ms);
    Manifest { version: 1, name: id.clone(), id, created_at: created, modified_at: now_ms(), folders: vec![] }
}

/// Atomic write: temp file, then rename, so a crash never leaves half a manifest.
fn write_manifest(dir: &Path, m: &Manifest) -> Result<(), String> {
    let json = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    let tmp = dir.join(".manifest.json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("Cannot write manifest: {e}"))?;
    fs::rename(&tmp, dir.join(MANIFEST_FILE)).map_err(|e| format!("Cannot save manifest: {e}"))
}

/// (id, created_ms) for every databases/*.duckdb file.
fn scan_databases(db_dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = fs::read_dir(db_dir) else { return vec![] };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some(DB_EXT) {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            if !is_valid_id(&id) {
                return None;
            }
            Some((id, file_created_ms(&path).unwrap_or_else(now_ms)))
        })
        .collect()
}

pub fn file_created_ms(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let t = meta.created().or_else(|_| meta.modified()).ok()?;
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}
