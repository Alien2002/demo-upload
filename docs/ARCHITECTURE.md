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

### Import: `import.rs` + `staging.rs` (staged import)

```
stage_import ──▶ _staging.s_<id> (every column TEXT) ──▶ profile (suggestions)
                                                            │
Columns step ──▶ check_import_column (all rows, per change) ◀┘
     │
     ├── commit_import ──▶ CREATE TABLE main.<name> AS SELECT <conversions> … ORDER BY rowid; drop staging
     └── discard_import ──▶ drop staging
```

**Staging (`import.rs`).** The file is read once into a table in the `_staging` schema of the folder's database — invisible to `list_tables`, which only lists `main`. Everything is stored as text: Excel cells via calamine (dates as ISO 8601, Excel error cells as blank and counted per column), CSV via `read_csv(all_varchar = true, header = true)`. Progress every 25,000 rows; cancel via flag (Excel) or connection interrupt (CSV). Failure or cancel leaves nothing behind.

**Suggestions (`staging::profile`).** Exact blank counts over all rows; type tests on a 50,000-row reservoir sample (fast). A type is suggested when ≥ 99 % of sampled non-blank values convert, so one bad row doesn't force a whole column to Text — the leftovers appear as "won't convert" instead.

| Rule (in order) | Suggestion |
|---|---|
| all blank, or any digit-only value with a leading zero (`00123`) | Text |
| integer-looking (`12`, `1,200`, `12.00`) | Whole number |
| numeric (`1,250.50`, `-3.7`, `1e5`) | Number |
| `true/false/yes/no/1/0`-style | True / False |
| ISO date without time | Date |
| ISO date-time | Date & time |
| looks like a date: try `%d/%m/%Y`, `%m/%d/%Y`, `%d-%m-%Y`, `%d.%m.%Y`, `%Y/%m/%d`, `%d-%b-%Y`, `%d %b %Y`, `%d %B %Y` (± `%H:%M[:%S]`) | Date / Date & time with that format (day-first wins ties) |
| otherwise | Text |

**Conversions (`staging::conversion_expr`)** — all in SQL, all "try" functions, so a bad value becomes NULL instead of failing the import:

| Kind | DuckDB type | Expression |
|---|---|---|
| Text | VARCHAR | as is |
| Whole number | BIGINT | only if `[+-]?[0-9]+(.0+)?` → `TRY_CAST` (DuckDB would otherwise round `12.5` to 13) |
| Number | DOUBLE | `TRY_CAST(clean_number)` |
| Currency | DECIMAL(18,2) | `TRY_CAST(clean_number)` (rounds to 2 dp — reported) |
| Date | DATE | `TRY_CAST` (ISO) or `TRY_STRPTIME(value, format)::DATE` |
| Date & time | TIMESTAMP | `TRY_CAST` (ISO) or `TRY_STRPTIME(value, format)` |
| True / False | BOOLEAN | `TRY_CAST` |

`clean_number` strips commas only when they are real thousands separators (`1,234,567.89`).

**Checks (`staging::check_column`)** over all rows: values that won't convert (count + 5 distinct examples), a converted preview, values rounded by Currency, and leading-zero codes that a number type would damage. The Columns step runs a check for every non-text column on open (so counts are exact, not sampled) and again on every type/format change (debounced, stale replies dropped).

**Commit (`staging::commit`).** Validates the table name (not empty, not taken), column names (not empty, unique ignoring case), that at least one column is included, and that sources exist; then one `CREATE TABLE … AS SELECT … ORDER BY rowid` (keeps file order and contiguous rowids for fast paging) and drops the staging table.

**Housekeeping.** `ImportControl` tracks staging ids still open in a wizard; `stage_import` drops any other staging tables in that folder (left by a crash). Ids are validated (`s<digits>_<digits>`) before use in SQL.

