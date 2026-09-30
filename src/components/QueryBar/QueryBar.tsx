// src/components/QueryBar/QueryBar.tsx
//
// Search box, active filter/sort chips, and "+ Filter".
// Everything writes to useQueryStore; the grid reacts to it.
import { useEffect, useState } from "react";
import type { ColumnInfo } from "../../lib/api";
import { useQueryStore } from "../../stores/useQueryStore";
import { FilterEditor } from "./FilterEditor";

type Props = { columns: ColumnInfo[] };

const SEARCH_DELAY_MS = 350;

export function QueryBar({ columns }: Props) {
  const { filters, sort, search, setSearch, clearFilter, setSort, clearFilters, table_key } = useQueryStore();
  const [text, setText] = useState(search);
  const [editing, setEditing] = useState<string | null | "new">(null);

  // Keep the box in step when switching tables.
  useEffect(() => setText(search), [table_key]); // eslint-disable-line react-hooks/exhaustive-deps

  // Debounced search: don't query on every keystroke.
  useEffect(() => {
    if (text === search) return;
    const t = setTimeout(() => setSearch(text), SEARCH_DELAY_MS);
    return () => clearTimeout(t);
  }, [text, search, setSearch]);

  const filterEntries = Object.entries(filters);
  const hasAnything = filterEntries.length > 0 || sort.length > 0 || search.trim() !== "";

  return (
    <div className="query-bar">
      <input
        className="search-input"
        type="search"
        placeholder="Search text columns…"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />

      <div className="chips">
        {filterEntries.map(([field, f]) => (
          <span key={field} className="chip">
            <button className="chip-label" onClick={() => setEditing(field)} title="Edit filter">
              {field} {f.condition} {f.value === null || f.value === "" ? "(empty)" : `"${f.value}"`}
            </button>
            <button className="chip-x" onClick={() => clearFilter(field)} aria-label={`Remove filter on ${field}`}>
              ✕
            </button>
          </span>
        ))}
        {sort.map((s, i) => (
          <span key={s.field} className="chip sort">
            <span className="chip-label">
              {sort.length > 1 ? `${i + 1}. ` : ""}
              {s.field} {s.direction === "asc" ? "▲" : "▼"}
            </span>
            <button
              className="chip-x"
              onClick={() => setSort(sort.filter((x) => x.field !== s.field))}
              aria-label={`Remove sort on ${s.field}`}
            >
              ✕
            </button>
          </span>
        ))}
      </div>

      <div className="query-actions">
        <button onClick={() => setEditing("new")}>+ Filter</button>
        {hasAnything && (
          <button
            onClick={() => {
              clearFilters();
              setSort([]);
              setText("");
            }}
          >
            Clear all
          </button>
        )}
      </div>

      {editing !== null && (
        <FilterEditor
          columns={columns}
          field={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}
