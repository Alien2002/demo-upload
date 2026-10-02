// src/components/CommandBar/CommandBar.tsx
//
// ACL command line for the open table.
//   Enter = run · ↑/↓ = previous commands · Esc = clear
// Errors underline the exact part of the command they refer to.
import { useRef, useState } from "react";
import { useCommandStore } from "../../stores/useCommandStore";
import { useTableStore } from "../../stores/useTableStore";

const EXAMPLES = [
  "COUNT IF Amount > 1000",
  "TOTAL Amount IF [Txn Date] >= `20240101`",
  "EXTRACT FIELDS Account Amount IF Amount > 1000",
  "SORT ON Amount D",
  "SUMMARIZE ON Account SUBTOTAL Amount",
];

export function CommandBar() {
  const { run, running, last_error, clearError, recall } = useCommandStore();
  const table = useTableStore((s) => s.current_table);
  const selectedRow = useTableStore((s) => s.selected_row);
  const labelFor = useCommandStore((s) => s.labelFor);

  const [text, setText] = useState("");
  const [recallIndex, setRecallIndex] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  async function submit() {
    if (!text.trim() || running) return;
    const ok = await run(text);
    if (ok) {
      setText("");
      setRecallIndex(null);
    }
    inputRef.current?.focus();
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault();
      void submit();
    } else if (e.key === "ArrowUp" && recall.length) {
      e.preventDefault();
      const i = recallIndex === null ? recall.length - 1 : Math.max(0, recallIndex - 1);
      setRecallIndex(i);
      setText(recall[i]);
    } else if (e.key === "ArrowDown" && recallIndex !== null) {
      e.preventDefault();
      const i = recallIndex + 1;
      if (i >= recall.length) {
        setRecallIndex(null);
        setText("");
      } else {
        setRecallIndex(i);
        setText(recall[i]);
      }
    } else if (e.key === "Escape") {
      setText("");
      setRecallIndex(null);
      clearError();
    }
  }

  const showError = last_error && last_error.text === text.trim();

  return (
    <div className="command-bar">
      <div className="command-row">
        <span className="command-prompt" title={`Commands run on ${table ? labelFor(table) : "—"}`}>
          ACL&gt;
        </span>
        <input
          ref={inputRef}
          className={`command-input ${showError ? "invalid" : ""}`}
          value={text}
          placeholder={EXAMPLES[0] + "   (↑ for history)"}
          spellCheck={false}
          autoComplete="off"
          disabled={!table}
          onChange={(e) => {
            setText(e.target.value);
            setRecallIndex(null);
          }}
          onKeyDown={onKeyDown}
          aria-label="ACL command"
        />
        <button className="primary" onClick={() => void submit()} disabled={!text.trim() || running || !table}>
          {running ? "Running…" : "Run"}
        </button>
      </div>

      {showError && last_error && (
        <div className="command-error" role="alert">
          {last_error.end > last_error.start && (
            <pre className="command-echo">
              {last_error.text.slice(0, last_error.start)}
              <mark>{last_error.text.slice(last_error.start, last_error.end) || " "}</mark>
              {last_error.text.slice(last_error.end)}
            </pre>
          )}
          <span>{last_error.message}</span>
        </div>
      )}

      {!showError && (
        <div className="command-hint muted small">
          Runs on <strong>{table ? labelFor(table) : "—"}</strong>
          {selectedRow !== null && <> · NEXT starts at row {(selectedRow + 1).toLocaleString()}</>}
          {" · "}e.g.{" "}
          {EXAMPLES.slice(1, 4).map((ex, i) => (
            <button key={i} className="example" onClick={() => { setText(ex); inputRef.current?.focus(); }}>
              {ex}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
