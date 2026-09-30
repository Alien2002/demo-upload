// src-tauri/src/state.rs
//
// Holds the single DuckDB workspace database for the app.
// Every imported file becomes a table inside this one .duckdb file,
// the same way ACL keeps imported data as tables inside a project.

use duckdb::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("Failed to open database: {e}"))?;
        Ok(Db(Mutex::new(conn)))
    }

    /// A second connection to the same database.
    /// Long jobs (imports) use their own connection so the grid
    /// can keep paging while the import runs.
    pub fn clone_conn(&self) -> Result<Connection, String> {
        let guard = self.0.lock().map_err(|_| "Database lock poisoned".to_string())?;
        guard.try_clone().map_err(|e| e.to_string())
    }
}
