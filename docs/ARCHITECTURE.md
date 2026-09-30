# Architecture

How the app is built, what each file does, and why. Setup and usage are in the [README](../README.md).

---

## 1. Core ideas

1. **Import once, query forever.** A source file is read once into a DuckDB table. Everything after that — browsing, filtering, sorting, future ACL commands — runs as SQL against tables.
2. **The frontend never holds the dataset.** The grid draws only visible cells and asks Rust for 500-row blocks. Cost depends on screen size, not table size.
3. **Filesystem is the source of truth.** Workspaces are folders, folders are `.duckdb` files. The manifest only adds display names and dates, and heals itself.
4. **One open database per file.** DuckDB locks its file, so every module gets connections from a single `ConnectionManager`.

---

## 2. Data model

```
~/workspaces/<workspace-id>/
├── .manifest.json
└── databases/<folder-id>.duckdb   ← tables live here
```

| Concept | On disk | Id | Display name |
|---|---|---|---|
| Workspace | folder | folder name (slug) | manifest `name` |
| Folder | `.duckdb` file | file stem (slug) | manifest `folders[].name` |
| Table | table inside the DuckDB file | table name | same |

Slugs (`names::slugify`): lowercase ASCII letters/digits/`_`, other runs become `-`, max 64 chars, Windows reserved names (`CON`, `NUL`, …) get a `_` suffix. Every id arriving from the frontend is checked with `is_valid_id` (it must already be a slug), which blocks path traversal like `../../etc`.

Manifest example:

```json
{
  "version": 1,
  "id": "q3-audit",
  "name": "Q3 Audit",
  "created_at": 1790770838349,
  "modified_at": 1790770900000,
  "folders": [
    { "id": "general-ledger", "name": "General Ledger", "created_at": 1790770838360 }
  ]
}
```

Timestamps are milliseconds since epoch (`new Date(ms)` in JS).

---

## 3. Backend (`src-tauri/src/`)

### `connections.rs` — ConnectionManager

- `HashMap<path, Entry{ base: Connection, last_used }>` behind `Arc<Mutex<…>>`; the manager is `Clone`.
- `get_connection(path)` (file must exist) / `get_or_create(path)`: opens lazily, then returns `base.try_clone()` — a cheap handle sharing the same database instance, safe to move to another thread.
- A reaper thread closes databases idle for **300 s**; it stops when the manager is dropped.
- `close_connection(path)`, `close_under(dir)` (a whole workspace), `close_all()` (on app exit, with `CHECKPOINT`).
- Error mapping: *not found*, *permission denied*, *open in another program* (lock), *corrupt / not a DuckDB file*.

