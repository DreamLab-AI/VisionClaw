/**
 * Virtual agents (ADR-2142): the six agents of the shared demo have no graph node.
 *
 * The server's agent-demo director sends ordinary `0x23` action frames that name a reserved
 * source id (agent flag | 0xD001..0xD006). A real agent is a graph node a beam can start from;
 * these are not, so each is drawn as a marker hovering beside the node it is working on, exactly
 * as the headset does. Presence follows the evidence: an agent stays while its actions keep
 * arriving and fades 30 s after the last one (the same TTL the headset's registry uses), so a
 * stopped demo clears itself even before the status poll notices.
 */

/** Evidence time-to-live, matching the XR agent registry. Longer than the server keep-alive (5 s)
 *  and the longest rest between jobs (26 s), so a resting agent stays on screen. */
export const VIRTUAL_AGENT_TTL_MS = 30_000;

const AGENT_NODE_FLAG = 0x80000000;

export interface VirtualRole {
  suffix: number;
  label: string;
  /** CSS colour of the marker. */
  colour: string;
}

/** Same order and labels as the server's `ROLES`. */
export const VIRTUAL_ROLES: readonly VirtualRole[] = [
  { suffix: 0xd001, label: 'Architect', colour: '#4fc3f7' },
  { suffix: 0xd002, label: 'Analyst', colour: '#ffd54f' },
  { suffix: 0xd003, label: 'Coder', colour: '#81c784' },
  { suffix: 0xd004, label: 'Reviewer', colour: '#ba68c8' },
  { suffix: 0xd005, label: 'Tester', colour: '#ff8a65' },
  { suffix: 0xd006, label: 'Optimizer', colour: '#4db6ac' },
];

/** The wire id of a role (agent flag set), as a non-negative number. */
export const virtualWireId = (suffix: number): number => (AGENT_NODE_FLAG | suffix) >>> 0;

/** Whether `id` (with or without the agent flag and class bits) is in the reserved demo range. */
export function isVirtualAgentId(id: number): boolean {
  const masked = (id >>> 0) & 0x03ffffff;
  return masked >= 0xd001 && masked <= 0xd0ff;
}

/**
 * A stable point around a target node for this agent to hover at: on a ring of `radius` in the
 * horizontal plane, at height `lift`, at an angle derived from the agent's id so the six agents
 * working one node spread out instead of stacking, and never move between frames.
 */
export function virtualAgentHoverOffset(
  agentId: number,
  radius: number,
  lift: number,
): { x: number; y: number; z: number } {
  const slot = ((agentId >>> 0) & 0x03ffffff) - 0xd001;
  const angle = (slot * 2 * Math.PI) / 6 + 0.35;
  return { x: Math.cos(angle) * radius, y: lift, z: Math.sin(angle) * radius };
}

/** Whether the agent has been heard from within the TTL. */
export function livePresence(agentId: number, seenAt: ReadonlyMap<number, number>, now: number): boolean {
  const t = seenAt.get(agentId);
  return t !== undefined && now - t < VIRTUAL_AGENT_TTL_MS;
}

/** Where a node-less agent hovers: this far from the node it works on, this high above it. */
export const VIRTUAL_HOVER_RADIUS = 14;
export const VIRTUAL_HOVER_LIFT = 8;

interface Settable {
  set(x: number, y: number, z: number): unknown;
}

/**
 * Resolve a virtual agent to a world position: beside the node it was last seen working on, while
 * it is present. `resolveNode` is the viewer's own node-position lookup, so the agent moves with
 * the node as the layout moves. Returns false (leaving `out` untouched) when the agent is absent,
 * expired or its target is not on screen.
 */
export function resolveVirtualAgentPosition(
  agentId: number,
  out: Settable,
  state: { targets: ReadonlyMap<number, number>; seenAt: ReadonlyMap<number, number> },
  resolveNode: (nodeId: number, out: { x: number; y: number; z: number }) => boolean,
  now: number,
): boolean {
  if (!livePresence(agentId, state.seenAt, now)) return false;
  const target = state.targets.get(agentId);
  if (target === undefined) return false;
  const p = { x: 0, y: 0, z: 0 };
  if (!resolveNode(target, p)) return false;
  const off = virtualAgentHoverOffset(agentId, VIRTUAL_HOVER_RADIUS, VIRTUAL_HOVER_LIFT);
  out.set(p.x + off.x, p.y + off.y, p.z + off.z);
  return true;
}
