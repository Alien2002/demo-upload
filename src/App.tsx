// src/App.tsx
import { Layout } from "./components/Layout";
import { QueryBar } from "./components/QueryBar/QueryBar";
import { DataGridView } from "./components/DataGridView";
import { useWorkspaceStore } from "./stores/useWorkspaceStore";
import { useCurrentFolder } from "./stores/useFolderStore";
import { useTableStore } from "./stores/useTableStore";
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

  return (
    <div className="table-view">
      <div className="table-header">
        <h1>{current_meta.table}</h1>
        <span className="muted">
          {current_meta.row_count.toLocaleString()} rows · {current_meta.columns.length} columns
        </span>
      </div>
      <QueryBar columns={current_meta.columns} />
      <DataGridView workspaceId={workspaceId} folderId={folder.id} meta={current_meta} />
    </div>
  );
}

function Empty({ title, text }: { title: string; text: string }) {
  return (
    <div className="empty-state">
      <h2>{title}</h2>
      <p className="muted">{text}</p>
    </div>
  );
}
