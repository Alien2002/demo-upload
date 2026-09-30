// src/components/Navigation/TopBar.tsx
// Sidebar toggle, breadcrumb, workspace actions, import, settings.
import { useState } from "react";
import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { Breadcrumb } from "./Breadcrumb";
import { WorkspaceManager } from "./WorkspaceManager";
import { NamePrompt } from "../common/NamePrompt";
import { ImportButton } from "../ImportButton";

type Props = { onToggleSidebar: () => void };

export function TopBar({ onToggleSidebar }: Props) {
  const createWorkspace = useWorkspaceStore((s) => s.createWorkspace);
  const [managing, setManaging] = useState(false);
  const [creating, setCreating] = useState(false);

  return (
    <header className="topbar">
      <div className="topbar-left">
        <button className="icon-btn" onClick={onToggleSidebar} title="Toggle sidebar" aria-label="Toggle sidebar">
          ☰
        </button>
        <Breadcrumb />
      </div>
      <div className="topbar-right">
        <button onClick={() => setManaging(true)}>Workspaces</button>
        <button onClick={() => setCreating(true)}>+ New workspace</button>
        <ImportButton />
        <button className="icon-btn" disabled title="Settings (coming soon)" aria-label="Settings">
          ⚙
        </button>
      </div>

      {managing && <WorkspaceManager onClose={() => setManaging(false)} />}
      {creating && (
        <NamePrompt
          title="New workspace"
          label="Workspace name"
          placeholder="e.g. Q3 Audit"
          onSubmit={createWorkspace}
          onClose={() => setCreating(false)}
        />
      )}
    </header>
  );
}
