/**
 * Draw-range writes for the agent line layers (AgentTrail, AgentCollabEdges).
 *
 * A line whose draw range is too short to form a segment is hidden rather
 * than drawn: WebGPURenderer would otherwise issue `Draw(0)` (Dawn warns
 * "Draw with a vertex count of 0 is unusual") for every empty trail or edge
 * layer, and a one-vertex strip draws nothing anyway.
 */
import * as THREE from 'three';

/** Set `line`'s draw range to `vertexCount` and show it only if that forms a segment. */
export function setLineDrawCount(line: THREE.Line, vertexCount: number): void {
  line.geometry.setDrawRange(0, vertexCount);
  line.visible = vertexCount >= 2;
}
