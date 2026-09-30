// src/components/TableList.tsx
// List of tables in a folder.

type Props = {
  tables: string[];
  active: string | null;
  loading?: boolean;
  onSelect: (table: string) => void;
};

export function TableList({ tables, active, loading = false, onSelect }: Props) {
  if (loading && tables.length === 0) return <p className="muted small">Loading…</p>;
  if (tables.length === 0) return <p className="muted small">No tables. Use Import to add one.</p>;
  return (
    <ul className="table-list">
      {tables.map((t) => (
        <li key={t}>
          <button className={t === active ? "active" : ""} onClick={() => onSelect(t)} title={t}>
            <span className="table-icon">▦</span> {t}
          </button>
        </li>
      ))}
    </ul>
  );
}
