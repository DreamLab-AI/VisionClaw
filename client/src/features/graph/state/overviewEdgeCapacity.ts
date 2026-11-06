/**
 * Overview edge capacity (ADR-2181 D4): `visualisation.graphView.overviewEdges.capacity`.
 *
 * `'auto'` sizes the buffer from the data: the edge count rounded up to a power of
 * two. A number is a GPU memory budget in edges. Settings controls may write the
 * number as a string, so any numeric string is accepted; anything else is 'auto'.
 */
export interface OverviewEdgeCapacity {
  /** Buffer size to allocate (a power of two, at least 1). */
  capacity: number;
  /** Edges actually drawn: min(edgeCount, capacity). */
  drawn: number;
  /** True when edges were dropped: the caller must show "N of M edges drawn" in-canvas. */
  truncated: boolean;
}

function nextPow2(n: number): number {
  return n <= 1 ? 1 : 2 ** Math.ceil(Math.log2(n));
}

/** Normalise the stored setting to `'auto'` or a positive integer budget. */
export function normaliseEdgeCapacity(value: unknown): 'auto' | number {
  const n = typeof value === 'number' ? value : typeof value === 'string' && value.trim() !== '' ? Number(value) : NaN;
  return Number.isFinite(n) && n >= 1 ? Math.floor(n) : 'auto';
}

export function resolveOverviewEdgeCapacity(setting: unknown, edgeCount: number): OverviewEdgeCapacity {
  const want = normaliseEdgeCapacity(setting);
  const count = Math.max(0, Math.floor(edgeCount));
  const capacity = want === 'auto' ? nextPow2(count) : Math.min(want, nextPow2(count));
  const drawn = Math.min(count, capacity);
  return { capacity, drawn, truncated: drawn < count };
}
