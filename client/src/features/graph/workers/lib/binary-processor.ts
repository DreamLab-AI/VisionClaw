/**
 * Binary position frame processing for the graph worker.
 *
 * Applies a server position frame (always a full snapshot), updates target positions and
 * optional V3 analytics fields, and writes compact output into the caller's
 * pre-allocated positionArray. Zero allocation per call when buffers are reused.
 */
import { getActualNodeId } from '../../../../types/binaryProtocol';
import { workerLogger } from './logger';

export interface BinaryFrameUpdate {
  nodeId: number;
  position: { x: number; y: number; z: number };
  clusterId?: number;
  anomalyScore?: number;
  communityId?: number;
  centrality?: number;
  ssspDistance?: number;
}

/** Per-node analytics stride for the worker analytics buffer (ADR-031 D2). */
export const WORKER_ANALYTICS_STRIDE = 5;

export interface ProcessFrameInput {
  nodeUpdates: BinaryFrameUpdate[];
  /** Target positions array (mutated in place). */
  targetPositions: Float32Array;
  /** Analytics buffer — 5 floats per node: [clusterId, anomalyScore, communityId, centrality, ssspDistance] (ADR-031 D2). */
  analyticsBuffer: Float32Array | null;
  /** Output buffer — 4 floats per update: [nodeId, x, y, z]. Must be pre-allocated. */
  positionArray: Float32Array;
  /** reverseNodeIdMap: numeric wire ID → string node id. */
  reverseNodeIdMap: Map<number, string>;
  /** nodeIndexMap: string node id → array index. */
  nodeIndexMap: Map<string, number>;
  /** Set of pinned node numeric IDs. */
  pinnedNodeIds: Set<number>;
}

export interface ProcessFrameResult {
  unknownCount: number;
  unknownNodeIds: Set<number>;
}

/**
 * Process all node updates from a parsed binary frame.
 * Mutates targetPositions, analyticsBuffer, and positionArray in place.
 * Returns count of unknown node IDs and the set for caller tracking.
 */
export function processFrameUpdates(
  input: ProcessFrameInput,
  existingUnknownIds: Set<number>,
): number {
  const {
    nodeUpdates, targetPositions, analyticsBuffer,
    positionArray, reverseNodeIdMap, nodeIndexMap, pinnedNodeIds,
  } = input;

  let unknownCount = 0;

  for (let index = 0; index < nodeUpdates.length; index++) {
    const update = nodeUpdates[index];
    // Strip flag bits (agent/knowledge/ontology type) from wire ID
    const actualNodeId = getActualNodeId(update.nodeId);
    const stringNodeId = reverseNodeIdMap.get(actualNodeId);

    if (!stringNodeId) {
      existingUnknownIds.add(actualNodeId);
      unknownCount++;
    }

    if (stringNodeId) {
      const nodeIndex = nodeIndexMap.get(stringNodeId);
      if (nodeIndex !== undefined && !pinnedNodeIds.has(actualNodeId)) {
        const i3 = nodeIndex * 3;
        targetPositions[i3]     = update.position.x;
        targetPositions[i3 + 1] = update.position.y;
        targetPositions[i3 + 2] = update.position.z;
        // Store V3 analytics fields per node (stride 5; ADR-031 D2).
        if (analyticsBuffer && update.clusterId !== undefined) {
          const a = nodeIndex * WORKER_ANALYTICS_STRIDE;
          analyticsBuffer[a]     = update.clusterId;
          analyticsBuffer[a + 1] = update.anomalyScore ?? 0;
          analyticsBuffer[a + 2] = update.communityId ?? 0;
          analyticsBuffer[a + 3] = update.centrality ?? 0;
          analyticsBuffer[a + 4] = Number.isFinite(update.ssspDistance) ? (update.ssspDistance as number) : Infinity;
        }
      }
    }

    const arrayOffset = index * 4;
    positionArray[arrayOffset] = actualNodeId;
    positionArray[arrayOffset + 1] = update.position.x;
    positionArray[arrayOffset + 2] = update.position.y;
    positionArray[arrayOffset + 3] = update.position.z;
  }

  return unknownCount;
}

/**
 * Emit a throttled warning when the binary stream contains unknown node IDs.
 * Returns the updated lastAlertTimestamp.
 */
export function warnUnknownNodes(
  unknownCount: number,
  totalTracked: number,
  lastAlertMs: number,
): number {
  if (unknownCount === 0) return lastAlertMs;
  const now = Date.now();
  if (now - lastAlertMs > 5000) {
    workerLogger.warn(
      `Binary stream contains ${unknownCount} unknown node IDs (total tracked: ${totalTracked}). ` +
      `Graph mutation likely occurred — client should re-fetch /api/graph/data.`
    );
    return now;
  }
  return lastAlertMs;
}
