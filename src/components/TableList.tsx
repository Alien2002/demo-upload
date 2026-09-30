// src/components/TableList.tsx
//
// Sidebar listing the tables in the workspace (like ACL's Navigator).

type Props = {
  tables: string[];
  active: string | null;
  onSelect: (table: string) => void;
};

export function TableList({ tables, active, onSelect }: Props) {
  if (tables.length === 0) {
    return <p className="empty">No tables yet. Import a file to start.</p>;
  }
  return (
    <ul className="table-list">
      {tables.map((t) => (
        <li key={t}>
          <button className={t === active ? "active" : ""} onClick={() => onSelect(t)}>
            {t}
          </button>
        </li>
      ))}
    </ul>
  );
}
