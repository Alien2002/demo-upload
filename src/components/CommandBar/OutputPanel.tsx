// src/components/CommandBar/OutputPanel.tsx
//
// Session output: every command run, its values or result table, timing,
// and the generated SQL (for checking what the engine did).
import { useEffect, useRef, useState } from "react";
import { useCommandStore, type OutputEntry } from "../../stores/useCommandStore";
import { useTableStore } from "../../stores/useTableStore";

export function OutputPanel() {
  const { output, variables, clearOutput } = useCommandStore();
  const [open, setOpen] = useState(true);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "nearest" });
  }, [output.length]);

  return (
    <section className={`output-panel ${open ? "open" : ""}`}>
      <header className="output-header">
        <button className="link-btn" onClick={() => setOpen((o) => !o)}>
          {open ? "▾" : "▸"} Output {output.length > 0 && <span className="badge">{output.length}</span>}
        </button>
        {variables.length > 0 && (
          <span className="vars muted small" title="Variables you can use in later commands">
            {variables.map((v) => (
              <code key={v.name}>
                {v.name}={v.value}
              </code>
            ))}
          </span>
        )}
        {open && output.length > 0 && (
          <button className="link-btn small" onClick={clearOutput}>
            Clear
          </button>
        )}
      </header>
      {open && (
        <div className="output-body">
          {output.length === 0 && <p className="muted small">Results of your commands appear here.</p>}
          {output.map((e) => (
            <OutputRow key={e.id} entry={e} />
          ))}
          <div ref={endRef} />
        </div>
      )}
    </section>
  );
}

function OutputRow({ entry }: { entry: OutputEntry }) {
  const [showSql, setShowSql] = useState(false);
  const openTable = useTableStore((s) => s.openTable);
  const r = entry.response;

  return (
    <div className={`output-row ${entry.error ? "failed" : ""}`}>
      <div className="output-cmd">
        <code>{entry.text}</code>
        <span className="muted small">
          on {entry.input_label} · {entry.at.toLocaleTimeString()}
          {r && ` · ${r.duration_ms.toLocaleString()} ms`}
        </span>
      </div>

      {entry.error && <div className="error-text small">✗ {entry.error.message}</div>}

      {r?.outcome.kind === "scalars" && (
        <div className="scalars">
          {r.outcome.items.map((i) => (
            <span key={i.var} className="scalar">
              <span className="muted small">{i.label}</span>
              <strong>{formatValue(i.value)}</strong>
              <code className="small muted">{i.var}</code>
            </span>
          ))}
        </div>
      )}

      {r?.outcome.kind === "table" && (
        <div className="small">
          → {r.outcome.permanent ? "table" : "result"}{" "}
          <button className="link-btn" onClick={() => void openTable((r.outcome as { table: string }).table)}>
            {r.outcome.permanent ? r.outcome.table : r.outcome.label}
          </button>{" "}
          · {r.outcome.meta.row_count.toLocaleString()} rows · {r.outcome.meta.columns.length} columns
        </div>
      )}

      {r && (
        <>
          <button className="link-btn small" onClick={() => setShowSql((s) => !s)}>
            {showSql ? "Hide SQL" : "SQL"}
          </button>
          {showSql && <pre className="sql">{r.sql}</pre>}
        </>
      )}
    </div>
  );
}

function formatValue(v: string | null) {
  if (v === null) return "(blank)";
  const n = Number(v);
  return Number.isFinite(n) && v.trim() !== "" ? n.toLocaleString(undefined, { maximumFractionDigits: 6 }) : v;
}
