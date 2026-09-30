// src/App.tsx
// Replaces the template's App.tsx.

import { useCallback, useEffect, useState } from "react";
import { api, type TableMeta } from "./lib/api";
import { ImportButton } from "./components/ImportButton";
import { TableList } from "./components/TableList";
import { DataGridView } from "./components/DataGridView";
import "./App.css";

export default function App() {
  const [tables, setTables] = useState<string[]>([]);
  const [meta, setMeta] = useState<TableMeta | null>(null);

  const refreshTables = useCallback(async () => {
    setTables(await api.listTables());
  }, []);

  useEffect(() => {
    void refreshTables();
  }, [refreshTables]);

  async function selectTable(table: string) {
    setMeta(await api.tableMeta(table));
  }

  async function handleImported(m: TableMeta) {
    await refreshTables();
    setMeta(m);
  }

  return (
    <div className="app">
      <header>
        <ImportButton onImported={handleImported} />
        {meta && (
          <span className="meta">
            {meta.table} · {meta.row_count.toLocaleString()} rows · {meta.columns.length} columns
          </span>
        )}
      </header>
      <aside>
        <TableList tables={tables} active={meta?.table ?? null} onSelect={selectTable} />
      </aside>
      <main>{meta ? <DataGridView meta={meta} /> : <p className="empty">Select or import a table.</p>}</main>
    </div>
  );
}
