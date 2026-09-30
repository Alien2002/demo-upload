// src/components/QueryBar/FilterEditor.tsx
// Add or edit one filter: column, operator, value.
import { useState } from "react";
import { isNumeric, type ColumnInfo, type FilterOperator } from "../../lib/api";
import { useQueryStore } from "../../stores/useQueryStore";
import { Modal } from "../common/Modal";

type Props = {
  columns: ColumnInfo[];
  /** Existing filter's field, or null for a new one. */
  field: string | null;
  onClose: () => void;
};

const OPERATORS: { op: FilterOperator; label: string }[] = [
  { op: "==", label: "equals" },
  { op: "!=", label: "does not equal" },
  { op: ">", label: "greater than" },
  { op: ">=", label: "greater than or equal" },
  { op: "<", label: "less than" },
  { op: "<=", label: "less than or equal" },
  { op: "contains", label: "contains" },
];

function placeholderFor(sqlType: string) {
  if (isNumeric(sqlType)) return "e.g. 1000";
  if (/TIMESTAMP|DATE/i.test(sqlType)) return "e.g. 2024-06-30 or 2024-06-30 14:00:00";
  if (/BOOL/i.test(sqlType)) return "true or false";
  return "text";
}

export function FilterEditor({ columns, field, onClose }: Props) {
  const { filters, setFilter, clearFilter } = useQueryStore();
  const existing = field ? filters[field] : undefined;

  const [column, setColumn] = useState(field ?? columns[0]?.name ?? "");
  const [op, setOp] = useState<FilterOperator>(existing?.condition ?? "==");
  const [value, setValue] = useState(existing?.value ?? "");

  const col = columns.find((c) => c.name === column);
  const allowsEmpty = op === "==" || op === "!=";
  const canApply = column !== "" && (allowsEmpty || value.trim() !== "");

  function apply() {
    if (!canApply) return;
    if (field && field !== column) clearFilter(field); // column changed while editing
    setFilter(column, { condition: op, value: value.trim() === "" ? null : value.trim() });
    onClose();
  }

  return (
    <Modal
      title={field ? "Edit filter" : "Add filter"}
      onClose={onClose}
      width={420}
      footer={
        <>
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={apply} disabled={!canApply}>
            Apply
          </button>
        </>
      }
    >
      <label className="field">
        <span>Column</span>
        <select value={column} onChange={(e) => setColumn(e.target.value)}>
          {columns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name} ({c.sql_type.toLowerCase()})
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Condition</span>
        <select value={op} onChange={(e) => setOp(e.target.value as FilterOperator)}>
          {OPERATORS.map((o) => (
            <option key={o.op} value={o.op}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Value</span>
        <input
          autoFocus
          value={value}
          placeholder={col ? placeholderFor(col.sql_type) : ""}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && apply()}
        />
      </label>
      {allowsEmpty && <p className="muted small">Leave the value empty to match blank cells.</p>}
      {filters[column] && column !== field && (
        <p className="muted small">This replaces the existing filter on “{column}” (one filter per column).</p>
      )}
    </Modal>
  );
}
