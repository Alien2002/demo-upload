// src/hooks/useRowCache.ts
//
// Block cache between the grid and Rust.
// - Rows are fetched in fixed blocks (BLOCK_SIZE rows each) through `fetchBlock`.
// - Only blocks near the viewport are requested; duplicates are skipped.
// - Old blocks are evicted (LRU) so memory stays flat.
// - A new cacheKey (different table OR different filters/search/sort) clears
//   everything and bumps a generation counter, so late responses from the
//   previous query are dropped instead of painting stale rows.

import { useCallback, useEffect, useRef } from "react";
import type { QueryResult, Row } from "../lib/api";

export const BLOCK_SIZE = 500;
const MAX_BLOCKS = 120; // ~60k rows kept in memory

export type FetchBlock = (offset: number, limit: number) => Promise<QueryResult>;

export function useRowCache(
  cacheKey: string | null,
  fetchBlock: FetchBlock,
  onBlockLoaded: (startRow: number, endRow: number) => void,
  onTotal: (total: number) => void,
  onError: (message: string) => void,
) {
  const blocks = useRef(new Map<number, Row[]>());
  const pending = useRef(new Set<number>());
  const generation = useRef(0);

  // Keep the latest callbacks without changing the identity of getRow/ensureRange.
  const cb = useRef({ fetchBlock, onBlockLoaded, onTotal, onError });
  cb.current = { fetchBlock, onBlockLoaded, onTotal, onError };

  useEffect(() => {
    generation.current += 1;
    blocks.current.clear();
    pending.current.clear();
  }, [cacheKey]);

  const getRow = useCallback((row: number): Row | undefined => {
    const b = Math.floor(row / BLOCK_SIZE);
    return blocks.current.get(b)?.[row - b * BLOCK_SIZE];
  }, []);

  const loadBlock = useCallback(
    async (b: number) => {
      if (cacheKey === null || blocks.current.has(b) || pending.current.has(b)) return;
      const gen = generation.current;
      pending.current.add(b);
      try {
        const result = await cb.current.fetchBlock(b * BLOCK_SIZE, BLOCK_SIZE);
        if (gen !== generation.current) return; // stale: query changed meanwhile
        blocks.current.set(b, result.rows);
        while (blocks.current.size > MAX_BLOCKS) {
          const oldest = blocks.current.keys().next().value;
          if (oldest === undefined) break;
          blocks.current.delete(oldest);
        }
        cb.current.onTotal(result.total_count);
        cb.current.onBlockLoaded(b * BLOCK_SIZE, b * BLOCK_SIZE + result.rows.length);
      } catch (err) {
        if (gen === generation.current) cb.current.onError(String(err));
      } finally {
        if (gen === generation.current) pending.current.delete(b);
      }
    },
    [cacheKey],
  );

  /** Make sure rows [start, end) plus one block either side are loaded. */
  const ensureRange = useCallback(
    (start: number, end: number) => {
      const first = Math.max(0, Math.floor(start / BLOCK_SIZE) - 1);
      const last = Math.floor(Math.max(start, end - 1) / BLOCK_SIZE) + 1;
      for (let b = first; b <= last; b++) {
        const block = blocks.current.get(b);
        if (block) {
          blocks.current.delete(b); // touch: most recently used goes last
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
