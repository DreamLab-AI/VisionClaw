/**
 * Shared node-position refs — module-level singleton so both GraphManager (the
 * writer) and BotsVisualization (the reader) access the same live SAB buffer
 * without a React context wrapper.
 */

/** Live SAB position buffer, written by GraphManager's useFrame loop. */
export let sharedNodePositions: Float32Array | null = null;

/** node id → instance index, rebuilt when the node set changes. */
export let sharedNodeIdToIndexMap: Map<string, number> = new Map();

export function setSharedNodePositions(positions: Float32Array | null): void {
  sharedNodePositions = positions;
}

export function setSharedNodeIdToIndexMap(map: Map<string, number>): void {
  sharedNodeIdToIndexMap = map;
}
