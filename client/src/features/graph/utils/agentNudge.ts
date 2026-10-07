/**
 * Desktop agent momentum nudge: a per-agent visual offset added to the
 * server position in the GemNodes render loop.
 *
 * - **Working** (a 0x23 beam in the last 15 s): the offset converges on the
 *   target node, so the agent visibly goes to the node it acts on.
 * - **Done / idle, merged layout**: the offset is kept and drifts slowly
 *   outward from the graph centre (fade-to-edge), the long-standing look.
 * - **Done / idle, separated layout (ADR-2135)**: the offset relaxes to zero,
 *   handing the agent back to its server position — the centroid of the
 *   knowledge / ontology / memory triangle plus the server's activity drift,
 *   which itself returns home as the agent's activity decays. A first-order
 *   lerp, so the return never overshoots or oscillates.
 */

export type Vec3 = readonly [number, number, number];

/** per-frame lerp towards the work target */
export const AGENT_NUDGE_SPEED = 0.03;
/** per-frame outward step for done agents in the merged layout */
export const AGENT_DRIFT_SPEED = 0.005;
/** per-frame relaxation of the offset for done agents in the separated layout */
export const AGENT_RELAX_SPEED = 0.03;

/**
 * Advance the offset at `off[o3 .. o3+2]` by one frame. `base` is the agent's
 * server position, `target` the work target's position (null when unknown),
 * `centre` the graph centre and `separated` whether Graph Separation > 0.
 */
export function stepAgentOffset(
  off: Float32Array,
  o3: number,
  base: Vec3,
  target: Vec3 | null,
  state: 'working' | 'done' | null,
  centre: Vec3,
  separated: boolean,
): void {
  if (state === 'working') {
    if (!target) return;
    for (let k = 0; k < 3; k++) off[o3 + k] += (target[k] - base[k] - off[o3 + k]) * AGENT_NUDGE_SPEED;
    return;
  }
  if (separated) {
    for (let k = 0; k < 3; k++) off[o3 + k] *= 1 - AGENT_RELAX_SPEED;
    return;
  }
  if (state === 'done') {
    const cx = base[0] + off[o3] - centre[0];
    const cy = base[1] + off[o3 + 1] - centre[1];
    const cz = base[2] + off[o3 + 2] - centre[2];
    const d = Math.sqrt(cx * cx + cy * cy + cz * cz) || 1;
    off[o3] += (cx / d) * AGENT_DRIFT_SPEED;
    off[o3 + 1] += (cy / d) * AGENT_DRIFT_SPEED;
    off[o3 + 2] += (cz / d) * AGENT_DRIFT_SPEED;
  }
}