**One-step import.** `import_file_to_folder` still exists: stage → accept all suggestions → commit.

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
| `stage_import` | `workspace_id, folder_id, file_path, sheet?` | `StagedImport` |
| `check_import_column` | `workspace_id, folder_id, staging_id, source, kind, format?` | `ColumnCheck` |
| `commit_import` | `workspace_id, folder_id, staging_id, table_name, columns: ColumnSpec[]` | `TableMeta` |
| `discard_import` | `workspace_id, folder_id, staging_id` | — |
| `import_file_to_folder` | `workspace_id, folder_id, file_path, sheet?` | `TableMeta` (one step, suggestions accepted) |
| `cancel_import` | — | `bool` (was one running) |

Events: `import-progress` `{done, total}` and `import-complete` `{workspace_id, folder_id, table}`.

`ImportControl` allows one import at a time and holds the cancel flag plus the DuckDB interrupt handle.

### `lib.rs`

Creates `~/workspaces`, manages `WorkspaceStore`, `ConnectionManager`, `CountCache`, `ImportControl`, registers commands, and calls `close_all()` on `RunEvent::Exit`.

---

## ACL command engine (`src-tauri/src/analytics/`)

```
text ─▶ ① lexer ─▶ ② parser ─▶ Command ─▶ ③ binder ─▶ ④ planner ─▶ LogicalPlan ─▶ ⑤ sqlgen ─▶ SQL ─▶ ⑥ executor
                     │                       │
              shared clauses          fields, types,
         (IF/WHILE/FIRST/NEXT/TO)   functions, variables
```

| Stage | File | Job |
|---|---|---|
| ① Lexer | `dsl/lexer.rs`, `dsl/token.rs` | Text → tokens with character spans. Keywords stay `Ident` so fields can be called `Next`; `[Field Name]`, `"text"`, `` `20240131` `` |
| ② Parser | `dsl/parser.rs` + `commands/<cmd>.rs` | Precedence-climbing expression parser; shared clause parsers (scope clauses in any order, each once); each command parses only its own keywords |
| ③ Binder | `binder.rs`, `functions.rs` | Resolves fields (case-insensitive, "did you mean"), variables, function calls; type-checks; compiles each expression to a quoted SQL fragment with a type |
| ④ Planner | `commands/<cmd>.rs` `plan()` | Builds a tree from **shared** nodes; `base_plan()` gives every command Scan → Scope → Filter |
| ⑤ SQL gen | `sqlgen.rs` | The only place SQL is assembled from plans |
| ⑥ Executor | `executor.rs`, `results.rs` | Runs it; scalars → values + variables; tables → `_results.r_n` or `main.<TO>`; logs every run |

### Shared plan nodes (`plan.rs`)

| Node | SQL shape |
|---|---|
| `Scan` | `SELECT rowid AS __acl_rn, * FROM table` — the record number travels with every row |
| `Scope` | `WHERE __acl_rn >= start` (NEXT) · WHILE: `bool_and(coalesce(cond,false)) OVER (ORDER BY __acl_rn ROWS UNBOUNDED PRECEDING)` · `ORDER BY __acl_rn LIMIT n` |
| `Filter` | `WHERE cond` |
| `Project` | `SELECT __acl_rn, expr AS name, …` |
| `Sort` | `row_number() OVER (ORDER BY keys…, __acl_rn) - 1 AS __acl_rn` (stable, renumbered) |
| `Aggregate` | no keys: one row (scalars) · keys: `GROUP BY`, renumbered in key order |

Tables are written with `CREATE TABLE … AS SELECT * EXCLUDE (__acl_rn) … ORDER BY __acl_rn`, so results keep record order and contiguous rowids: the grid's fast paging and the next command's FIRST/NEXT/WHILE work on them unchanged.

| Command | Plan |
|---|---|
| COUNT | `Aggregate(count) ← base` |
| TOTAL | `Aggregate(sum…) ← base` |
| EXTRACT | `[Project] ← base` |
| SORT | `Sort ← base` |
| SUMMARIZE | `Aggregate(group keys; arg_min OTHER; sum SUBTOTAL; count) ← base` |

