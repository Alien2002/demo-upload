// src/components/ImportButton.tsx
//
// Opens a file picker, runs the Rust import, and shows progress.

import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type TableMeta } from "../lib/api";

type Props = { onImported: (meta: TableMeta) => void };

export function ImportButton({ onImported }: Props) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");

  async function handleImport() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Data files", extensions: ["xlsx", "xlsm", "xlsb", "xls", "ods", "csv", "tsv"] },
      ],
    });
    if (!path) return;

    setBusy(true);
    setStatus("Reading file…");
    const unlisten = await api.onImportProgress((p) =>
      setStatus(`Importing ${p.done.toLocaleString()} / ${p.total.toLocaleString()} rows`),
    );

    try {
      const meta = await api.importFile(path);
      setStatus(`Imported ${meta.row_count.toLocaleString()} rows into "${meta.table}"`);
      onImported(meta);
    } catch (err) {
      setStatus(`Import failed: ${String(err)}`);
    } finally {
      unlisten();
      setBusy(false);
    }
  }

  return (
    <div className="import-bar">
      <button onClick={handleImport} disabled={busy}>
        {busy ? "Importing…" : "Import file"}
      </button>
      <span className="status">{status}</span>
    </div>
  );
}
