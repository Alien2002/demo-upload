// src/stores/useQueryStore.ts
//
// Filters / search / sort for the open table.
// Saved per table in localStorage, so reopening a table restores its view
// and one table's filters never leak onto another table's columns.
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { FilterCondition, QueryModel, SortField } from "../lib/api";

export type QuerySlice = {
  filters: Record<string, FilterCondition>;
  search: string;
  sort: SortField[];
  offset: number;
  limit: number;
};

export type QueryState = QuerySlice & {
  /** "workspace/folder/table" whose query is currently loaded */
  table_key: string | null;
  saved: Record<string, QuerySlice>;

  bindTable: (key: string | null) => void;
  setFilter: (field: string, condition: FilterCondition) => void;
  clearFilter: (field: string) => void;
  clearFilters: () => void;
  setSearch: (query: string) => void;
  setSort: (sort: SortField[]) => void;
  resetQuery: () => void;
};

export const DEFAULT_QUERY: QuerySlice = {
  filters: {},
  search: "",
  sort: [],
  offset: 0,
  limit: 100,
};

export const tableKey = (ws: string, folder: string, table: string) => `${ws}/${folder}/${table}`;

export const useQueryStore = create<QueryState>()(
  persist(
    (set, get) => {
      /** Update the live query and remember it for the current table. */
      const commit = (patch: Partial<QuerySlice>) => {
        const next = { ...pick(get()), ...patch };
        const key = get().table_key;
        set({ ...next, saved: key ? { ...get().saved, [key]: next } : get().saved });
      };

      return {
        ...DEFAULT_QUERY,
        table_key: null,
        saved: {},

        bindTable: (key) => {
          if (key === get().table_key) return;
          const restored = key ? (get().saved[key] ?? DEFAULT_QUERY) : DEFAULT_QUERY;
          set({ ...restored, table_key: key });
        },

        setFilter: (field, condition) =>
          commit({ filters: { ...get().filters, [field]: condition }, offset: 0 }),

        clearFilter: (field) => {
          const { [field]: _removed, ...rest } = get().filters;
          commit({ filters: rest, offset: 0 });
        },

        clearFilters: () => commit({ filters: {}, search: "", offset: 0 }),

        setSearch: (query) => commit({ search: query, offset: 0 }),

        setSort: (sort) => commit({ sort, offset: 0 }),

        resetQuery: () => commit({ ...DEFAULT_QUERY }),
      };
    },
    {
      name: "egrid-query-preferences",
      partialize: (s) => ({ saved: s.saved }),
    },
  ),
);

function pick(s: QuerySlice): QuerySlice {
  return { filters: s.filters, search: s.search, sort: s.sort, offset: s.offset, limit: s.limit };
}

/** The part of the query that changes the result set (not paging). */
export const queryShape = (s: QuerySlice) =>
  JSON.stringify({ f: s.filters, q: s.search.trim(), s: s.sort });

export const toQueryModel = (s: QuerySlice, offset: number, limit: number): QueryModel => ({
  offset,
  limit,
  sort: s.sort,
  filters: s.filters,
  search: s.search.trim() || undefined,
});
