// src/components/ImportWizard/Step2_SelectFolder.tsx
// Folders of the workspace chosen in step 1 (which may not be the current one).
import { useCallback, useEffect, useState } from "react";
import { api, errorText, type Folder } from "../../lib/api";
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { useFolderStore } from "../../stores/useFolderStore";
import { NamePrompt } from "../common/NamePrompt";

type Props = {
  workspaceId: string;
  selected: string | null;
  onSelect: (id: string) => void;
  onBack: () => void;
  onNext: () => void;
};

export function Step2_SelectFolder({ workspaceId, selected, onSelect, onBack, onNext }: Props) {
  const [folders, setFolders] = useState<Folder[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setFolders(await api.folders.list(workspaceId));
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void load();
  }, [load]);

  async function create(name: string) {
    const folder = await api.folders.create(workspaceId, name);
    await load();
    onSelect(folder.id);
    // Keep the sidebar in step if this is the workspace on screen.
    if (useWorkspaceStore.getState().current_workspace_id === workspaceId) {
      await useFolderStore.getState().loadFolders();
    }
    await useWorkspaceStore.getState().loadWorkspaces();
  }

  return (
    <div className="wizard-step">
      {loading && <p className="muted">Loading folders…</p>}
      {error && <p className="error-text">{error}</p>}
      {!loading && folders.length === 0 && <p className="muted">This workspace has no folders yet.</p>}
      <ul className="picker-list">
        {folders.map((f) => (
          <li key={f.id} className={f.id === selected ? "selected" : ""}>
            <label className="picker-main">
              <input type="radio" name="folder" checked={f.id === selected} onChange={() => onSelect(f.id)} />
              <strong>{f.name}</strong>
              <span className="muted small">{f.table_count} table(s)</span>
            </label>
          </li>
        ))}
      </ul>
      <button className="link-btn" onClick={() => setCreating(true)}>
        + Create new folder
      </button>

      <div className="wizard-actions">
        <button onClick={onBack}>Back</button>
        <button className="primary" onClick={onNext} disabled={!selected}>
          Next
        </button>
      </div>

      {creating && (
        <NamePrompt title="New folder" label="Folder name" onSubmit={create} onClose={() => setCreating(false)} />
      )}
    </div>
  );
}
