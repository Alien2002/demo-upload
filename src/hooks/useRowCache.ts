// src/hooks/useRowCache.ts
//
// Block cache between the grid and Rust.
// - Rows are fetched in fixed blocks (BLOCK_SIZE rows each).
// - Only blocks near the viewport are requested; duplicates are skipped.
// - Old blocks are evicted (LRU) so memory stays flat.
// - Switching tables bumps a generation counter, so late responses
//   for the previous table are thrown away instead of painting stale rows.

import { useCallback, useEffect, useRef } from "react";
import { api, type Row } from "../lib/api";

const BLOCK_SIZE = 500;
const MAX_BLOCKS = 120; // ~60k rows kept in memory

export function useRowCache(
  table: string | null,
  onBlockLoaded: (startRow: number, endRow: number) => void,
) {
  const blocks = useRef(new Map<number, Row[]>());
  const pending = useRef(new Set<number>());
  const generation = useRef(0);
  const loadedCb = useRef(onBlockLoaded);
  loadedCb.current = onBlockLoaded;

  // Reset everything when the table changes.
  useEffect(() => {
    generation.current += 1;
    blocks.current.clear();
    pending.current.clear();
  }, [table]);

  const getRow = useCallback((row: number): Row | undefined => {
    const b = Math.floor(row / BLOCK_SIZE);
    const block = blocks.current.get(b);
    return block?.[row - b * BLOCK_SIZE];
  }, []);

  const loadBlock = useCallback(
    async (b: number) => {
      if (!table || blocks.current.has(b) || pending.current.has(b)) return;
      const gen = generation.current;
      pending.current.add(b);
      try {
        const rows = await api.fetchRows(table, b * BLOCK_SIZE, BLOCK_SIZE);
        if (gen !== generation.current) return; // stale response
        blocks.current.set(b, rows);
        evict();
        loadedCb.current(b * BLOCK_SIZE, b * BLOCK_SIZE + rows.length);
      } catch (err) {
        console.error(`Failed to load block ${b}`, err);
      } finally {
        if (gen === generation.current) pending.current.delete(b);
      }
    },
    [table],
  );

  const evict = () => {
    while (blocks.current.size > MAX_BLOCKS) {
      const oldest = blocks.current.keys().next().value;
      if (oldest === undefined) break;
      blocks.current.delete(oldest);
    }
  };

  /** Make sure rows [start, end) plus a buffer on each side are loaded. */
  const ensureRange = useCallback(
    (start: number, end: number) => {
      const first = Math.max(0, Math.floor(start / BLOCK_SIZE) - 1);
      const last = Math.floor(end / BLOCK_SIZE) + 1;
      for (let b = first; b <= last; b++) {
        const block = blocks.current.get(b);
        if (block) {
          // touch: move to the end so LRU keeps visible blocks
          blocks.current.delete(b);
          blocks.current.set(b, block);
        } else {
          void loadBlock(b);
        }
      }
    },
    [loadBlock],
  );

  return { getRow, ensureRange };
}
