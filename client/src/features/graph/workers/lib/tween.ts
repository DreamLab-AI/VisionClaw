/**
 * Client-side tweening (interpolation) toward server-authoritative target positions.
 *
 * The server (Rust/CUDA GPU) is the single source of truth for node positions.
 * This module provides a pure function that advances current positions one step
 * toward their targets. It writes directly into the caller's typed arrays —
 * zero allocation per tick.
 */
import { TweenSettings } from './types';

export interface TweenInput {
  /** Current (interpolated) positions — mutated in place. Layout: [x0,y0,z0, x1,y1,z1, ...] */
  curPos: Float32Array;
  /** Server-authoritative target positions. Layout matches curPos. */
  tgtPos: Float32Array;
  /** Velocity buffer. Layout matches curPos. Zeroed on snap. */
  vel: Float32Array;
  /** Number of nodes to process. */
  nodeCount: number;
  /** Set of numeric node IDs that are pinned (skip interpolation). */
  pinnedNodeIds: Set<number>;
  /** nodeIdMap: string node id → numeric id (for pinning check). */
  nodeIdMap: Map<string, number>;
  /** graphData node id list (parallel to position arrays). */
  nodeIds: string[];
  /** Tweening configuration. */
  tweenSettings: TweenSettings;
  /** Delta time in seconds (clamped to max 0.033 by caller). */
  deltaTime: number;
}

export interface TweenResult {
  /** Total movement (sum of displacement magnitudes) this tick. */
  totalMovement: number;
  /** True if any node was more than 0.001 units from its target at tick start. */
  hadMovement: boolean;
}

/**
 * Advance all node positions one interpolation step toward their server targets.
 * Mutates curPos and vel in place. Returns movement metrics.
 *
 * Hot path: no allocations, typed-array index arithmetic only.
 */
export function tickTween(input: TweenInput): TweenResult {
  const { curPos, tgtPos, vel, nodeCount, pinnedNodeIds, nodeIdMap, nodeIds, tweenSettings, deltaTime } = input;

  // Early-exit check: is any node still moving?
  let hadMovement = false;
  for (let i = 0; i < nodeCount && !hadMovement; i++) {
    const i3 = i * 3;
    if (
      Math.abs(tgtPos[i3]     - curPos[i3])     > 0.001 ||
      Math.abs(tgtPos[i3 + 1] - curPos[i3 + 1]) > 0.001 ||
      Math.abs(tgtPos[i3 + 2] - curPos[i3 + 2]) > 0.001
    ) {
      hadMovement = true;
    }
  }

  if (!hadMovement) {
    return { totalMovement: 0, hadMovement: false };
  }

  // Exponential approach with time constant `responseMs`: a node covers 63% of the remaining
  // distance in one response time, whatever the frame rate. LOWER = FASTER. (The earlier
  // `lerpBase` form, 1 - base^dt, had a comment saying the opposite of what it did.)
  const lerpFactor = 1 - Math.exp(-(deltaTime * 1000) / Math.max(tweenSettings.responseMs, 1));
  const snapThreshold = tweenSettings.snapThreshold;
  const maxDiv = tweenSettings.maxDivergence;

  let totalMovement = 0;

  for (let i = 0; i < nodeCount; i++) {
    const i3 = i * 3;

    // Skip pinned nodes
    const nodeId = nodeIdMap.get(nodeIds[i]);
    if (nodeId !== undefined && pinnedNodeIds.has(nodeId)) {
      continue;
    }

    const dx = tgtPos[i3]     - curPos[i3];
    const dy = tgtPos[i3 + 1] - curPos[i3 + 1];
    const dz = tgtPos[i3 + 2] - curPos[i3 + 2];
    const distanceSq = dx * dx + dy * dy + dz * dz;

    // Force snap when divergence exceeds maxDivergence (prevents runaway drift
    // on topology changes where new target positions differ greatly)
    if (distanceSq > maxDiv * maxDiv) {
      curPos[i3]     = tgtPos[i3];
      curPos[i3 + 1] = tgtPos[i3 + 1];
      curPos[i3 + 2] = tgtPos[i3 + 2];
      vel[i3] = 0; vel[i3 + 1] = 0; vel[i3 + 2] = 0;
      totalMovement += Math.sqrt(distanceSq);
    } else if (distanceSq < snapThreshold * snapThreshold) {
      // Sub-pixel distance: snap to avoid floating drift
      const positionChanged =
        Math.abs(curPos[i3]     - tgtPos[i3])     > 0.01 ||
        Math.abs(curPos[i3 + 1] - tgtPos[i3 + 1]) > 0.01 ||
        Math.abs(curPos[i3 + 2] - tgtPos[i3 + 2]) > 0.01;
      if (positionChanged) {
        totalMovement += Math.sqrt(distanceSq);
        curPos[i3]     = tgtPos[i3];
        curPos[i3 + 1] = tgtPos[i3 + 1];
        curPos[i3 + 2] = tgtPos[i3 + 2];
      }
      vel[i3] = 0; vel[i3 + 1] = 0; vel[i3 + 2] = 0;
    } else {
      // Normal lerp step
      const moveX = dx * lerpFactor;
      const moveY = dy * lerpFactor;
      const moveZ = dz * lerpFactor;
      totalMovement += Math.sqrt(moveX * moveX + moveY * moveY + moveZ * moveZ);
      curPos[i3]     += moveX;
      curPos[i3 + 1] += moveY;
      curPos[i3 + 2] += moveZ;
    }
  }

  return { totalMovement, hadMovement: true };
}
