// src/hooks/useNavigationSync.ts
//
// Keeps the stores in step with each other and with the backend:
// - load workspaces on start
// - workspace changes -> load its folders
// - folder changes    -> load its tables
// - table changes     -> load that table's saved filters/sort
// - "import-complete" / "tables-changed" -> refresh counts and table lists
// - folder changes    -> load its session results, clear the output panel
// Mounted once, in Layout.

import { useEffect } from "react";
import { api } from "../lib/api";
import { useWorkspaceStore } from "../stores/useWorkspaceStore";
import { useFolderStore } from "../stores/useFolderStore";
import { useTableStore } from "../stores/useTableStore";
import { tableKey, useQueryStore } from "../stores/useQueryStore";
import { useCommandStore } from "../stores/useCommandStore";

export function useNavigationSync() {
  const workspaceId = useWorkspaceStore((s) => s.current_workspace_id);
  const folderId = useFolderStore((s) => s.current_folder_id);
  const table = useTableStore((s) => s.current_table);

  useEffect(() => {
    void useWorkspaceStore.getState().loadWorkspaces();
  }, []);

  // The stores drop a selection that doesn't belong to the new scope,
  // but keep one set deliberately with selectFolderIn / openTableIn.
  useEffect(() => {
    void useFolderStore.getState().loadFolders();
  }, [workspaceId]);

  useEffect(() => {
    void useTableStore.getState().refreshTables();
    useCommandStore.getState().clearOutput();
    void useCommandStore.getState().loadResults();
  }, [workspaceId, folderId]);

  useEffect(() => {
    useQueryStore
      .getState()
      .bindTable(workspaceId && folderId && table ? tableKey(workspaceId, folderId, table) : null);
  }, [workspaceId, folderId, table]);

  useEffect(() => {
    const unlisten = api.tables.onImportComplete(async (e) => {
      await useWorkspaceStore.getState().loadWorkspaces();
      if (useWorkspaceStore.getState().current_workspace_id === e.workspace_id) {
        await useFolderStore.getState().loadFolders();
      }
      if (useFolderStore.getState().current_folder_id === e.folder_id) {
        await useTableStore.getState().refreshTables();
      }
    });
    // Tables created by commands (TO "name") or by saving a result.
    const unlistenTables = api.analytics.onTablesChanged(async (e) => {
      if (useWorkspaceStore.getState().current_workspace_id === e.workspace_id) {
        await useFolderStore.getState().loadFolders();
      }
      if (useFolderStore.getState().current_folder_id === e.folder_id) {
        await useTableStore.getState().refreshTables();
      }
    });
    return () => {
      void unlisten.then((f) => f());
      void unlistenTables.then((f) => f());
    };
  }, []);
}
