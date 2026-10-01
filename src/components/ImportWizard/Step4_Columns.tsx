// src/components/ImportWizard/Step4_Columns.tsx
//
// Review and edit columns before the table is created.
// - Suggested names/types come from the staging profile (valid by construction).
// - Changing a type or date format re-checks that column against EVERY row
//   (debounced), showing how many values won't convert and examples.
// - Import creates the table in one SQL statement; values that don't convert
//   become blank, after an explicit confirmation.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";

import { api, errorText, type ColumnSpec, type StagedImport, type TableMeta } from "../../lib/api";
import { ColumnEditorRow, type ColumnDraft } from "./ColumnEditorRow";

type Props = {
  workspaceId: string;
  folderId: string;
  staged: StagedImport;
  onBack: () => void;
  onRunningChange: (running: boolean) => void;
  onImported: (meta: TableMeta) => void;
};

const CHECK_DELAY_MS = 300;

export function Step4_Columns({ workspaceId, folderId, staged, onBack, onRunningChange, onImported }: Props) {
  const [drafts, setDrafts] = useState<ColumnDraft[]>(() =>
    staged.columns.map((c) => ({
      source: c.source,
      name: c.suggested_name,
      kind: c.suggested_kind,
      format: c.suggested_format,
      include: true,
      samples: c.samples,
      blank_count: c.blank_count,
      excel_error_count: c.excel_error_count,
      check: null,
      checking: false,
      check_error: null,
      custom_format: false,
    })),
  );
  const [tableName, setTableName] = useState(staged.suggested_table);
  const [existingTables, setExistingTables] = useState<string[]>([]);
  const [committing, setCommitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const versions = useRef(new Map<string, number>());

  useEffect(() => {
    api.tables.list(workspaceId, folderId).then(setExistingTables).catch(() => setExistingTables([]));
    const t = timers.current;
    return () => t.forEach(clearTimeout);
  }, [workspaceId, folderId]);

  // ---- live checks
  const runCheck = useCallback(
    (d: ColumnDraft) => {
      const version = (versions.current.get(d.source) ?? 0) + 1;
      versions.current.set(d.source, version);
      clearTimeout(timers.current.get(d.source));

      if (d.kind === "text") {
        // Text never fails.
        patchDraft(d.source, {
          checking: false,
          check: { invalid_count: 0, examples: [], preview: [], rounded_count: 0, leading_zero_count: 0 },
          check_error: null,
        });
        return;
      }
      patchDraft(d.source, { checking: true, check_error: null });
      timers.current.set(
        d.source,
        setTimeout(async () => {
          try {
            const check = await api.tables.checkColumn(
              workspaceId,
              folderId,
              staged.staging_id,
              d.source,
              d.kind,
              d.format?.trim() ? d.format.trim() : null,
            );
            if (versions.current.get(d.source) !== version) return; // a newer check is on its way
            patchDraft(d.source, { check, checking: false });
          } catch (err) {
            if (versions.current.get(d.source) !== version) return;
            patchDraft(d.source, { checking: false, check: null, check_error: errorText(err) });
          }
        }, CHECK_DELAY_MS),
      );
    },
    [workspaceId, folderId, staged.staging_id],
  );

  // Suggestions come from a sample: check every non-text column against ALL rows once.
  useEffect(() => {
    drafts.filter((d) => d.kind !== "text").forEach(runCheck);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function patchDraft(source: string, patch: Partial<ColumnDraft>) {
    setDrafts((ds) => ds.map((d) => (d.source === source ? { ...d, ...patch } : d)));
  }

  function onRowChange(source: string, patch: Partial<ColumnDraft>) {
    const current = drafts.find((d) => d.source === source);
    if (!current) return;
    patchDraft(source, patch);
    if ("kind" in patch || "format" in patch) runCheck({ ...current, ...patch });
  }

  // ---- validation
  const nameErrors = useMemo(() => {
    const errors = new Map<string, string>();
    const seen = new Map<string, string>();
    for (const d of drafts) {
      if (!d.include) continue;
      const n = d.name.trim();
      if (!n) errors.set(d.source, "Name required");
      else if (seen.has(n.toLowerCase())) errors.set(d.source, `Same as "${seen.get(n.toLowerCase())}"`);
      else seen.set(n.toLowerCase(), d.source);
    }
    return errors;
  }, [drafts]);

  const included = drafts.filter((d) => d.include);
  const tableError = !tableName.trim()
    ? "Table name required"
    : existingTables.some((t) => t.toLowerCase() === tableName.trim().toLowerCase())
      ? "A table with this name already exists in the folder"
      : null;
  const pendingChecks = included.some((d) => d.checking);
  const badChecks = included.filter((d) => d.check_error);
  const invalidCols = included.filter((d) => (d.check?.invalid_count ?? 0) > 0);
  const invalidTotal = invalidCols.reduce((n, d) => n + (d.check?.invalid_count ?? 0), 0);

  const canImport =
    !committing && !pendingChecks && !tableError && included.length > 0 && nameErrors.size === 0 && badChecks.length === 0;

  // ---- commit
  async function importNow() {
    if (!canImport) return;
    if (invalidTotal > 0) {
      const list = invalidCols.map((d) => `• ${d.name}: ${d.check!.invalid_count.toLocaleString()}`).join("\n");
      const ok = await ask(
        `${invalidTotal.toLocaleString()} value(s) can't be converted to the chosen types and will be imported as blank:\n\n${list}\n\nImport anyway?`,
        { title: "Some values won't convert", kind: "warning" },
      );
      if (!ok) return;
    }

    const columns: ColumnSpec[] = drafts.map((d) => ({
      source: d.source,
      name: d.name.trim(),
      kind: d.kind,
      format: d.format?.trim() ? d.format.trim() : null,
      include: d.include,
    }));

    setCommitting(true);
    onRunningChange(true);
    setError(null);
    try {
      const meta = await api.tables.commit(workspaceId, folderId, staged.staging_id, tableName.trim(), columns);
      onRunningChange(false);
      onImported(meta);
    } catch (err) {
      setError(errorText(err));
      setCommitting(false);
      onRunningChange(false);
    }
  }

  const fileName = staged.file_path.split(/[\\/]/).pop();

  return (
    <div className="wizard-step columns-step">
      <div className="columns-summary">
        <label className="field inline">
          <span>Table name</span>
          <input
            value={tableName}
            className={tableError ? "invalid" : ""}
            onChange={(e) => setTableName(e.target.value)}
          />
        </label>
        <span className="muted small">
          {fileName}
          {staged.sheet ? ` · sheet "${staged.sheet}"` : ""} · {staged.row_count.toLocaleString()} rows ·{" "}
          {included.length} of {drafts.length} columns
        </span>
      </div>
      {tableError && <p className="error-text small">{tableError}</p>}

      <div className="columns-table-wrap">
        <table className="columns-table">
          <thead>
            <tr>
              <th>
                <input
                  type="checkbox"
                  checked={included.length === drafts.length}
                  onChange={(e) => setDrafts((ds) => ds.map((d) => ({ ...d, include: e.target.checked })))}
                  aria-label="Include all columns"
                />
              </th>
              <th>From file</th>
              <th>Name</th>
              <th>Type</th>
              <th>Format</th>
              <th>Sample values</th>
              <th>Check</th>
            </tr>
          </thead>
          <tbody>
            {drafts.map((d) => (
              <ColumnEditorRow
                key={d.source}
                draft={d}
                nameError={nameErrors.get(d.source) ?? null}
                onChange={(patch) => onRowChange(d.source, patch)}
              />
            ))}
          </tbody>
        </table>
      </div>

      {invalidTotal > 0 && !pendingChecks && (
        <p className="warn small">
          ⚠ {invalidTotal.toLocaleString()} value(s) in {invalidCols.length} column(s) won't convert and would import as
          blank. Change the type or format, or import anyway.
        </p>
      )}
      {error && <p className="error-text">{error}</p>}

      <div className="wizard-actions">
        <button onClick={onBack} disabled={committing}>
          Back
        </button>
        <button className="primary" onClick={importNow} disabled={!canImport}>
          {committing ? "Creating table…" : pendingChecks ? "Checking…" : `Import ${staged.row_count.toLocaleString()} rows`}
        </button>
      </div>
    </div>
  );
}
