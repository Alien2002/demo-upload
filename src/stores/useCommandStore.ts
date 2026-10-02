// src/stores/useCommandStore.ts
//
// ACL commands for the current folder: run, session output, results,
// variables, and the command-line history (↑ / ↓).
import { create } from "zustand";
import {
  api,
  asCommandError,
  errorText,
  type CommandErrorInfo,
  type CommandResponse,
  type ResultInfo,
  type VarInfo,
} from "../lib/api";
import { useWorkspaceStore } from "./useWorkspaceStore";
import { useFolderStore } from "./useFolderStore";
import { useTableStore } from "./useTableStore";
import { useQueryStore } from "./useQueryStore";

export type OutputEntry = {
  id: number;
  text: string;
  /** table the command ran on */
  input: string;
  input_label: string;
  at: Date;
  response: CommandResponse | null;
  error: CommandErrorInfo | null;
};

export type CommandState = {
  running: boolean;
  /** Error of the last run, shown under the command bar */
  last_error: (CommandErrorInfo & { text: string }) | null;
  output: OutputEntry[];
  results: ResultInfo[];
  variables: VarInfo[];
  /** Lines typed, newest last (for ↑ / ↓) */
  recall: string[];

  run: (text: string) => Promise<CommandResponse | null>;
  clearError: () => void;
  clearOutput: () => void;
  loadResults: () => Promise<void>;
  saveResult: (result: string, name: string) => Promise<void>;
  deleteResult: (result: string) => Promise<void>;
  labelFor: (table: string) => string;
};

const RECALL_MAX = 100;
let nextId = 1;

const scope = () => ({
  ws: useWorkspaceStore.getState().current_workspace_id,
  folder: useFolderStore.getState().current_folder_id,
});

export const useCommandStore = create<CommandState>((set, get) => ({
  running: false,
  last_error: null,
  output: [],
  results: [],
  variables: [],
  recall: [],

  run: async (text) => {
    const { ws, folder } = scope();
    const tables = useTableStore.getState();
    const table = tables.current_table;
    const trimmed = text.trim();
    if (!ws || !folder || !table || !trimmed) return null;

    // NEXT starts at the selected record. The grid row equals the record
    // number only when the view isn't filtered/sorted/searched.
    const q = useQueryStore.getState();
    const plainView = Object.keys(q.filters).length === 0 && q.sort.length === 0 && !q.search.trim();
    const currentRow = plainView ? tables.selected_row : null;

    const recall = [...get().recall.filter((r) => r !== trimmed), trimmed].slice(-RECALL_MAX);
    set({ running: true, last_error: null, recall });

    const entry: OutputEntry = {
      id: nextId++,
      text: trimmed,
      input: table,
      input_label: get().labelFor(table),
      at: new Date(),
      response: null,
      error: null,
    };

    try {
      const response = await api.analytics.run(ws, folder, table, trimmed, currentRow);
      entry.response = response;
      set((s) => ({ output: [...s.output, entry], variables: response.variables, running: false }));

      if (response.outcome.kind === "table") {
        if (!response.outcome.permanent) await get().loadResults();
        // Show the new table; the next command runs on it.
        await useTableStore.getState().openTableIn(ws, folder, response.outcome.table);
      }
      return response;
    } catch (err) {
      const e = asCommandError(err);
      entry.error = e;
      set((s) => ({ output: [...s.output, entry], last_error: { ...e, text: trimmed }, running: false }));
      return null;
    }
  },

  clearError: () => set({ last_error: null }),

  clearOutput: () => set({ output: [] }),

  loadResults: async () => {
    const { ws, folder } = scope();
    if (!ws || !folder) {
      set({ results: [], variables: [] });
      return;
    }
    try {
      const [results, variables] = await Promise.all([
        api.analytics.results(ws, folder),
        api.analytics.variables(ws, folder),
      ]);
      const now = scope();
      if (now.ws === ws && now.folder === folder) set({ results, variables });
    } catch (err) {
      console.error("Could not load results", errorText(err));
    }
  },

  saveResult: async (result, name) => {
    const { ws, folder } = scope();
    if (!ws || !folder) return;
    const meta = await api.analytics.saveResult(ws, folder, result, name);
    await useTableStore.getState().refreshTables();
    await useTableStore.getState().openTableIn(ws, folder, meta.table);
  },

  deleteResult: async (result) => {
    const { ws, folder } = scope();
    if (!ws || !folder) return;
    await api.analytics.deleteResult(ws, folder, result);
    if (useTableStore.getState().current_table === result) await useTableStore.getState().openTable(null);
    await get().loadResults();
  },

  /** Friendly name: result label, or the table name. */
  labelFor: (table) => {
    const r = get().results.find((x) => x.table === table);
    return r ? r.label : table;
  },
}));