**Implications:** handles already given out keep working after a close until dropped; deletes happen only after closing (Windows won't delete open files). Paths are normalised (canonical parent + file name) so the same file always maps to one entry.

### `workspace.rs` — WorkspaceStore

`create_workspace`, `list_workspaces`, `get_workspace`, `delete_workspace`, plus `load_synced` and `update_manifest`.

`load_synced` reconciles the manifest with `databases/*.duckdb`: drops entries whose file is gone, adds files that appeared, rebuilds the manifest if missing or unreadable, writes only when something changed. Writes are atomic (temp file + rename).

### `folder.rs`

`create_folder` (slug, uniqueness check, creates the DuckDB file via the manager + `CHECKPOINT`, adds to manifest), `list_folders` / `get_folder` (with `table_count` from the database; a corrupt file shows 0 instead of breaking the list), `delete_folder` (close connection, delete `.duckdb` and `.wal`, update manifest).

### `import.rs` — import engine

| Step | Excel/ODS | CSV/TSV/TXT |
|---|---|---|
| Detect | extension → `Excel` | extension → `Csv` |
| Read | `calamine` loads the sheet once | DuckDB `read_csv_auto` |
| Types | pass 1 widens a type per column (table below) | DuckDB sniffer |
| Load | pass 2, DuckDB `Appender` | `CREATE TABLE AS SELECT` |
| Progress | every 25,000 rows | start + end |
| Cancel | flag checked every 1,000 rows | connection interrupt |

Type inference:

| Cells seen in the column | Type |
|---|---|
| booleans only | BOOLEAN |
| whole numbers only | BIGINT |
| any fraction | DOUBLE |
| real datetimes only | TIMESTAMP |
| anything mixed / text / all empty | VARCHAR |

Guarantees:
- **New table every time**: name from the file, made unique (`ledger`, `ledger_2`).
- **No half tables**: on failure or cancel the table is dropped.
- **Clear errors**: file not found; unsupported extension (lists supported ones); corrupt / password-protected / mislabelled file; sheet not found (lists available sheets); row-level load failure with Excel row number and a preview of the row's values.

`list_sheets(path)` returns sheet names for the wizard.

### `query_engine.rs`

```jsonc
// QueryModel
{
  "offset": 0,
  "limit": 500,                                   // capped at 5000
  "sort": [{ "field": "Amount", "direction": "desc" }],
  "filters": { "Amount": { "condition": ">=", "value": "1000" },
               "Memo":   { "condition": "contains", "value": "refund" },
               "Notes":  { "condition": "==", "value": null } },   // IS NULL
  "search": "acme"
}
// QueryResult
{ "rows": [["1", "…", null]], "total_count": 972 }
```

| Situation | SQL shape |
|---|---|
| no filters, search or sort (fast path) | `WHERE rowid >= ? AND rowid < ? ORDER BY rowid` |
| anything else | `WHERE … ORDER BY … , rowid LIMIT n OFFSET m` |

- **Filters** (ANDed): text columns compare as text; other columns compare against `CAST(? AS <column type>)`, pre-checked so `Amount > abc` gives *"'abc' is not a valid DOUBLE value for column 'Amount'"*. `contains` is case-insensitive `ILIKE` with `%`/`_` escaped. `==`/`!=` with an empty value mean `IS NULL`/`IS NOT NULL`.
- **Search**: `ILIKE` across all VARCHAR columns (all columns if none are text).
- **Sort**: multi-column, `NULLS LAST`, plus `rowid` as a tie-breaker so page boundaries never shuffle.
- **Safety**: table and column names are checked against the catalog and quoted; every value is a bound parameter.
- **Count cache** (`CountCache`): total counts per (database, table, WHERE, params), so scrolling a filtered view doesn't re-count. Invalidated for a database after an import into it or when its folder is deleted.

### `query.rs`

Catalog helpers: `list_tables`, `columns` (also validates the table name), `row_count`, `table_meta`.

### `names.rs`

`slugify`, `is_valid_id`, `table_name_from_path`, `unique_name`, `quote_ident`, `quote_literal`, `now_ms`.

### `commands.rs`

All commands are `async`, use `rename_all = "snake_case"` (JS passes `workspace_id`, not `workspaceId`), and run engine code in `spawn_blocking`.

| Command | Args | Returns |
|---|---|---|
| `list_workspaces` | — | `Workspace[]` |
| `create_workspace` | `name` | `Workspace` |
| `get_workspace` / `delete_workspace` | `id` | `Workspace` / — |
| `list_folders` | `workspace_id` | `Folder[]` |
| `create_folder` | `workspace_id, name` | `Folder` |
| `get_folder` / `delete_folder` | `workspace_id, folder_id` | `Folder` / — |
| `list_tables` | `workspace_id, folder_id` | `string[]` |
| `table_meta` | `workspace_id, folder_id, table` | `TableMeta` |
| `execute_query` | `workspace_id, folder_id, table, query_model` | `QueryResult` |
| `list_sheets` | `file_path` | `string[]` |
| `import_file_to_folder` | `workspace_id, folder_id, file_path, sheet?` | `TableMeta` |
| `cancel_import` | — | `bool` (was one running) |

Events: `import-progress` `{done, total}` and `import-complete` `{workspace_id, folder_id, table}`.

`ImportControl` allows one import at a time and holds the cancel flag plus the DuckDB interrupt handle.

### `lib.rs`

Creates `~/workspaces`, manages `WorkspaceStore`, `ConnectionManager`, `CountCache`, `ImportControl`, registers commands, and calls `close_all()` on `RunEvent::Exit`.

---

## 4. Frontend (`src/`)

### State (Zustand, one store per file)

| Store | Holds | Notes |
|---|---|---|
| `useWorkspaceStore` | `workspaces`, `current_workspace_id` | `loadWorkspaces`, `createWorkspace`, `deleteWorkspace` |
| `useFolderStore` | `folders`, `current_folder_id`, `workspace_id` | `workspace_id` records which workspace the selection belongs to |
| `useTableStore` | `tables`, `current_table`, `current_meta`, `scope_key` | `openTable`, `openTableIn(ws, folder, table)` |
| `useQueryStore` | `filters`, `search`, `sort`, `offset`, `limit` | saved **per table** in localStorage |

**Selection ownership:** after a workspace switch, the folder store may briefly hold a folder id from the old workspace. Each store records the scope its selection belongs to, and loaders drop selections from another scope — but keep ones set deliberately with `selectFolderIn` / `openTableIn` (used by the import wizard to jump to the new table).

**`useNavigationSync`** (mounted once in `Layout`) loads workspaces on start, reloads folders when the workspace changes, tables when the folder changes, binds the query store to the open table, and refreshes lists on `import-complete`.

### Components

| Component | Role |
|---|---|
| `Layout` | TopBar + Sidebar + main; sidebar collapses (auto on narrow windows) |
| `Navigation/TopBar` | toggle, breadcrumb, *Workspaces* manager, *+ New workspace*, *Import file*, settings (placeholder) |
| `Navigation/Sidebar` | workspace dropdown, folders (right-click: open / rename *(soon)* / delete), tables of the selected folder, *+ New folder* |
| `Navigation/Breadcrumb` | Workspace / Folder / Table, each clickable |
| `Navigation/WorkspaceManager` | modal: switch, create, delete workspaces |
| `ImportWizard/*` | 3 steps; step 3 uses the native dialog (a web file input can't give Rust a path), sheet dropdown, progress bar, Cancel that really stops the import |
| `QueryBar/QueryBar` | debounced search, filter and sort chips, *+ Filter*, *Clear all* |
| `QueryBar/FilterEditor` | column / operator / value, type-aware placeholder |
| `DataGridView` | Glide grid; header click sorts, Shift+click adds a level; footer "Showing X–Y of Z rows (filtered from N)" |
| `common/*` | `Modal`, `NamePrompt`, `ContextMenu` |

Confirmations use the dialog plugin's native `ask()` (browser `confirm()` is unreliable in webviews).

### Grid and cache

- `cacheKey = workspace/folder/table | filters+search+sort`. A new key clears the cache, bumps its generation (late responses are dropped), and remounts the grid at the top.
- Blocks of 500 rows, ±1 buffer block, max 120 blocks (LRU via `Map` insertion order).
- `total_count` from each response sets the grid's row count.
- Glide calls `getCellContent` synchronously for every visible cell; cache hits draw values, misses draw loading bars until `updateCells` repaints them.

---

## 5. Performance notes

| Path | Why it's fast / slow |
|---|---|
| Unfiltered paging | rowid range → constant time at any depth (~7 ms) |
| Filters on numbers/dates | DuckDB vectorised scan (~10 ms on 1M rows) |
| `contains` / search | `ILIKE` on every row of text columns (0.2–0.5 s) |
| Sorted, deep pages | each block re-sorts to reach its offset (~0.4 s at row 900k) |

Idea for deep sorted scrolling: materialise the sorted/filtered result into a temporary table once, then page it by rowid. That is also exactly how ACL commands (SORT, EXTRACT) will work: they create new tables.

---

## 6. Design decisions

| Decision | Alternative | Reason |
|---|---|---|
| Folder = one `.duckdb` file | One database, schema per folder | Files are easy to copy, back up, delete; each folder locks independently |
| Ids are slugs of names | UUIDs | Readable on disk; manifest can be rebuilt from the filesystem |
| Filesystem is authoritative | Manifest authoritative | Survives manual copies/deletes and crashes |
| New table on every import | Replace existing | Audit trail: never silently lose data |
| Native file dialog | `<input type=file>` | Rust needs a real path; webviews hide it |
| Query saved per table | One global query | One table's filters would break on another's columns |
| Count cache | Count every request | Scrolling a filtered view would repeat the scan per block |
| Glide (canvas) | ag-Grid (DOM) | Constant render cost at any row count |

---

## 7. Extending

**New command:** engine function taking `&Connection` → wrapper in `commands.rs` (`#[tauri::command(rename_all = "snake_case")]`, `spawn_blocking`) → register in `lib.rs` → typed wrapper in `src/lib/api.ts`.

**ACL commands (e-Analytics):** translate to SQL that creates a table in the same folder, e.g. `FILTER Amount > 1000` → `CREATE TABLE big_filtered AS SELECT * FROM big WHERE "Amount" > 1000`. Invalidate the folder's `CountCache` scope and emit `import-complete` (or a new `table-created` event) so the sidebar refreshes.

---

## Roadmap

- Rename folders and workspaces
- Multiple conditions per column / OR groups
- Materialised sorted views for fast deep scrolling
- Delete / rename / duplicate tables
- Export (DuckDB `COPY … TO` CSV/XLSX/Parquet)
- Count and report Excel error cells per column
- Streaming Excel import for very large sheets
- Column formatting (thousands separators, date styles)
- e-Analytics command console
