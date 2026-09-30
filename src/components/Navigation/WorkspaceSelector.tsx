// src/components/Navigation/WorkspaceSelector.tsx
// Dropdown to switch the current workspace.
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";

export function WorkspaceSelector() {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const current = useWorkspaceStore((s) => s.current_workspace_id);
  const setCurrent = useWorkspaceStore((s) => s.setCurrentWorkspace);

  if (workspaces.length === 0) return <span className="muted">No workspaces</span>;

  return (
    <select
      className="workspace-select"
      value={current ?? ""}
      onChange={(e) => setCurrent(e.target.value || null)}
      title="Switch workspace"
    >
      {workspaces.map((w) => (
        <option key={w.id} value={w.id}>
          {w.name}
        </option>
      ))}
    </select>
  );
}
