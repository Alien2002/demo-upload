// src/stores/useWorkspaceStore.ts
import { create } from "zustand";
import { api, errorText, type Workspace } from "../lib/api";

export type WorkspaceState = {
  workspaces: Workspace[];
  current_workspace_id: string | null;
  loading: boolean;
  error: string | null;

  setWorkspaces: (workspaces: Workspace[]) => void;
  setCurrentWorkspace: (id: string | null) => void;
  loadWorkspaces: () => Promise<void>;
  createWorkspace: (name: string) => Promise<Workspace>;
  deleteWorkspace: (id: string) => Promise<void>;
};

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  workspaces: [],
  current_workspace_id: null,
  loading: false,
  error: null,

  setWorkspaces: (workspaces) => set({ workspaces }),

  setCurrentWorkspace: (id) => set({ current_workspace_id: id }),

  loadWorkspaces: async () => {
    set({ loading: true, error: null });
    try {
      const workspaces = await api.workspaces.list();
      const current = get().current_workspace_id;
      const stillExists = workspaces.some((w) => w.id === current);
      set({
        workspaces,
        current_workspace_id: stillExists ? current : (workspaces[0]?.id ?? null),
      });
    } catch (err) {
      set({ error: errorText(err) });
    } finally {
      set({ loading: false });
    }
  },

  createWorkspace: async (name) => {
    const ws = await api.workspaces.create(name);
    await get().loadWorkspaces();
    set({ current_workspace_id: ws.id });
    return ws;
  },

  deleteWorkspace: async (id) => {
    await api.workspaces.delete(id);
    if (get().current_workspace_id === id) set({ current_workspace_id: null });
    await get().loadWorkspaces();
  },
}));

export const useCurrentWorkspace = () =>
  useWorkspaceStore((s) => s.workspaces.find((w) => w.id === s.current_workspace_id) ?? null);
