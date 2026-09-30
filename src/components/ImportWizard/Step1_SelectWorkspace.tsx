// src/components/ImportWizard/Step1_SelectWorkspace.tsx
import { useState } from "react";
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { NamePrompt } from "../common/NamePrompt";

type Props = {
  selected: string | null;
  onSelect: (id: string) => void;
  onNext: () => void;
  onCancel: () => void;
};

export function Step1_SelectWorkspace({ selected, onSelect, onNext, onCancel }: Props) {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const createWorkspace = useWorkspaceStore((s) => s.createWorkspace);
  const [creating, setCreating] = useState(false);

  return (
    <div className="wizard-step">
      {workspaces.length === 0 && <p className="muted">No workspaces yet. Create one to continue.</p>}
      <ul className="picker-list">
        {workspaces.map((w) => (
          <li key={w.id} className={w.id === selected ? "selected" : ""}>
            <label className="picker-main">
              <input type="radio" name="workspace" checked={w.id === selected} onChange={() => onSelect(w.id)} />
              <strong>{w.name}</strong>
              <span className="muted small">{w.folder_count} folder(s)</span>
            </label>
          </li>
        ))}
      </ul>
      <button className="link-btn" onClick={() => setCreating(true)}>
        + Create new workspace
      </button>

      <div className="wizard-actions">
        <button onClick={onCancel}>Cancel</button>
        <button className="primary" onClick={onNext} disabled={!selected}>
          Next
        </button>
      </div>

      {creating && (
        <NamePrompt
          title="New workspace"
          label="Workspace name"
          onSubmit={async (name) => onSelect((await createWorkspace(name)).id)}
          onClose={() => setCreating(false)}
        />
      )}
    </div>
  );
}
