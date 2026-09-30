// src/components/DataGridView.tsx
//
// Canvas-based grid (Glide Data Grid). Only the visible cells are drawn,
// so 1 row or 10 million rows cost the same to render.

import { useCallback, useMemo, useRef } from "react";
import {
  DataEditor,
  GridCellKind,
  type DataEditorRef,
  type GridCell,
  type GridColumn,
  type Item,
  type Rectangle,
} from "@glideapps/glide-data-grid";
import "@glideapps/glide-data-grid/dist/index.css";

import { isNumeric, type TableMeta } from "../lib/api";
import { useRowCache } from "../hooks/useRowCache";

type Props = { meta: TableMeta };

export function DataGridView({ meta }: Props) {
  const gridRef = useRef<DataEditorRef>(null);
  const visible = useRef<Rectangle>({ x: 0, y: 0, width: 0, height: 0 });

  // When a block arrives, repaint only the visible cells inside it.
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

  const { getRow, ensureRange } = useRowCache(meta.table, onBlockLoaded);

  const columns: GridColumn[] = useMemo(
    () =>
      meta.columns.map((c) => ({
        id: c.name,
        title: c.name,
        width: isNumeric(c.sql_type) ? 120 : 160,
      })),
    [meta],
  );

  const numericCols = useMemo(
    () => meta.columns.map((c) => isNumeric(c.sql_type)),
    [meta],
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
    (range: Rectangle) => {
      visible.current = range;
      const end = Math.min(range.y + range.height, meta.row_count);
      ensureRange(range.y, end);
    },
    [ensureRange, meta.row_count],
  );

  return (
    <DataEditor
      key={meta.table} // fresh grid state per table
      ref={gridRef}
      columns={columns}
      rows={meta.row_count}
      getCellContent={getCellContent}
      onVisibleRegionChanged={onVisibleRegionChanged}
      rowMarkers="number"
      rowHeight={28}
      smoothScrollX
      smoothScrollY
      getCellsForSelection
      width="100%"
      height="100%"
    />
  );
}
