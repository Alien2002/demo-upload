// src/components/ImportWizard/Step3_ImportFile.tsx
//
// Pick a file (native dialog: the backend needs a real path, which an HTML
// <input type="file"> can't provide in Tauri), choose a sheet for Excel
// files, then import with live progress and a working Cancel.
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  api,
  CSV_EXTENSIONS,
  errorText,
  EXCEL_EXTENSIONS,
  isExcelPath,
  type ImportProgress,
  type TableMeta,
} from "../../lib/api";

type Props = {
  workspaceId: string;
  folderId: string;
  onBack: () => void;
  onClose: () => void;
  onRunningChange: (running: boolean) => void;
  onImported: (meta: TableMeta) => void;
};

export function Step3_ImportFile({ workspaceId, folderId, onBack, onClose, onRunningChange, onImported }: Props) {
  const [path, setPath] = useState<string | null>(null);
  const [sheets, setSheets] = useState<string[]>([]);
  const [sheet, setSheet] = useState<string>("");
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string | null>(null);
  const unlisten = useRef<UnlistenFn | null>(null);

  useEffect(() => () => unlisten.current?.(), []);

  function setBusy(b: boolean) {
    setRunning(b);
    onRunningChange(b);
  }

  async function chooseFile() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Spreadsheets & CSV", extensions: [...EXCEL_EXTENSIONS, ...CSV_EXTENSIONS] },
        { name: "Excel / ODS", extensions: EXCEL_EXTENSIONS },
        { name: "CSV / TSV", extensions: CSV_EXTENSIONS },
      ],
    });
    if (!picked) return;
    setPath(picked);
    setError(null);
    setProgress(null);
    setStatus("");
    setSheets([]);
    setSheet("");
    if (isExcelPath(picked)) {
      try {
        const names = await api.tables.sheets(picked);
        setSheets(names);
        setSheet(names[0] ?? "");
      } catch (err) {
        setError(errorText(err));
      }
    }
  }

  async function start() {
    if (!path) return;
    setBusy(true);
    setCancelling(false);
    setError(null);
    setStatus("Reading file…");
    setProgress(null);
    unlisten.current = await api.tables.onImportProgress((p) => {
      setProgress(p);
      if (p.total > 0) setStatus(`Importing ${p.done.toLocaleString()} / ${p.total.toLocaleString()} rows…`);
      else setStatus("Importing…");
    });

    try {
      const meta = await api.tables.import(workspaceId, folderId, path, sheet || undefined);
      setStatus(`Import complete! ${meta.row_count.toLocaleString()} rows in "${meta.table}".`);
      setBusy(false);
      onImported(meta);
    } catch (err) {
      const msg = errorText(err);
      setError(msg === "Import cancelled" ? null : msg);
      setStatus(msg === "Import cancelled" ? "Import cancelled." : "");
      setBusy(false);
    } finally {
      unlisten.current?.();
      unlisten.current = null;
    }
  }

  async function cancel() {
    if (running) {
      setCancelling(true);
      setStatus("Cancelling…");
      await api.tables.cancelImport();
    } else {
      onClose();
    }
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : null;
  const fileName = path?.split(/[\\/]/).pop();

  return (
    <div className="wizard-step">
      <div className="file-row">
        <button onClick={chooseFile} disabled={running}>
          {path ? "Choose another file…" : "Choose file…"}
        </button>
        <span className="file-name" title={path ?? ""}>
          {fileName ?? <span className="muted">No file selected</span>}
        </span>
      </div>

      {sheets.length > 0 && (
        <label className="field">
          <span>Sheet</span>
          <select value={sheet} onChange={(e) => setSheet(e.target.value)} disabled={running}>
            {sheets.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
      )}

      {(running || progress) && (
        <div className="progress" aria-label="Import progress">
          <div
            className={`progress-bar ${pct === null ? "indeterminate" : ""}`}
            style={pct === null ? undefined : { width: `${pct}%` }}
          />
        </div>
      )}
      {status && <p className="status-text">{status}</p>}
      {error && <p className="error-text">{error}</p>}

      <div className="wizard-actions">
        <button onClick={onBack} disabled={running}>
          Back
        </button>
        <button onClick={cancel} disabled={cancelling}>
          Cancel
        </button>
        <button className="primary" onClick={start} disabled={!path || running}>
          {running ? "Importing…" : "Start import"}
        </button>
      </div>
    </div>
  );
}
