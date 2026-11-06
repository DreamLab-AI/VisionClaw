/**
 * SAB / typed-array buffer management helpers for the graph worker.
 *
 * Handles reallocation after node removal while preserving existing position data.
 * All operations write into pre-allocated Float32Arrays — no heap allocation in steady state.
 */
import { GraphData } from './types';

export interface PositionBuffers {
  currentPositions: Float32Array;
  targetPositions: Float32Array;
  velocities: Float32Array;
}

/**
 * Rebuild position/velocity buffers after a node has been removed from graphData.
 * Old positions are compacted into new arrays according to the post-removal node order.
 * Also rebuilds nodeIndexMap and nodeIdCache to match the new node ordering.
 *
 * @param graphData       Updated graphData (node already removed from .nodes array)
 * @param oldBuffers      The pre-removal position/velocity buffers
 * @param oldIndexMap     nodeIndexMap snapshot captured before removal
 * @param nodeIndexMap    Caller's live nodeIndexMap (cleared and repopulated)
 * @param nodeIdCache     Caller's live nodeIdCache (resized and repopulated)
 * @returns New compacted PositionBuffers
 */
export function reallocateAfterRemoval(
  graphData: GraphData,
  oldBuffers: PositionBuffers,
  oldIndexMap: Map<string, number>,
  nodeIndexMap: Map<string, number>,
  nodeIdCache: string[],
): PositionBuffers {
  const nodeCount = graphData.nodes.length;
  const newCurrent = new Float32Array(nodeCount * 3);
  const newTarget  = new Float32Array(nodeCount * 3);
  const newVel     = new Float32Array(nodeCount * 3);

  nodeIndexMap.clear();
  nodeIdCache.length = nodeCount;

  graphData.nodes.forEach((node, newIndex) => {
    nodeIndexMap.set(node.id, newIndex);
    nodeIdCache[newIndex] = node.id;
    const oldIndex = oldIndexMap.get(node.id);
    if (oldIndex !== undefined) {
      for (let k = 0; k < 3; ++k) {
        newCurrent[newIndex * 3 + k] = oldBuffers.currentPositions[oldIndex * 3 + k];
        newTarget[newIndex * 3 + k]  = oldBuffers.targetPositions[oldIndex * 3 + k];
        newVel[newIndex * 3 + k]     = oldBuffers.velocities[oldIndex * 3 + k];
      }
    }
  });

  return { currentPositions: newCurrent, targetPositions: newTarget, velocities: newVel };
}

export interface InitBuffersResult {
  currentPositions: Float32Array;
  targetPositions: Float32Array;
  velocities: Float32Array;
  preservedCount: number;
}

/**
 * Allocate fresh position/velocity buffers for a new graph topology.
 * Preserves positions for nodes that existed in the previous topology.
 * New nodes at the origin are scattered onto a Fibonacci sphere to avoid pile-up.
 */
export function initPositionBuffers(
  nodes: GraphData['nodes'],
  oldCurrentPos: Float32Array | null,
  oldTargetPos: Float32Array | null,
  oldNodeIndexMap: Map<string, number>,
): InitBuffersResult {
  const nodeCount = nodes.length;
  const newCurrent = new Float32Array(nodeCount * 3);
  const newTarget  = new Float32Array(nodeCount * 3);
  const newVel     = new Float32Array(nodeCount * 3);
  let preservedCount = 0;

  for (let index = 0; index < nodeCount; index++) {
    const node = nodes[index];
    const i3 = index * 3;
    const oldIndex = oldNodeIndexMap.get(String(node.id));

    if (oldIndex !== undefined && oldCurrentPos && oldCurrentPos.length > oldIndex * 3 + 2) {
      const oi3 = oldIndex * 3;
      newCurrent[i3]     = oldCurrentPos[oi3];
      newCurrent[i3 + 1] = oldCurrentPos[oi3 + 1];
      newCurrent[i3 + 2] = oldCurrentPos[oi3 + 2];
      if (oldTargetPos && oldTargetPos.length > oi3 + 2) {
        newTarget[i3]     = oldTargetPos[oi3];
        newTarget[i3 + 1] = oldTargetPos[oi3 + 1];
        newTarget[i3 + 2] = oldTargetPos[oi3 + 2];
      } else {
        newTarget[i3]     = newCurrent[i3];
        newTarget[i3 + 1] = newCurrent[i3 + 1];
        newTarget[i3 + 2] = newCurrent[i3 + 2];
      }
      preservedCount++;
    } else {
      const pos = node.position || (node as unknown as { x?: number; y?: number; z?: number });
      let px = Number(pos.x) || 0;
      let py = Number(pos.y) || 0;
      let pz = Number(pos.z) || 0;
      // Fibonacci sphere: scatter nodes at origin to prevent pile-up
      if (px === 0 && py === 0 && pz === 0) {
        const ga = Math.PI * (3 - Math.sqrt(5));
        const tt = 1 - (index / Math.max(nodeCount, 1)) * 2;
        const rr = Math.sqrt(1 - tt * tt);
        const spread = 15;
        px = Math.cos(index * ga) * rr * spread;
        py = tt * spread;
        pz = Math.sin(index * ga) * rr * spread;
      }
      newCurrent[i3] = px; newCurrent[i3 + 1] = py; newCurrent[i3 + 2] = pz;
      newTarget[i3]  = px; newTarget[i3 + 1]  = py; newTarget[i3 + 2]  = pz;
    }
  }

  return { currentPositions: newCurrent, targetPositions: newTarget, velocities: newVel, preservedCount };
}

/**
 * Copy currentPositions into a SharedArrayBuffer view (main-thread read path).
 * Returns how many nodes did NOT fit (0 when the view holds them all), so a
 * short buffer is never silent (ADR-2181 D4): the main thread grows the SAB
 * from the delivered node count (graphWorkerProxy.ensureSharedCapacity) and
 * reports any shortfall it could not cover in the canvas notice.
 */
export function syncToSharedBuffer(
  positionView: Float32Array | null,
  currentPositions: Float32Array | null,
): number {
  if (positionView && currentPositions) {
    const len = Math.min(currentPositions.length, positionView.length);
    positionView.set(currentPositions.subarray(0, len));
    return Math.max(0, Math.ceil((currentPositions.length - len) / 3));
  }
  return 0;
}
