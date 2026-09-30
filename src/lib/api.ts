// src/lib/api.ts
// Typed wrappers for every Tauri command. The only file that calls invoke().
// Argument names are snake_case to match the Rust commands (rename_all = "snake_case").

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---------------------------------------------------------------- types

export type Workspace = {
  id: string;
  name: string;
  created_at: number; // ms since epoch
  modified_at: number;
  path: string;
  folder_count: number;
};

export type Folder = {
  id: string;
  name: string;
  workspace_id: string;
  path: string;
  created_at: number;
  table_count: number;
};

export type ColumnInfo = { name: string; sql_type: string };

export type TableMeta = {
  table: string;
  row_count: number;
  columns: ColumnInfo[];
};

export type Row = (string | null)[];

export type SortDirection = "asc" | "desc";
export type SortField = { field: string; direction: SortDirection };

export type FilterOperator = "==" | "!=" | ">" | "<" | ">=" | "<=" | "contains";
/** value null (or "") with == / != means IS NULL / IS NOT NULL */
export type FilterCondition = { condition: FilterOperator; value: string | null };

export type QueryModel = {
  offset: number;
  limit: number;
  sort: SortField[];
  filters: Record<string, FilterCondition>;
  search?: string;
};

export type QueryResult = { rows: Row[]; total_count: number };

export type ImportProgress = { done: number; total: number };
export type ImportComplete = { workspace_id: string; folder_id: string; table: string };

export const MAX_PAGE = 5000;

// ---------------------------------------------------------------- api

export const api = {
  workspaces: {
    list: () => invoke<Workspace[]>("list_workspaces"),
    create: (name: string) => invoke<Workspace>("create_workspace", { name }),
    delete: (id: string) => invoke<void>("delete_workspace", { id }),
    get: (id: string) => invoke<Workspace>("get_workspace", { id }),
  },

  folders: {
    list: (workspace_id: string) => invoke<Folder[]>("list_folders", { workspace_id }),
    create: (workspace_id: string, name: string) =>
      invoke<Folder>("create_folder", { workspace_id, name }),
    delete: (workspace_id: string, folder_id: string) =>
      invoke<void>("delete_folder", { workspace_id, folder_id }),
    get: (workspace_id: string, folder_id: string) =>
      invoke<Folder>("get_folder", { workspace_id, folder_id }),
  },

  tables: {
    list: (workspace_id: string, folder_id: string) =>
      invoke<string[]>("list_tables", { workspace_id, folder_id }),

    meta: (workspace_id: string, folder_id: string, table: string) =>
      invoke<TableMeta>("table_meta", { workspace_id, folder_id, table }),

    sheets: (file_path: string) => invoke<string[]>("list_sheets", { file_path }),

    import: (workspace_id: string, folder_id: string, file_path: string, sheet?: string) =>
      invoke<TableMeta>("import_file_to_folder", {
        workspace_id,
        folder_id,
        file_path,
        sheet: sheet ?? null,
      }),

    /** Resolves true if a running import was asked to stop. */
    cancelImport: () => invoke<boolean>("cancel_import"),

    onImportProgress: (cb: (p: ImportProgress) => void): Promise<UnlistenFn> =>
      listen<ImportProgress>("import-progress", (e) => cb(e.payload)),

    onImportComplete: (cb: (p: ImportComplete) => void): Promise<UnlistenFn> =>
      listen<ImportComplete>("import-complete", (e) => cb(e.payload)),

    query: (workspace_id: string, folder_id: string, table: string, query_model: QueryModel) =>
      invoke<QueryResult>("execute_query", { workspace_id, folder_id, table, query_model }),
  },
};

// ---------------------------------------------------------------- helpers

const NUMERIC = /^(TINYINT|SMALLINT|INTEGER|BIGINT|HUGEINT|UTINYINT|USMALLINT|UINTEGER|UBIGINT|DOUBLE|FLOAT|REAL|DECIMAL)/i;
export const isNumeric = (sqlType: string) => NUMERIC.test(sqlType);

export const EXCEL_EXTENSIONS = ["xlsx", "xlsm", "xlsb", "xls", "ods"];
export const CSV_EXTENSIONS = ["csv", "tsv", "txt"];
export const isExcelPath = (path: string) =>
  EXCEL_EXTENSIONS.includes(path.split(".").pop()?.toLowerCase() ?? "");

/** Error from invoke() is usually a plain string from Rust. */
export const errorText = (err: unknown) =>
  typeof err === "string" ? err : err instanceof Error ? err.message : JSON.stringify(err);
