/**
 * Desktop agent momentum nudge: a per-agent visual offset added to the
 * server position, once per frame, by `DrawnPositions.applyAgents`.
 *
 * - **Working** (a 0x23 beam in the last 15 s): the offset converges on the
 *   target node, so the agent visibly goes to the node it acts on.
 * - **Otherwise** (done, or no work on record): the offset relaxes to zero,
 *   handing the agent back to its server position, the centroid of the
 *   knowledge / ontology / memory triangle plus the server's activity drift
 *   (ADR-2135), which itself returns home as the agent's activity decays. A
 *   first-order lerp, so the return never overshoots or oscillates, and it
 *   reaches a fixed point (the float offset underflows to zero), after which
 *   the agent's drawn position no longer changes.
 *
 * There is no outward drift for a finished agent. The merged layout had one
 * (fade to the edge, 0.005 a frame without end); the layout has been always
 * separated since ADR-2135, so it never ran, and it is deleted rather than
 * kept as a branch that could keep a finished agent moving.
 */

export type Vec3 = readonly [number, number, number];

/** per-frame lerp towards the work target */
export const AGENT_NUDGE_SPEED = 0.03;
/** per-frame relaxation of the offset for an agent that is not working */
export const AGENT_RELAX_SPEED = 0.03;

/**
 * Advance the offset at `off[o3 .. o3+2]` by one frame. `base` is the agent's
 * server position (plus its activity drift), `target` the work target's
 * position (null when unknown) and `state` its work state.
 */
export function stepAgentOffset(
  off: Float32Array,
  o3: number,
  base: Vec3,
  target: Vec3 | null,
  state: 'working' | 'done' | null,
): void {
  if (state === 'working') {
    if (!target) return;
    for (let k = 0; k < 3; k++) off[o3 + k] += (target[k] - base[k] - off[o3 + k]) * AGENT_NUDGE_SPEED;
    return;
  }
  for (let k = 0; k < 3; k++) off[o3 + k] *= 1 - AGENT_RELAX_SPEED;
}
