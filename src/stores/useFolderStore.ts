// src/stores/useFolderStore.ts
// Folders of the CURRENT workspace.
import { create } from "zustand";
import { api, errorText, type Folder } from "../lib/api";
import { useWorkspaceStore } from "./useWorkspaceStore";

export type FolderState = {
  folders: Folder[];
  current_folder_id: string | null;
  /** Workspace the folder list (and current_folder_id) belongs to. */
  workspace_id: string | null;
  loading: boolean;
  error: string | null;

  setFolders: (folders: Folder[]) => void;
  setCurrentFolder: (id: string | null) => void;
  /** Select a folder in a specific workspace (used when navigating across workspaces). */
  selectFolderIn: (workspace_id: string, folder_id: string) => void;
  loadFolders: () => Promise<void>;
  createFolder: (name: string) => Promise<Folder>;
  deleteFolder: (id: string) => Promise<void>;
};

const currentWorkspaceId = () => useWorkspaceStore.getState().current_workspace_id;

export const useFolderStore = create<FolderState>((set, get) => ({
  folders: [],
  current_folder_id: null,
  workspace_id: null,
  loading: false,
  error: null,

  setFolders: (folders) => set({ folders }),

  setCurrentFolder: (id) => set({ current_folder_id: id }),

  selectFolderIn: (workspace_id, folder_id) => set({ workspace_id, current_folder_id: folder_id }),

  loadFolders: async () => {
    const ws = currentWorkspaceId();
    if (!ws) {
      set({ folders: [], current_folder_id: null, workspace_id: null });
      return;
    }
    set({ loading: true, error: null });
    try {
      const folders = await api.folders.list(ws);
      if (currentWorkspaceId() !== ws) return; // workspace changed meanwhile
      const { current_folder_id, workspace_id } = get();
      const keep =
        workspace_id === ws && folders.some((f) => f.id === current_folder_id) ? current_folder_id : null;
      set({ folders, current_folder_id: keep, workspace_id: ws });
    } catch (err) {
      set({ error: errorText(err) });
    } finally {
      set({ loading: false });
    }
  },

  createFolder: async (name) => {
    const ws = currentWorkspaceId();
    if (!ws) throw new Error("Select a workspace first");
    const folder = await api.folders.create(ws, name);
    set({ current_folder_id: folder.id, workspace_id: ws });
    await get().loadFolders();
    await useWorkspaceStore.getState().loadWorkspaces(); // folder_count changed
    return folder;
  },

  deleteFolder: async (id) => {
    const ws = currentWorkspaceId();
    if (!ws) return;
    await api.folders.delete(ws, id);
    if (get().current_folder_id === id) set({ current_folder_id: null });
    await get().loadFolders();
    await useWorkspaceStore.getState().loadWorkspaces();
  },
}));

export const useCurrentFolder = () =>
  useFolderStore((s) => s.folders.find((f) => f.id === s.current_folder_id) ?? null);
