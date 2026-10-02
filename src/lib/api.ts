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

// ---- staged import (review columns before the table is created)

export type ColumnKind = "text" | "integer" | "number" | "currency" | "date" | "datetime" | "boolean";

export type StagedColumn = {
  source: string;
  suggested_name: string;
  suggested_kind: ColumnKind;
  suggested_format: string | null;
  samples: string[];
  blank_count: number;
  /** Excel error cells (#N/A, #DIV/0!) staged as blank */
  excel_error_count: number;
};

export type StagedImport = {
  staging_id: string;
  file_path: string;
  sheet: string | null;
  row_count: number;
  suggested_table: string;
  columns: StagedColumn[];
};

export type ColumnSpec = {
  source: string;
  name: string;
  kind: ColumnKind;
  format: string | null;
  include: boolean;
};

export type ColumnCheck = {
  invalid_count: number;
  examples: string[];
  /** [original, converted] for the first few values */
  preview: [string, string | null][];
  /** Currency: values with more than 2 decimals that will be rounded */
  rounded_count: number;
  /** Number types: values like "00123" that will lose their leading zeros */
  leading_zero_count: number;
};
export type ImportComplete = { workspace_id: string; folder_id: string; table: string };

export const MAX_PAGE = 5000;

// ---- ACL commands

export type ScalarValue = { var: string; label: string; value: string | null };

export type CommandOutcome =
  | { kind: "table"; table: string; meta: TableMeta; permanent: boolean; label: string }
  | { kind: "scalars"; items: ScalarValue[] };

export type VarInfo = { name: string; value: string; kind: string };

export type CommandResponse = {
  command: string;
  input: string;
  outcome: CommandOutcome;
  sql: string;
  duration_ms: number;
  log_id: number;
  variables: VarInfo[];
};

/** Error from run_command: the character range to underline. */
export type CommandErrorInfo = { message: string; start: number; end: number };

export type ResultInfo = {
  /** "_results.r_7": open it in the grid or run the next command on it */
  table: string;
  label: string;
  command: string;
  source: string;
  row_count: number;
  created_at: string;
};

export type LogEntry = {
  id: number;
  at: string;
  command: string;
  input: string;
  ok: boolean;
  output: string;
  row_count: number | null;
  duration_ms: number;
  sql: string;
};

export const isResultTable = (table: string) => table.startsWith("_results.");

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

  analytics: {
    /** Run one ACL command. Rejects with CommandErrorInfo. */
    run: (workspace_id: string, folder_id: string, table: string, text: string, current_row: number | null) =>
      invoke<CommandResponse>("run_command", { workspace_id, folder_id, table, text, current_row }),

    results: (workspace_id: string, folder_id: string) =>
      invoke<ResultInfo[]>("list_results", { workspace_id, folder_id }),

    saveResult: (workspace_id: string, folder_id: string, result: string, name: string) =>
      invoke<TableMeta>("save_result", { workspace_id, folder_id, result, name }),

    deleteResult: (workspace_id: string, folder_id: string, result: string) =>
      invoke<void>("delete_result", { workspace_id, folder_id, result }),

    history: (workspace_id: string, folder_id: string, limit = 200) =>
      invoke<LogEntry[]>("command_history", { workspace_id, folder_id, limit }),

    variables: (workspace_id: string, folder_id: string) =>
      invoke<VarInfo[]>("get_variables", { workspace_id, folder_id }),

    onTablesChanged: (cb: (p: ImportComplete) => void): Promise<UnlistenFn> =>
      listen<ImportComplete>("tables-changed", (e) => cb(e.payload)),
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

    /** Staged import, step 1: read the file into a hidden staging table. */
    stage: (workspace_id: string, folder_id: string, file_path: string, sheet?: string) =>
      invoke<StagedImport>("stage_import", { workspace_id, folder_id, file_path, sheet: sheet ?? null }),

    /** Staged import, step 2: how many values won't convert to this type. */
    checkColumn: (
      workspace_id: string,
      folder_id: string,
      staging_id: string,
      source: string,
      kind: ColumnKind,
      format: string | null,
    ) => invoke<ColumnCheck>("check_import_column", { workspace_id, folder_id, staging_id, source, kind, format }),

    /** Staged import, step 3: create the table. */
    commit: (workspace_id: string, folder_id: string, staging_id: string, table_name: string, columns: ColumnSpec[]) =>
      invoke<TableMeta>("commit_import", { workspace_id, folder_id, staging_id, table_name, columns }),

    /** Throw a staged import away. */
    discard: (workspace_id: string, folder_id: string, staging_id: string) =>
      invoke<void>("discard_import", { workspace_id, folder_id, staging_id }),

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

export const KIND_LABELS: Record<ColumnKind, string> = {
  text: "Text",
  integer: "Whole number",
  number: "Number",
  currency: "Currency (2 dp)",
  date: "Date",
  datetime: "Date & time",
  boolean: "True / False",
};

/** null = ISO (2024-01-31), detected automatically */
export const DATE_FORMATS: { value: string | null; label: string }[] = [
  { value: null, label: "ISO  2024-01-31" },
  { value: "%d/%m/%Y", label: "31/01/2024" },
  { value: "%m/%d/%Y", label: "01/31/2024" },
  { value: "%d-%m-%Y", label: "31-01-2024" },
  { value: "%d.%m.%Y", label: "31.01.2024" },
  { value: "%Y/%m/%d", label: "2024/01/31" },
  { value: "%d-%b-%Y", label: "31-Jan-2024" },
  { value: "%d %B %Y", label: "31 January 2024" },
];

export const DATETIME_FORMATS: { value: string | null; label: string }[] = [
  { value: null, label: "ISO  2024-01-31 14:05:00" },
  { value: "%d/%m/%Y %H:%M", label: "31/01/2024 14:05" },
  { value: "%d/%m/%Y %H:%M:%S", label: "31/01/2024 14:05:00" },
  { value: "%m/%d/%Y %H:%M", label: "01/31/2024 14:05" },
  { value: "%d-%m-%Y %H:%M", label: "31-01-2024 14:05" },
];

export const EXCEL_EXTENSIONS =["xlsx", "xlsm", "xlsb", "xls", "ods"];
export const CSV_EXTENSIONS = ["csv", "tsv", "txt"];
export const isExcelPath = (path: string) =>
  EXCEL_EXTENSIONS.includes(path.split(".").pop()?.toLowerCase() ?? "");

/** Error from invoke(): a string from Rust, or { message, … } from run_command. */
export const errorText = (err: unknown): string => {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) return String((err as { message: unknown }).message);
  return JSON.stringify(err);
};

export const asCommandError = (err: unknown): CommandErrorInfo => {
  if (err && typeof err === "object" && "message" in err && "start" in err && "end" in err) {
    return err as CommandErrorInfo;
  }
  return { message: errorText(err), start: 0, end: 0 };
};
