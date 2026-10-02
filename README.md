# demo-upload — desktop audit analytics (Tauri + React + DuckDB)

The foundation of an ACL-style audit analytics app. Organise data into **workspaces** and **folders**, import Excel/CSV files into them as tables, then browse, filter, search and sort million-row tables in a canvas grid. Everything runs locally; no server.

```
Workspace (a folder on disk)
└── Folder (one .duckdb database — like an ACL project)
    └── Tables (imported files, later: results of commands)
```

For how it works internally, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

---

## Features

- **Workspaces & folders** — create, switch, delete; stored under `~/workspaces/`
- **Import wizard** — workspace → folder → file → **columns**: sheet picker, live progress, working Cancel
- **Review columns before import** — rename, change type, set date format, or skip any column; every change is checked against all rows ("3 won't convert, e.g. "31/02/2024"") before the table is created
- **Formats** — `.xlsx .xlsm .xlsb .xls .ods .csv .tsv .txt`
- **Suggested column types** — Text, Whole number, Number, Currency (2 dp), Date, Date & time, True/False; detects `dd/mm/yyyy`-style dates and `1,250.00` numbers; codes like `00123` stay text; Excel error cells are counted
- **Never overwrites** — importing `ledger.xlsx` twice gives `ledger` and `ledger_2`
- **Query engine** — filters (`== != > < >= <= contains`, empty value = blank cells), full-text search, multi-column sort; filters/sort are remembered per table
- **Canvas grid** — only visible cells are drawn; click a header to sort, Shift+click to add a sort level
- **ACL commands** — COUNT, TOTAL, EXTRACT, SORT, SUMMARIZE with IF / WHILE / FIRST / NEXT, ACL expressions and functions, variables (COUNT1, TOTAL1…); results become session tables you can keep querying, or permanent tables with `TO`; every command logged with its SQL — see [docs/ACL_COMMANDS.md](docs/ACL_COMMANDS.md)

### Measured (1M rows × 9 columns, 48 MB `.xlsx`)

| Operation | Time |
|---|---|
| Excel read + column suggestions | ~9 s |
| Check one column against all rows | ~0.4 s |
| Create table from reviewed columns | ~0.9 s |
| Page of rows, no filters (any depth) | ~7 ms |
| Numeric / date / equality filter | 6–17 ms |
| `contains` filter on one column | ~180 ms |
| Search across all text columns | ~430 ms |
| Sorted, first page | ~10 ms |
| Sorted, page at row 900,000 | ~400 ms |
| `COUNT IF …` / `TOTAL …` (ACL) | 5–50 ms |
| `EXTRACT … IF` / `SUMMARIZE ON` 100k groups | 20–80 ms |
| `SORT ON` 1M rows into a result | ~0.9 s |

---

## Tech stack

| Layer | Technology |
|---|---|
| Shell | Tauri 2 |
| Storage & queries | DuckDB (`duckdb` crate, bundled) |
| Excel reading | `calamine` |
| File dialogs | `tauri-plugin-dialog` |
| UI | React 19, TypeScript, Vite |
| State | Zustand |
| Grid | `@glideapps/glide-data-grid` 6.0.4-alpha24 |

---

## Prerequisites

- **Ubuntu:** `sudo apt install build-essential` plus the standard Tauri packages (`libwebkit2gtk-4.1-dev`, etc.)
- **Windows:** Visual Studio Build Tools with **Desktop development with C++**; WebView2 (preinstalled on 10/11)
- **Both:** Rust stable, Node.js 20+

DuckDB needs no separate install — it is compiled into the app.

---

## Setup

### 1. Rust dependencies

```bash
npm run tauri add dialog
cd src-tauri
cargo add duckdb --features bundled
cargo add calamine@0.36
cd ..
```

`src-tauri/Cargo.toml` should contain:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
tauri-plugin-dialog = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
duckdb = { version = "1", features = ["bundled"] }
calamine = "0.36"

