// src/lib/api.ts
// Typed wrappers around the Rust commands.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ColumnInfo = { name: string; sql_type: string };

export type TableMeta = {
  table: string;
  row_count: number;
  columns: ColumnInfo[];
};

export type ImportProgress = { done: number; total: number };

export type Row = (string | null)[];

export const api = {
  importFile: (path: string, sheet?: string) =>
    invoke<TableMeta>("import_file", { path, sheet: sheet ?? null }),

  listTables: () => invoke<string[]>("list_tables"),

  tableMeta: (table: string) => invoke<TableMeta>("table_meta", { table }),

  fetchRows: (table: string, offset: number, limit: number) =>
    invoke<Row[]>("fetch_rows", { table, offset, limit }),

  onImportProgress: (cb: (p: ImportProgress) => void): Promise<UnlistenFn> =>
    listen<ImportProgress>("import-progress", (e) => cb(e.payload)),
};

const NUMERIC = /^(TINYINT|SMALLINT|INTEGER|BIGINT|HUGEINT|DOUBLE|FLOAT|REAL|DECIMAL)/i;
export const isNumeric = (sqlType: string) => NUMERIC.test(sqlType);
