/**
 * Where an agent is drawn, for the bots overlay (ADR-2135).
 *
 * The overlay (membrane, nameplate, trail, collaboration edges, payment labels) has no
 * position source of its own. It asks the one drawn-positions array that the gem is drawn
 * from, which already carries the agent's drift, nudge and run to the scene edge. Read it
 * on use, inside the frame that draws: a copy kept elsewhere is a second path.
 */
import type * as THREE from 'three';
import { sharedNodePositions, sharedNodeIdToIndexMap } from '../graph/contexts/NodePositionContext';
import { resolveNodeWorldPosition } from '../visualisation/cameraFocus';

/**
 * Write the agent's drawn world position into `out`.
 *
 * `agentId` is the agent's wire id as a string (`BotsAgent.id`); the raw and the flag-masked
 * forms both resolve, as for the beams. Returns false, leaving `out` untouched, when the
 * agent has no slot in the drawn array: the caller hides it rather than drawing it elsewhere.
 *
 * `idToIndex` and `drawn` default to the live shared ones; tests pass their own.
 */
export function agentDrawnPosition(
  agentId: string,
  out: THREE.Vector3,
  idToIndex: Map<string, number> = sharedNodeIdToIndexMap,
  drawn: Float32Array | null = sharedNodePositions,
): boolean {
  const id = Number(agentId);
  if (!Number.isFinite(id)) return false;
  const p = resolveNodeWorldPosition(id, idToIndex, drawn);
  if (!p) return false;
  out.set(p.x, p.y, p.z);
  return true;
}
