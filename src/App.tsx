// src/App.tsx
import { useState } from "react";
import { Layout } from "./components/Layout";
import { QueryBar } from "./components/QueryBar/QueryBar";
import { DataGridView } from "./components/DataGridView";
import { CommandBar } from "./components/CommandBar/CommandBar";
import { OutputPanel } from "./components/CommandBar/OutputPanel";
import { NamePrompt } from "./components/common/NamePrompt";
import { useWorkspaceStore } from "./stores/useWorkspaceStore";
import { useCurrentFolder } from "./stores/useFolderStore";
import { useTableStore } from "./stores/useTableStore";
import { useCommandStore } from "./stores/useCommandStore";
import { isResultTable } from "./lib/api";
import "./App.css";

export default function App() {
  return (
    <Layout>
      <MainContent />
    </Layout>
  );
}

function MainContent() {
  const workspaceId = useWorkspaceStore((s) => s.current_workspace_id);
  const hasWorkspaces = useWorkspaceStore((s) => s.workspaces.length > 0);
  const folder = useCurrentFolder();
  const { current_meta, tables, error } = useTableStore();
  const results = useCommandStore((s) => s.results);
  const saveResult = useCommandStore((s) => s.saveResult);
  const [saving, setSaving] = useState(false);

  if (!hasWorkspaces)
    return <Empty title="Welcome" text="Create a workspace with “+ New workspace”, then import a file." />;
  if (!workspaceId || !folder)
    return <Empty title="Select a folder" text="Pick a folder in the sidebar, or create one with “+ New folder”." />;
  if (!current_meta)
    return (
      <Empty
        title={folder.name}
        text={
          error ??
          (tables.length ? "Select a table in the sidebar." : "This folder is empty. Use “Import file” to add a table.")
        }
      />
    );

  const result = isResultTable(current_meta.table) ? results.find((r) => r.table === current_meta.table) : undefined;

  return (
    <div className="table-view">
      <div className="table-header">
        <h1>{result ? result.label : current_meta.table}</h1>
        <span className="muted">
          {current_meta.row_count.toLocaleString()} rows · {current_meta.columns.length} columns
          {result && <> · session result from {labelOf(result.source, results)}</>}
        </span>
        {result && (
          <button className="save-result" onClick={() => setSaving(true)}>
            Save as table…
          </button>
        )}
      </div>
      <CommandBar />
      <QueryBar columns={current_meta.columns} />
      <DataGridView workspaceId={workspaceId} folderId={folder.id} meta={current_meta} />
      <OutputPanel />

      {saving && result && (
        <NamePrompt
          title="Save result as table"
          label="Table name"
          confirmText="Save"
          onSubmit={(name) => saveResult(result.table, name)}
          onClose={() => setSaving(false)}
        />
      )}
    </div>
  );
}

function labelOf(table: string, results: { table: string; label: string }[]) {
  return results.find((r) => r.table === table)?.label ?? table;
}

function Empty({ title, text }: { title: string; text: string }) {
  return (
    <div className="empty-state">
      <h2>{title}</h2>
      <p className="muted">{text}</p>
    </div>
  );
}
