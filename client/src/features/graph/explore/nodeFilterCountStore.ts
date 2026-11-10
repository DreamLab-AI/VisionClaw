/**
 * Node-filter counts (ADR-2192): how many nodes the server streams to this client
 * (`visible`) out of the nodes in its graph view (`total`). The server sends them in
 * `filter_update_success` once it has recomputed this client's filter; the node-filter
 * panel shows them as "Showing N of M nodes".
 */
import { create } from 'zustand';

export interface NodeFilterCounts {
  /** Nodes streamed to this client; null until the server has answered a filter update. */
  visible: number | null;
  /** Nodes in this client's graph view before scope and filter; null until answered. */
  total: number | null;
}

interface NodeFilterCountState extends NodeFilterCounts {
  setCounts: (visible: number, total: number) => void;
  reset: () => void;
}

export const useNodeFilterCountStore = create<NodeFilterCountState>((set) => ({
  visible: null,
  total: null,
  setCounts: (visible, total) => set({ visible, total }),
  reset: () => set({ visible: null, total: null }),
}));

/**
 * Read the counts from a `filter_update_success` frame's `data`, or null when the frame
 * carries none (a non-negative integer pair is required).
 */
export function parseFilterCounts(data: unknown): { visible: number; total: number } | null {
  if (!data || typeof data !== 'object') return null;
  const d = data as Record<string, unknown>;
  const visible = d.visible_nodes;
  const total = d.total_nodes;
  if (
    typeof visible !== 'number' || typeof total !== 'number' ||
    !Number.isInteger(visible) || !Number.isInteger(total) || visible < 0 || total < 0
  ) {
    return null;
  }
  return { visible, total };
}

/** The panel text for a pair of counts. */
export function formatFilterCounts(counts: NodeFilterCounts): string {
  if (counts.visible === null || counts.total === null) {
    return 'Node count arrives with the next filter update';
  }
  const n = counts.visible.toLocaleString('en-GB');
  const m = counts.total.toLocaleString('en-GB');
  return counts.visible === counts.total ? `Showing all ${m} nodes` : `Showing ${n} of ${m} nodes`;
}
