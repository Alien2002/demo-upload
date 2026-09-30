// src-tauri/src/connections.rs
//
// ConnectionManager: one open DuckDB database per .duckdb file (folder).
//
// Why this exists: DuckDB locks its file. Opening the same file twice in one
// process fails, so every module must go through this manager instead of
// calling Connection::open itself.
//
// - Lazy: a database is opened the first time it is requested.
// - Pooled: it stays open until it has been idle for IDLE_TIMEOUT, or until
//   close_connection / close_all is called.
// - Cheap handles: get_connection returns a fresh Connection cloned from the
//   cached one (try_clone). Clones share the same database instance, can be
//   moved into background threads, and are simply dropped when done.

use duckdb::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

pub const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const REAPER_INTERVAL: Duration = Duration::from_secs(30);

struct Entry {
    base: Connection,
    last_used: Instant,
}

type Pool = Mutex<HashMap<PathBuf, Entry>>;

#[derive(Clone)]
pub struct ConnectionManager {
    pool: Arc<Pool>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::with_idle_timeout(IDLE_TIMEOUT)
    }

    pub fn with_idle_timeout(idle_timeout: Duration) -> Self {
        let pool: Arc<Pool> = Arc::new(Mutex::new(HashMap::new()));
        spawn_reaper(Arc::downgrade(&pool), idle_timeout);
        Self { pool }
    }

    /// Connection to an EXISTING database file.
    pub fn get_connection(&self, db_path: &Path) -> Result<Connection, String> {
        self.open(db_path, false)
    }

    /// Connection to a database file, creating the file if it does not exist.
    pub fn get_or_create(&self, db_path: &Path) -> Result<Connection, String> {
        self.open(db_path, true)
    }

    fn open(&self, db_path: &Path, create: bool) -> Result<Connection, String> {
        let key = normalize(db_path);
        let mut pool = self.lock()?;

        if let Some(entry) = pool.get_mut(&key) {
            entry.last_used = Instant::now();
            return entry.base.try_clone().map_err(|e| describe_error(&key, &e.to_string()));
        }

        check_file(&key, create)?;
        let base = Connection::open(&key).map_err(|e| describe_error(&key, &e.to_string()))?;
        let handle = base.try_clone().map_err(|e| describe_error(&key, &e.to_string()))?;
        pool.insert(key, Entry { base, last_used: Instant::now() });
        Ok(handle)
    }

    /// Close one database (e.g. before deleting its file).
    /// Handles already given out keep working until they are dropped.
    pub fn close_connection(&self, db_path: &Path) {
        if let Ok(mut pool) = self.lock() {
            pool.remove(&normalize(db_path));
        }
    }

    /// Close every database whose file lives under `dir` (e.g. a workspace).
    pub fn close_under(&self, dir: &Path) {
        let dir = normalize(dir);
        if let Ok(mut pool) = self.lock() {
            pool.retain(|path, _| !path.starts_with(&dir));
        }
    }

    /// Close everything. Called when the app exits.
    pub fn close_all(&self) {
        if let Ok(mut pool) = self.lock() {
            for (_, entry) in pool.drain() {
                let _ = entry.base.execute_batch("CHECKPOINT");
            }
        }
    }

    pub fn open_count(&self) -> usize {
        self.lock().map(|p| p.len()).unwrap_or(0)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<PathBuf, Entry>>, String> {
        self.pool.lock().map_err(|_| "Connection pool lock poisoned".to_string())
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Background thread that closes idle databases. Stops when the manager is dropped.
fn spawn_reaper(pool: Weak<Pool>, idle_timeout: Duration) {
    let interval = REAPER_INTERVAL.min(idle_timeout);
    thread::spawn(move || loop {
        thread::sleep(interval);
        let Some(pool) = pool.upgrade() else { break };
        if let Ok(mut map) = pool.lock() {
            map.retain(|_, e| e.last_used.elapsed() < idle_timeout);
        };
    });
}

/// Stable key for a path, whether or not the file exists yet:
/// canonical parent folder + file name.
fn normalize(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => std::fs::canonicalize(dir)
            .map(|d| d.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

fn check_file(path: &Path, create: bool) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                return Err(format!("Expected a database file but found a folder: {}", path.display()));
            }
            if meta.permissions().readonly() {
                return Err(format!("Permission denied: {} is read-only", path.display()));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if create {
                match path.parent() {
                    Some(dir) if dir.exists() => Ok(()),
                    _ => Err(format!("Parent folder does not exist: {}", path.display())),
                }
            } else {
                Err(format!("Database file not found: {}", path.display()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(format!("Permission denied: cannot access {}", path.display()))
        }
        Err(e) => Err(format!("Cannot access {}: {e}", path.display())),
    }
}

/// Turn raw DuckDB open errors into messages a user can act on.
fn describe_error(path: &Path, raw: &str) -> String {
    let lower = raw.to_lowercase();
    let file = path.display();
    if lower.contains("permission denied") {
        format!("Permission denied: cannot open {file}")
    } else if lower.contains("could not set lock") || lower.contains("conflicting lock") {
        format!("{file} is open in another program (close other copies of the app or the DuckDB CLI)")
    } else if lower.contains("not a valid duckdb database")
        || lower.contains("invalid database")
        || lower.contains("corrupt")
        || lower.contains("checksum")
    {
        format!("{file} is corrupt or is not a DuckDB database ({raw})")
    } else {
        format!("Failed to open {file}: {raw}")
    }
}
