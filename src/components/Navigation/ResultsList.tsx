// src/components/Navigation/ResultsList.tsx
//
// Session results of the selected folder (newest first). Click to open;
// right-click to save as a permanent table or delete.
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { errorText, type ResultInfo } from "../../lib/api";
import { useCommandStore } from "../../stores/useCommandStore";
import { useTableStore } from "../../stores/useTableStore";
import { ContextMenu } from "../common/ContextMenu";
import { NamePrompt } from "../common/NamePrompt";

export function ResultsList() {
  const { results, saveResult, deleteResult } = useCommandStore();
  const current = useTableStore((s) => s.current_table);
  const openTable = useTableStore((s) => s.openTable);
  const [menu, setMenu] = useState<{ x: number; y: number; r: ResultInfo } | null>(null);
  const [saving, setSaving] = useState<ResultInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (results.length === 0) return null;

  async function remove(r: ResultInfo) {
    const ok = await ask(`Delete result "${r.label}"?`, { title: "Delete result", kind: "warning" });
    if (!ok) return;
    try {
      await deleteResult(r.table);
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <div className="results-list">
      <div className="section-label sub">Results (this session)</div>
      {error && <p className="error-text small">{error}</p>}
      <ul className="table-list">
        {results.map((r) => (
          <li key={r.table}>
            <button
              className={r.table === current ? "active" : ""}
              onClick={() => void openTable(r.table)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, r });
              }}
              title={`${r.command}\non ${r.source} · ${r.row_count.toLocaleString()} rows`}
            >
              <span className="table-icon">↳</span> {r.label}
              <span className="badge">{r.row_count.toLocaleString()}</span>
            </button>
          </li>
        ))}
      </ul>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { label: "Open", onClick: () => void openTable(menu.r.table) },
            { label: "Save as table…", onClick: () => setSaving(menu.r) },
            { label: "Delete…", danger: true, onClick: () => void remove(menu.r) },
          ]}
        />
      )}
      {saving && (
        <NamePrompt
          title="Save result as table"
          label="Table name"
          placeholder="e.g. High value transactions"
          confirmText="Save"
          onSubmit={(name) => saveResult(saving.table, name)}
          onClose={() => setSaving(null)}
        />
      )}
    </div>
  );
}
