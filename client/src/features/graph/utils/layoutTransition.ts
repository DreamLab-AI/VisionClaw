/**
 * Layout-mode transition: a mass-aware lerp from the old to the new positions.
 * Progress is divided by `transitionMassFactor`, so better-connected nodes
 * reach their target sooner and the leaves follow. Positions are indexed by
 * render slot (x, y, z at slot * 3); degrees are keyed by node id.
 */

/** Below 1 for a connected node: its progress runs 1 / factor times faster. */
export function transitionMassFactor(connectionCount: number): number {
  return 1.0 / (1.0 + Math.sqrt(connectionCount) * 0.3);
}

/**
 * One frame of the transition: writes `drawn` for every render slot below
 * `slots` at eased `progress` (0..1). Each slot's degree is looked up by the
 * id of the node in that slot (`nodeIdToIndexMap`); an empty slot has none.
 */
export function lerpLayoutTransition(
  drawn: Float32Array,
  start: ArrayLike<number>,
  target: ArrayLike<number>,
  progress: number,
  slots: number,
  connectionCountMap: Map<string, number>,
  nodes: ReadonlyArray<{ id: string | number }>,
  nodeIdToIndexMap: Map<string, number>,
): void {
  const degree = new Float32Array(slots);
  for (const n of nodes) {
    const id = String(n.id);
    const slot = nodeIdToIndexMap.get(id);
    if (slot !== undefined && slot >= 0 && slot < slots) degree[slot] = connectionCountMap.get(id) || 0;
  }
  for (let i = 0; i < slots; i++) {
    const idx = i * 3;
    if (idx + 2 >= drawn.length) break;
    const np = Math.min(progress / transitionMassFactor(degree[i]), 1.0);
    drawn[idx] = start[idx] + (target[idx] - start[idx]) * np;
    drawn[idx + 1] = start[idx + 1] + (target[idx + 1] - start[idx + 1]) * np;
    drawn[idx + 2] = start[idx + 2] + (target[idx + 2] - start[idx + 2]) * np;
  }
}
