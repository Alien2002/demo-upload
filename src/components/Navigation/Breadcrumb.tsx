// src/components/Navigation/Breadcrumb.tsx
// Workspace / Folder / Table — each part navigates back to that level.
import { useCurrentWorkspace } from "../../stores/useWorkspaceStore";
import { useCurrentFolder, useFolderStore } from "../../stores/useFolderStore";
import { useTableStore } from "../../stores/useTableStore";
import { useCommandStore } from "../../stores/useCommandStore";

export function Breadcrumb() {
  const workspace = useCurrentWorkspace();
  const folder = useCurrentFolder();
  const table = useTableStore((s) => s.current_table);
  const openTable = useTableStore((s) => s.openTable);
  const setCurrentFolder = useFolderStore((s) => s.setCurrentFolder);
  const labelFor = useCommandStore((s) => s.labelFor);
  useCommandStore((s) => s.results); // re-render when result labels load

  if (!workspace) return <span className="breadcrumb muted">No workspace selected</span>;

  return (
    <nav className="breadcrumb" aria-label="Location">
      <button
        className="crumb"
        onClick={() => {
          setCurrentFolder(null);
          void openTable(null);
        }}
      >
        {workspace.name}
      </button>
      {folder && (
        <>
          <span className="sep">/</span>
          <button className="crumb" onClick={() => void openTable(null)}>
            {folder.name}
          </button>
        </>
      )}
      {folder && table && (
        <>
          <span className="sep">/</span>
          <span className="crumb current">{labelFor(table)}</span>
        </>
      )}
    </nav>
  );
}
