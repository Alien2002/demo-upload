// src/components/DataGridView.tsx
//
// Canvas grid (Glide Data Grid) driven by the query model.
// - Rows come from api.tables.query() through the block cache.
// - Changing filters/search/sort gives a new cacheKey: the cache is cleared and
//   the grid is remounted at the top.
// - Row count comes from the query's total_count, not the table's size.
// - Click a header to sort (asc -> desc -> off). Shift+click adds a second sort.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  DataEditor,
  GridCellKind,
  type DataEditorRef,
  type GridCell,
  type GridColumn,
  type HeaderClickedEventArgs,
  type Item,
  type Rectangle,
} from "@glideapps/glide-data-grid";
import "@glideapps/glide-data-grid/dist/index.css";

import { api, errorText, isNumeric, type SortField, type TableMeta } from "../lib/api";
import { useRowCache } from "../hooks/useRowCache";
import { queryShape, toQueryModel, useQueryStore } from "../stores/useQueryStore";

type Props = {
  workspaceId: string;
  folderId: string;
  meta: TableMeta;
};

export function DataGridView({ workspaceId, folderId, meta }: Props) {
  const gridRef = useRef<DataEditorRef>(null);
  const visible = useRef<Rectangle>({ x: 0, y: 0, width: 0, height: 0 });

  const query = useQueryStore();
  const shape = queryShape(query);
  const cacheKey = `${workspaceId}/${folderId}/${meta.table}|${shape}`;

  const [total, setTotal] = useState(meta.row_count);
  const [range, setRange] = useState({ first: 0, last: 0 });
  const [error, setError] = useState<string | null>(null);
  const [widths, setWidths] = useState<Record<string, number>>({});

  // New table: start from its full row count.
  useEffect(() => {
    setTotal(meta.row_count);
    setWidths({});
  }, [meta]);

  useEffect(() => setError(null), [cacheKey]);

  // Latest query in a ref so fetchBlock always uses it.
  const queryRef = useRef(query);
  queryRef.current = query;

  const fetchBlock = useCallback(
    (offset: number, limit: number) =>
      api.tables.query(workspaceId, folderId, meta.table, toQueryModel(queryRef.current, offset, limit)),
    [workspaceId, folderId, meta.table],
  );

  // Repaint only the visible cells of a block that just arrived.
  const onBlockLoaded = useCallback((start: number, end: number) => {
    const v = visible.current;
    const from = Math.max(start, v.y);
    const to = Math.min(end, v.y + v.height);
    if (from >= to) return;
    const cells: { cell: Item }[] = [];
    for (let r = from; r < to; r++) {
      for (let c = v.x; c < v.x + v.width; c++) cells.push({ cell: [c, r] });
    }
    gridRef.current?.updateCells(cells);
  }, []);

  const { getRow, ensureRange } = useRowCache(
    cacheKey,
    fetchBlock,
    onBlockLoaded,
    setTotal,
    (msg) => setError(errorText(msg)),
  );

  // ---- columns (title shows sort direction and order)
  const numericCols = useMemo(() => meta.columns.map((c) => isNumeric(c.sql_type)), [meta]);

  const columns: GridColumn[] = useMemo(
    () =>
      meta.columns.map((c, i) => {
        const sortIndex = query.sort.findIndex((s) => s.field === c.name);
        const s = query.sort[sortIndex];
        const arrow = s ? (s.direction === "asc" ? " ▲" : " ▼") : "";
        const order = s && query.sort.length > 1 ? `${sortIndex + 1}` : "";
        const filtered = query.filters[c.name] ? " ⧩" : "";
        return {
          id: c.name,
          title: `${c.name}${arrow}${order}${filtered}`,
          width: widths[c.name] ?? (numericCols[i] ? 120 : 160),
        };
      }),
    [meta, query.sort, query.filters, widths, numericCols],
  );

  const getCellContent = useCallback(
    ([col, row]: Item): GridCell => {
      const data = getRow(row);
      if (!data) return { kind: GridCellKind.Loading, allowOverlay: false };
      const value = data[col] ?? "";
      return {
        kind: GridCellKind.Text,
        data: value,
        displayData: value,
        allowOverlay: true,
        readonly: true,
        contentAlign: numericCols[col] ? "right" : "left",
      };
    },
    [getRow, numericCols],
  );

  const onVisibleRegionChanged = useCallback(
    (r: Rectangle) => {
      visible.current = r;
      ensureRange(r.y, Math.min(r.y + r.height, Math.max(total, 1)));
      setRange({ first: r.y, last: Math.min(r.y + r.height, total) });
    },
    [ensureRange, total],
  );

  // Kick off the first block for a new query even before Glide reports a region.
  useEffect(() => {
    ensureRange(0, 1);
  }, [cacheKey, ensureRange]);

  // ---- sorting by header click
  const onHeaderClicked = useCallback(
    (colIndex: number, e: HeaderClickedEventArgs) => {
      const field = meta.columns[colIndex]?.name;
      if (!field) return;
      const current = useQueryStore.getState().sort;
      const existing = current.find((s) => s.field === field);
      const next: SortField | null = !existing
        ? { field, direction: "asc" }
        : existing.direction === "asc"
          ? { field, direction: "desc" }
          : null; // third click removes it

      let sort: SortField[];
      if (e.shiftKey) {
        sort = current.filter((s) => s.field !== field);
        if (next) sort.push(next);
      } else {
        sort = next ? [next] : [];
      }
      useQueryStore.getState().setSort(sort);
    },
    [meta],
  );

  const onColumnResize = useCallback((col: GridColumn, newSize: number) => {
    setWidths((w) => ({ ...w, [col.id ?? col.title]: newSize }));
  }, []);

  const shown =
    total === 0
      ? "No matching rows"
      : `Showing ${(range.first + 1).toLocaleString()}–${Math.max(range.first + 1, range.last).toLocaleString()} of ${total.toLocaleString()} rows`;

  return (
    <div className="grid-view">
      {error && <div className="grid-error">{error}</div>}
      <div className="grid-body">
        <DataEditor
          key={cacheKey} // new query -> fresh grid at the top
          ref={gridRef}
          columns={columns}
          rows={total}
          getCellContent={getCellContent}
          onVisibleRegionChanged={onVisibleRegionChanged}
          onHeaderClicked={onHeaderClicked}
          onColumnResize={onColumnResize}
          rowMarkers="number"
          rowHeight={28}
          smoothScrollX
          smoothScrollY
          getCellsForSelection
          width="100%"
          height="100%"
        />
      </div>
      <footer className="grid-footer">
        <span>{shown}</span>
        {total !== meta.row_count && (
          <span className="muted">
            {" "}
            (filtered from {meta.row_count.toLocaleString()})
          </span>
        )}
      </footer>
    </div>
  );
}