### Results, variables, log

- `_results.r_<n>`: session results (no `TO`). Cleared the first time a folder is used after app start; newest 30 kept. `TableRef` (`query.rs`) maps `"_results.r_7"` ↔ schema + name so `table_meta`, `execute_query` and commands accept results like any table. Query-bar views of results are not persisted (ids are reused across sessions).
- `_meta.results`: label, command, source, row count — the sidebar's *Results* list and the lineage shown in the header.
- `_meta.command_log`: every run with input, output, row count, duration, SQL; failures too.
- Variables (`COUNT1`, `TOTAL1`…): in memory per folder (`AnalyticsState`), substituted as literals by the binder.

### Tauri commands

| Command | Args | Returns |
|---|---|---|
| `run_command` | `workspace_id, folder_id, table, text, current_row?` | `CommandResponse` (`outcome: table \| scalars`, `sql`, `duration_ms`, `variables`) or `{ message, start, end }` |
| `list_results` | `workspace_id, folder_id` | `ResultInfo[]` |
| `save_result` | `workspace_id, folder_id, result, name` | `TableMeta` (emits `tables-changed`) |
| `delete_result` | `workspace_id, folder_id, result` | — |
| `command_history` | `workspace_id, folder_id, limit?` | `LogEntry[]` |
| `get_variables` | `workspace_id, folder_id` | `VarInfo[]` |

### Frontend

`CommandBar` (Enter runs, ↑/↓ history, Esc clears, error text with the failing part highlighted), `OutputPanel` (values, result links, timings, SQL), `Navigation/ResultsList` (open, save as table, delete), `useCommandStore` (run, results, variables, recall). The selected grid row is sent as `current_row` for NEXT when the view isn't filtered/sorted/searched (then grid row = record number).

### Adding a command

1. `commands/<name>.rs`: AST struct, `parse(p: &mut Parser)` (loop: `p.parse_scope_clause` first, then own keywords, else `p.unexpected(...)`), `plan(cmd, ctx)` starting from `base_plan(ctx, &cmd.scope)`.
2. Add a variant to `Command`, an arm in `parse_command`, `name()` and `plan()` — the compiler lists anything missed.
3. Need a new kind of operation? Add a `LogicalPlan` node and its SQL in `sqlgen.rs` once; every later command can reuse it.
4. Test: parse → AST, plan → SQL, run on a fixture and compare with ACL's output.

Next good candidates: STATISTICS (Aggregate), DUPLICATES (Window + Filter), CLASSIFY/STRATIFY (Aggregate with buckets), JOIN (new Join node).

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
| `ImportWizard/*` | 4 steps. Step 3 uses the native dialog (a web file input can't give Rust a path), sheet dropdown, progress, Cancel that really stops reading. Step 4 (`Step4_Columns` + `ColumnEditorRow`) edits names/types/formats/inclusion with live checks; Back or closing discards the staged data |
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

**ACL commands:** see *ACL command engine → Adding a command* and [ACL_COMMANDS.md](ACL_COMMANDS.md).

---

## Roadmap

- Rename folders and workspaces
- Edit column names/types after import (`ALTER TABLE … RENAME COLUMN` / `ALTER COLUMN … TYPE … USING TRY_CAST`, reusing `check_column`)
- Decimal-comma numbers (`1.234,50`)
- Save column layouts as reusable import templates (same file every month)
- Multiple conditions per column / OR groups
- Materialised sorted views for fast deep scrolling
- Delete / rename / duplicate tables
- Export (DuckDB `COPY … TO` CSV/XLSX/Parquet)
- Streaming Excel import for very large sheets
- Column formatting (thousands separators, date styles)
- More ACL commands: STATISTICS, DUPLICATES, GAPS, CLASSIFY, STRATIFY, JOIN, SAMPLE, AGE
- Command scripts (several commands, saved and re-run)