# Recommended: smaller debug builds, faster DuckDB in dev
[profile.dev.package."*"]
debug = false
opt-level = 1
```

`src-tauri/capabilities/default.json` must include `"dialog:default"` (the wizard uses the open dialog and confirm prompts).

### 2. Frontend dependencies

```bash
npm install zustand @glideapps/glide-data-grid@6.0.4-alpha24 lodash marked react-responsive-carousel
```

Pin that Glide version — stable 6.0.3 doesn't support React 19.

### 3. `index.html`

After `<div id="root"></div>` add:

```html
<div id="portal" style="position: fixed; left: 0; top: 0; z-index: 9999;"></div>
```

### 4. Copy the source

Replace `src-tauri/src/` and `src/` with the ones from this project (keep the template's `src/main.tsx` and `src-tauri/src/main.rs`; `main.rs` must call `<your_crate>_lib::run()`).

### 5. Run

```bash
npm run tauri dev
```

First build compiles DuckDB from C++ (5–30+ min, once).

---

## Usage

1. **+ New workspace** (top bar) → name it.
2. **+ New folder** (sidebar bottom) → name it.
3. **Import file** → confirm workspace and folder → choose file (and sheet) → **Next: review columns**.
4. Adjust names, types and date formats; untick columns you don't want; check the warnings → **Import**.
5. The table opens. Use the search box, **+ Filter**, and header clicks to explore — or type ACL commands in the **ACL>** bar (`COUNT IF Amount > 1000`, `EXTRACT IF …`, `SORT ON Amount D`). Table results open in the grid and appear under *Results*; the next command runs on them.
6. Right-click a folder to delete it. **Workspaces** (top bar) lists all workspaces, with delete.

---

## Where data is stored

```
~/workspaces/                         (C:\Users\<you>\workspaces on Windows)
└── q3-audit/
    ├── .manifest.json                display names, dates, folder list
    └── databases/
        ├── general-ledger.duckdb     one folder = one DuckDB file
        └── payroll.duckdb
```

- Folder and workspace **ids** are the on-disk names (`q3-audit`); **display names** ("Q3 Audit") live in the manifest.
- The manifest is re-synced with the `databases/` folder on every list, and rebuilt if missing (display names then fall back to the ids).
- A `.duckdb.wal` file next to a database is DuckDB's write-ahead log — don't delete it while the app runs.
- To inspect a folder: close the app, then `duckdb ~/workspaces/q3-audit/databases/general-ledger.duckdb`.
- Change the root location in `workspaces_root()` in `src-tauri/src/lib.rs`.

---

## Project structure

```
src-tauri/src/
├── lib.rs            wiring: state, commands, shutdown
├── commands.rs       Tauri commands (the JS-callable API) + import cancel control
├── connections.rs    ConnectionManager: one pooled DuckDB per .duckdb file
├── workspace.rs      workspaces + manifest
├── folder.rs         folders (.duckdb files)
├── import.rs         reads files into text-only staging tables (Excel via calamine, CSV via DuckDB)
├── staging.rs        column suggestions, per-column checks, commit/discard
├── query_engine.rs   filters / search / sort / paging + count cache
├── query.rs          table catalog helpers + TableRef (main tables and _results.r_n)
├── analytics/        ACL engine: dsl/ (lexer, parser), commands/ (one file per command),
│                     binder, functions, plan, sqlgen, executor, results
└── names.rs          slugs, unique names, SQL quoting, timestamps

src/
├── App.tsx / App.css
├── lib/api.ts                    typed wrappers for every command
├── stores/                       Zustand: workspace, folder, table, query, command
├── hooks/useRowCache.ts          block cache between grid and Rust
├── hooks/useNavigationSync.ts    keeps stores in step + import-complete listener
└── components/
    ├── Layout.tsx
    ├── DataGridView.tsx          Glide grid driven by the query store
    ├── TableList.tsx
    ├── ImportButton.tsx
    ├── Navigation/               TopBar, Sidebar, Breadcrumb, WorkspaceSelector, WorkspaceManager
    ├── ImportWizard/             ImportWizardModal, Step1–4, ColumnEditorRow
    ├── QueryBar/                 QueryBar, FilterEditor
    ├── CommandBar/               CommandBar (ACL>), OutputPanel
    └── common/                   Modal, NamePrompt, ContextMenu
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `unresolved import duckdb` / `calamine` | Crates not in `Cargo.toml` | Setup §1 |
| Build sits on `libduckdb-sys` | DuckDB compiling | Normal on first build |
| `Disk quota exceeded` / GTK `Bail out!` | Disk full | Free space; add the `[profile.dev.package."*"]` block |
| Delete/confirm prompts don't appear | Missing dialog permission | Add `"dialog:default"` to capabilities |
| "… is open in another program" | DuckDB CLI or a second app copy has the file | Close it |
| "Another import is already running" | One import at a time | Wait or Cancel |
| Filters from one table show on another | — | Can't happen: filters are saved per table |
| Grid popups missing | No portal div | Setup §3 |

---

## Known limitations

- One filter per column (e.g. no `Amount > 100 AND Amount < 500` yet).
- Sorting a million rows costs ~0.4 s per block when scrolled deep; plain and filtered scrolling stay fast.
- Rename (folders/workspaces) is not implemented yet.
- Excel imports hold the whole sheet in memory.
- Numbers with a decimal comma (`1.234,50`) aren't recognised yet — import them as Text.
- Deleting a workspace's manifest loses its display names (ids remain).

See the roadmap in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#roadmap).
