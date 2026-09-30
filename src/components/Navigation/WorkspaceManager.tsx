// src/components/Navigation/WorkspaceManager.tsx
// Modal listing all workspaces: switch, create, delete.
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";

import { errorText } from "../../lib/api";
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { Modal } from "../common/Modal";
import { NamePrompt } from "../common/NamePrompt";

type Props = { onClose: () => void };

export function WorkspaceManager({ onClose }: Props) {
  const { workspaces, current_workspace_id, setCurrentWorkspace, createWorkspace, deleteWorkspace } =
    useWorkspaceStore();
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function remove(id: string, name: string, folders: number) {
    const ok = await ask(
      `Delete workspace "${name}" with ${folders} folder(s)? All its tables will be deleted. This cannot be undone.`,
      { title: "Delete workspace", kind: "warning" },
    );
    if (!ok) return;
    try {
      await deleteWorkspace(id);
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <Modal
      title="Workspaces"
      onClose={onClose}
      footer={
        <>
          <button onClick={() => setCreating(true)}>+ New workspace</button>
          <button className="primary" onClick={onClose}>
            Done
          </button>
        </>
      }
    >
      {error && <p className="error-text">{error}</p>}
      {workspaces.length === 0 && <p className="muted">No workspaces yet.</p>}
      <ul className="picker-list">
        {workspaces.map((w) => (
          <li key={w.id} className={w.id === current_workspace_id ? "selected" : ""}>
            <button
              className="picker-main"
              onClick={() => {
                setCurrentWorkspace(w.id);
                onClose();
              }}
            >
              <strong>{w.name}</strong>
              <span className="muted small">
                {w.folder_count} folder(s) · created {new Date(w.created_at).toLocaleDateString()}
              </span>
            </button>
            <button className="icon-btn danger" title="Delete workspace" onClick={() => void remove(w.id, w.name, w.folder_count)}>
              🗑
            </button>
          </li>
        ))}
      </ul>

      {creating && (
        <NamePrompt
          title="New workspace"
          label="Workspace name"
          placeholder="e.g. Q3 Audit"
          onSubmit={createWorkspace}
          onClose={() => setCreating(false)}
        />
      )}
    </Modal>
  );
}
