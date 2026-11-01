/**
 * Desktop agent momentum nudge: a per-agent visual offset added to the
 * server position, once per frame, by `DrawnPositions.applyAgents`.
 *
 * - **Working** (a 0x23 beam in the last 15 s): the offset converges on the
 *   target node, so the agent visibly goes to the node it acts on.
 * - **Done**: the agent drifts outward from the centre of the scene to the
 *   scene's edge and stops there (owner decision, 2026-11-01). The edge is the
 *   bounding sphere the camera frames (`sceneFitBounds`), passed in by the
 *   caller. The approach eases (a tenth of the remaining distance a frame,
 *   at most `AGENT_DRIFT_SPEED`), snaps onto the edge when it is closer than
 *   `AGENT_EDGE_SNAP`, and never passes it: an agent beyond the edge
 *   is put back onto it. Once on the edge the offset no longer changes, so the
 *   agent's drawn position is constant and the edge skip gate can skip.
 * - **No work on record**: the offset relaxes to zero, handing the agent back
 *   to its server position (ADR-2135), snapping to zero under
 *   `AGENT_RELAX_SNAP`.
 */

export type Vec3 = readonly [number, number, number];

/** per-frame lerp towards the work target */
export const AGENT_NUDGE_SPEED = 0.03;
/** per-frame outward step cap for a done agent (the long-standing fade-to-edge speed) */
export const AGENT_DRIFT_SPEED = 0.005;
/** fraction of the remaining distance to the edge covered per frame, so the arrival eases */
export const AGENT_EDGE_EASE = 0.1;
/** closer than this to the edge (world units, far under a pixel) the agent snaps onto it */
export const AGENT_EDGE_SNAP = 1e-4;
/** per-frame relaxation of the offset for an agent with no work on record */
export const AGENT_RELAX_SPEED = 0.03;
/**
 * Below this magnitude a relaxing offset snaps to zero. Without it a geometric `* 0.97` keeps
 * changing the float32 drawn position until the offset underflows (about 3 400 frames).
 */
export const AGENT_RELAX_SNAP = 1e-4;

/** The scene's edge: the sphere the camera frames (`sceneFitBounds`). */
export interface SceneEdge {
  centre: Vec3;
  radius: number;
}

/**
 * Advance the offset at `off[o3 .. o3+2]` by one frame. `base` is the agent's
 * server position (plus its activity drift), `target` the work target's
 * position (null when unknown), `state` its work state and `edge` the scene
 * edge (null when unknown: a done agent then holds still).
 */
export function stepAgentOffset(
  off: Float32Array,
  o3: number,
  base: Vec3,
  target: Vec3 | null,
  state: 'working' | 'done' | null,
  edge: SceneEdge | null,
): void {
  if (state === 'working') {
    if (!target) return;
    for (let k = 0; k < 3; k++) off[o3 + k] += (target[k] - base[k] - off[o3 + k]) * AGENT_NUDGE_SPEED;
    return;
  }
  if (state === 'done') {
    if (!edge) return;
    const px = base[0] + off[o3] - edge.centre[0];
    const py = base[1] + off[o3 + 1] - edge.centre[1];
    const pz = base[2] + off[o3 + 2] - edge.centre[2];
    const d = Math.sqrt(px * px + py * py + pz * pz);
    const gap = edge.radius - d; // > 0 inside the edge
    if (Math.abs(gap) < AGENT_EDGE_SNAP && d > 0) return; // on the edge: hold
    if (d === 0) return; // exactly at the centre: no outward direction
    // inside: ease out, at most AGENT_DRIFT_SPEED; outside (the base moved, or the scene shrank):
    // straight back onto the edge, so the agent is never drawn past it
    const step = gap > 0 ? Math.min(AGENT_DRIFT_SPEED, gap * AGENT_EDGE_EASE) : gap;
    const move = gap > 0 && Math.abs(gap - step) < AGENT_EDGE_SNAP ? gap : step;
    off[o3] += (px / d) * move;
    off[o3 + 1] += (py / d) * move;
    off[o3 + 2] += (pz / d) * move;
    return;
  }
  for (let k = 0; k < 3; k++) off[o3 + k] *= 1 - AGENT_RELAX_SPEED;
  const m2 = off[o3] * off[o3] + off[o3 + 1] * off[o3 + 1] + off[o3 + 2] * off[o3 + 2];
  if (m2 < AGENT_RELAX_SNAP * AGENT_RELAX_SNAP) {
    off[o3] = 0;
    off[o3 + 1] = 0;
    off[o3 + 2] = 0;
  }
}
