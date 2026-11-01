/**
 * Desktop agent momentum nudge: a per-agent visual offset added to the
 * marker's hover point, once per frame, by the marker pass (`features/bots/agentMarkerPass.ts`, ADR-2142 / ADR-2135).
 *
 * - **Not done**: no offset. An agent that works sits at its marker's base (beside its target
 *   node, or on the centre ring) and the work beam shows the node it acts on; nothing pulls it
 *   anywhere.
 * - **Done**: from the moment it finishes, the agent moves outward from the centre of the
 *   scene to the scene's edge in `AGENT_EDGE_SECONDS` (30 s) and stops there (owner decision,
 *   2026-11-01). The edge is the bounding sphere the camera frames (`sceneFitBounds`), passed in
 *   by the caller. The motion is a function of elapsed time, not of frames: its distance from
 *   the centre is `r0 + (edge - r0) * easeOutCubic(elapsed / 30 s)`, `r0` being its distance
 *   when it finished, so it takes the same 30 s at 60 or 120 fps, decelerates as it arrives,
 *   never passes the edge (an agent beyond the edge is put back onto it) and, once on it, no
 *   longer changes: the drawn position is constant and the edge skip gate can skip. Going back
 *   to work, or having no work on record, restarts the clock for its next finish.
 *   The headset runs the same rule in `xr-client/scripts/agent_choreography.gd` (`EDGE_SECONDS`, `ease_out_cubic`,
 *   its edge the overlay volume's box); ADR-2135 names the one definition, and `test_agent_edge_run.gd` fails
 *   if `AGENT_EDGE_SECONDS` here and `EDGE_SECONDS` there differ.
 *   The edge hold is final: the agent stays on the edge until the registry retires it (the server
 *   sends `retired` when the agent leaves the roster); nothing here ends the hold.
 * - **Not done** (working, or no state): any offset relaxes to zero, snapping to zero under
 *   `AGENT_RELAX_SNAP`.
 */

export type Vec3 = readonly [number, number, number];

/** seconds from finishing to arriving at the scene's edge */
export const AGENT_EDGE_SECONDS = 30;
/** once the 30 s are up, closer than this to the edge (world units, far under a pixel) the agent holds */
export const AGENT_EDGE_SNAP = 1e-4;
/** per-frame relaxation of the offset of an agent that is not done */
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

/** When an agent finished and how far from the scene centre it was then (`since < 0`: not finished). */
export interface AgentFinish {
  since: number;
  r0: number;
}

export function newAgentFinish(): AgentFinish {
  return { since: -1, r0: 0 };
}

/**
 * Advance the offset at `off[o3 .. o3+2]` by one frame. `base` is the agent's
 * marker base (plus its activity drift), `state` `'done'` or null, `edge` the scene
 * edge (null when unknown: a done agent then holds still), `finish` the
 * agent's finish clock and `nowMs` the frame's time in milliseconds.
 */
export function stepAgentOffset(
  off: Float32Array,
  o3: number,
  base: Vec3,
  state: 'done' | null,
  edge: SceneEdge | null,
  finish: AgentFinish,
  nowMs: number,
): void {
  if (state !== 'done') finish.since = -1;
  if (state === 'done') {
    if (!edge) return;
    const px = base[0] + off[o3] - edge.centre[0];
    const py = base[1] + off[o3 + 1] - edge.centre[1];
    const pz = base[2] + off[o3 + 2] - edge.centre[2];
    const d = Math.sqrt(px * px + py * py + pz * pz);
    if (finish.since < 0) {
      finish.since = nowMs;
      finish.r0 = d;
    }
    if (d === 0) return; // exactly at the centre: no outward direction
    const u = Math.min(Math.max((nowMs - finish.since) / (AGENT_EDGE_SECONDS * 1000), 0), 1);
    const eased = 1 - (1 - u) * (1 - u) * (1 - u);
    // beyond the edge when it finished (or the scene shrank): straight onto it, never further out
    const want = finish.r0 >= edge.radius ? edge.radius : finish.r0 + (edge.radius - finish.r0) * eased;
    const move = want - d;
    if (u >= 1 && Math.abs(edge.radius - d) < AGENT_EDGE_SNAP) return; // on the edge: hold
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
