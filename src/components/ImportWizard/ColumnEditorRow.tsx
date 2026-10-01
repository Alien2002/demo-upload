// src/components/ImportWizard/ColumnEditorRow.tsx
// One row of the Columns step: include, name, type, format, samples, status.
import { DATE_FORMATS, DATETIME_FORMATS, KIND_LABELS, type ColumnCheck, type ColumnKind } from "../../lib/api";

export type ColumnDraft = {
  source: string;
  name: string;
  kind: ColumnKind;
  format: string | null;
  include: boolean;
  samples: string[];
  blank_count: number;
  excel_error_count: number;
  /** null = not checked yet */
  check: ColumnCheck | null;
  checking: boolean;
  check_error: string | null;
  /** true once the user picks "Custom…" for the format */
  custom_format: boolean;
};

type Props = {
  draft: ColumnDraft;
  nameError: string | null;
  onChange: (patch: Partial<ColumnDraft>) => void;
};

const KINDS = Object.keys(KIND_LABELS) as ColumnKind[];
const CUSTOM = "__custom__";

export function ColumnEditorRow({ draft, nameError, onChange }: Props) {
  const isDate = draft.kind === "date" || draft.kind === "datetime";
  const presets = draft.kind === "datetime" ? DATETIME_FORMATS : DATE_FORMATS;
  const presetValue = draft.custom_format
    ? CUSTOM
    : presets.some((p) => p.value === draft.format)
      ? (draft.format ?? "")
      : CUSTOM;

  return (
    <tr className={draft.include ? "" : "excluded"}>
      <td className="col-include">
        <input
          type="checkbox"
          checked={draft.include}
          onChange={(e) => onChange({ include: e.target.checked })}
          aria-label={`Import column ${draft.source}`}
        />
      </td>

      <td className="col-source" title={draft.source}>
        {draft.source}
      </td>

      <td className="col-name">
        <input
          value={draft.name}
          disabled={!draft.include}
          className={nameError ? "invalid" : ""}
          onChange={(e) => onChange({ name: e.target.value })}
          aria-label={`Name for ${draft.source}`}
        />
        {nameError && <div className="error-text small">{nameError}</div>}
      </td>

      <td className="col-type">
        <select
          value={draft.kind}
          disabled={!draft.include}
          onChange={(e) => onChange({ kind: e.target.value as ColumnKind, format: null, custom_format: false })}
          aria-label={`Type for ${draft.source}`}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {KIND_LABELS[k]}
            </option>
          ))}
        </select>
      </td>

      <td className="col-format">
        {isDate ? (
          <>
            <select
              value={presetValue}
              disabled={!draft.include}
              onChange={(e) => {
                const v = e.target.value;
                if (v === CUSTOM) onChange({ custom_format: true, format: draft.format ?? "" });
                else onChange({ custom_format: false, format: v === "" ? null : v });
              }}
              aria-label={`Date format for ${draft.source}`}
            >
              {presets.map((p) => (
                <option key={p.label} value={p.value ?? ""}>
                  {p.label}
                </option>
              ))}
              <option value={CUSTOM}>Custom…</option>
            </select>
            {presetValue === CUSTOM && (
              <input
                className="format-input"
                value={draft.format ?? ""}
                placeholder="%d/%m/%Y"
                disabled={!draft.include}
                onChange={(e) => onChange({ format: e.target.value })}
                title="strftime codes: %d day, %m month, %Y year, %b Jan, %H hour, %M minute, %S second"
              />
            )}
          </>
        ) : (
          <span className="muted">—</span>
        )}
      </td>

      <td className="col-samples">
        {draft.samples.slice(0, 3).map((s, i) => (
          <span key={i} className="sample" title={s}>
            {s}
          </span>
        ))}
        {draft.samples.length === 0 && <span className="muted small">all blank</span>}
      </td>

      <td className="col-status">
        <Status draft={draft} />
      </td>
    </tr>
  );
}

function Status({ draft }: { draft: ColumnDraft }) {
  if (!draft.include) return <span className="muted small">skipped</span>;
  if (draft.checking) return <span className="muted small">checking…</span>;
  if (draft.check_error) return <span className="error-text small">{draft.check_error}</span>;

  const invalid = draft.check?.invalid_count ?? 0;
  const preview = draft.check?.preview.find(([orig, conv]) => conv !== null && orig !== conv);

  return (
    <div className="status-cell">
      {invalid === 0 ? (
        <span className="ok small">✓ all values fit</span>
      ) : (
        <span className="warn small" title={`Won't convert: ${draft.check?.examples.join(", ")}`}>
          ⚠ {invalid.toLocaleString()} won't convert
          <span className="examples">e.g. {draft.check?.examples.slice(0, 3).map((x) => `"${x}"`).join(", ")}</span>
        </span>
      )}
      {preview && (
        <span className="muted small">
          {preview[0]} → {preview[1]}
        </span>
      )}
      {(draft.check?.rounded_count ?? 0) > 0 && (
        <span className="warn small">{draft.check!.rounded_count.toLocaleString()} rounded to 2 decimals</span>
      )}
      {(draft.check?.leading_zero_count ?? 0) > 0 && (
        <span className="warn small">
          {draft.check!.leading_zero_count.toLocaleString()} lose leading zeros (e.g. codes) — consider Text
        </span>
      )}
      {draft.excel_error_count > 0 && (
        <span className="warn small">{draft.excel_error_count.toLocaleString()} Excel error cell(s) → blank</span>
      )}
    </div>
  );
}
