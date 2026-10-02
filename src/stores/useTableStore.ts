// src/stores/useTableStore.ts
// Tables of the CURRENT folder, and which one is open in the grid.
import { create } from "zustand";
import { api, errorText, type TableMeta } from "../lib/api";
import { useWorkspaceStore } from "./useWorkspaceStore";
import { useFolderStore } from "./useFolderStore";

export type TableState = {
  tables: string[];
  current_table: string | null;
  current_meta: TableMeta | null;
  /** "workspace/folder" the table list and open table belong to. */
  scope_key: string | null;
  /** Selected grid row (where NEXT starts), null if none. */
  selected_row: number | null;
  loading: boolean;
  error: string | null;

  setTables: (tables: string[]) => void;
  setSelectedRow: (row: number | null) => void;
  refreshTables: () => Promise<void>;
  /** Open a table in the current folder (null closes it). */
  openTable: (table: string | null) => Promise<void>;
  /** Open a table in a specific workspace/folder (used after an import). */
  openTableIn: (workspace_id: string, folder_id: string, table: string) => Promise<void>;
};

const currentScope = () => {
  const ws = useWorkspaceStore.getState().current_workspace_id;
  const fs = useFolderStore.getState();
  // A folder id only counts if it belongs to the current workspace
  // (right after a workspace switch it may still be the old one).
  const folder = fs.workspace_id === ws ? fs.current_folder_id : null;
  return { ws, folder, key: ws && folder ? `${ws}/${folder}` : null };
};

export const useTableStore = create<TableState>((set, get) => ({
  tables: [],
  current_table: null,
  current_meta: null,
  scope_key: null,
  selected_row: null,
  loading: false,
  error: null,

  setTables: (tables) => set({ tables }),

  setSelectedRow: (row) => set({ selected_row: row }),

  refreshTables: async () => {
    const { ws, folder, key } = currentScope();
    if (!ws || !folder) {
      set({ tables: [], current_table: null, current_meta: null, scope_key: null });
      return;
    }
    set({ loading: true, error: null });
    try {
      const tables = await api.tables.list(ws, folder);
      if (currentScope().key !== key) return; // selection changed meanwhile
      const { current_table, scope_key } = get();
      const keep = scope_key === key && current_table && tables.includes(current_table);
      // Session results ("_results.r_n") aren't in the table list but stay open.
      const isResult = current_table?.startsWith("_results.");
      set({
        tables,
        scope_key: key,
        ...(keep || (isResult && scope_key === key) ? {} : { current_table: null, current_meta: null }),
      });
    } catch (err) {
      set({ error: errorText(err) });
    } finally {
      set({ loading: false });
    }
  },

  openTable: async (table) => {
    const { ws, folder } = currentScope();
    if (!table || !ws || !folder) {
      set({ current_table: null, current_meta: null });
      return;
    }
    await get().openTableIn(ws, folder, table);
  },

  openTableIn: async (workspace_id, folder_id, table) => {
    try {
      const meta = await api.tables.meta(workspace_id, folder_id, table);
      set({
        current_table: table,
        current_meta: meta,
        scope_key: `${workspace_id}/${folder_id}`,
        selected_row: null,
        error: null,
      });
    } catch (err) {
      set({ error: errorText(err) });
    }
  },
}));
