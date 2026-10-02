// src/components/Navigation/Sidebar.tsx
//
// Workspace switcher, folders of that workspace, and the tables of the
// selected folder (like ACL's Navigator). Right-click a folder for options.
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";

import { errorText, type Folder } from "../../lib/api";
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { useFolderStore } from "../../stores/useFolderStore";
import { useTableStore } from "../../stores/useTableStore";
import { WorkspaceSelector } from "./WorkspaceSelector";
import { TableList } from "../TableList";
import { ResultsList } from "./ResultsList";
import { ContextMenu } from "../common/ContextMenu";
import { NamePrompt } from "../common/NamePrompt";

type Props = { collapsed: boolean };

export function Sidebar({ collapsed }: Props) {
  const workspaceId = useWorkspaceStore((s) => s.current_workspace_id);
  const { folders, current_folder_id, loading, error, setCurrentFolder, createFolder, deleteFolder } =
    useFolderStore();

  const [menu, setMenu] = useState<{ x: number; y: number; folder: Folder } | null>(null);
  const [creating, setCreating] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  if (collapsed) return null;

  async function handleDelete(folder: Folder) {
    const ok = await ask(
      `Delete folder "${folder.name}" and its ${folder.table_count} table(s)? This cannot be undone.`,
      { title: "Delete folder", kind: "warning" },
    );
    if (!ok) return;
    try {
      await deleteFolder(folder.id);
    } catch (err) {
      setActionError(errorText(err));
    }
  }

  return (
    <nav className="sidebar">
      <div className="sidebar-section">
        <div className="section-label">Workspace</div>
        <WorkspaceSelector />
      </div>

      <div className="sidebar-section grow">
        <div className="section-label">Folders</div>
        {!workspaceId && <p className="muted small">Create a workspace to start.</p>}
        {loading && folders.length === 0 && <p className="muted small">Loading…</p>}
        {(error || actionError) && <p className="error-text small">{error ?? actionError}</p>}

        <ul className="folder-list">
          {folders.map((f) => {
            const active = f.id === current_folder_id;
            return (
              <li key={f.id}>
                <button
                  className={`folder-item ${active ? "active" : ""}`}
                  onClick={() => setCurrentFolder(active ? null : f.id)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, folder: f });
                  }}
                  title={f.path}
                >
                  <span className="folder-icon">{active ? "📂" : "📁"}</span>
                  <span className="folder-name">{f.name}</span>
                  <span className="badge">{f.table_count}</span>
                </button>
                {active && <FolderTables />}
              </li>
            );
          })}
        </ul>
      </div>

      <div className="sidebar-footer">
        <button onClick={() => setCreating(true)} disabled={!workspaceId}>
          + New folder
        </button>
      </div>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { label: "Open", onClick: () => setCurrentFolder(menu.folder.id) },
            { label: "Rename", disabled: true, hint: "soon" },
            { label: "Delete…", danger: true, onClick: () => void handleDelete(menu.folder) },
          ]}
        />
      )}

      {creating && (
        <NamePrompt
          title="New folder"
          label="Folder name"
          placeholder="e.g. General Ledger"
          onSubmit={createFolder}
          onClose={() => setCreating(false)}
        />
      )}
    </nav>
  );
}

/** Tables inside the selected folder. */
function FolderTables() {
  const { tables, current_table, loading, openTable } = useTableStore();
  return (
    <div className="folder-tables">
      <TableList
        tables={tables}
        active={current_table}
        loading={loading}
        onSelect={(t) => void openTable(t)}
      />
      <ResultsList />
    </div>
  );
}
