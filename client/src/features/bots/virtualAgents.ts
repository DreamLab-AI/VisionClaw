/**
 * Agent embodiment constants and pure helpers (ADR-2142, ADR-2135).
 *
 * No agent is a graph node. Every agent, demo or real, reaches the desktop as `0x23` frames, is a
 * registry entry (`agentTargetStore`) and is drawn as one marker hovering beside its target; the marker
 * pass (`agentMarkerPass.ts`) is the only writer of that position, which is also the beam source. Presence
 * follows the evidence: an agent stays while its frames keep arriving and fades 30 s after the last one
 * (the headset registry's TTL), unless a `retired` state frame removes it at once.
 *
 * The six shared-demo agents keep their reserved ids (agent flag | 0xD001..0xD006) and their role colours;
 * real agents live in 0x8200_0000..=0x83FF_FFFF. Nothing else distinguishes them.
 */
import { generateAgentTypeColor } from './components/BotsShared';

/** Evidence time-to-live, matching the XR agent registry. Longer than the server keep-alive (5 s)
 *  and the longest rest between jobs (26 s), so a resting agent stays on screen. */
export const VIRTUAL_AGENT_TTL_MS = 30_000;

const AGENT_NODE_FLAG = 0x80000000;
const NODE_ID_MASK = 0x03ffffff;

/**
 * The registry key of an agent id: the agent flag plus the 26-bit id, as an unsigned number. The wire
 * carries it flagged; `/bots/data` ids and edge ends carry it masked. Every join goes through here.
 */
export function agentWireId(id: number | string): number {
  return (AGENT_NODE_FLAG | (Number(id) & NODE_ID_MASK)) >>> 0;
}

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

/**
 * Whether `id` (with or without the agent flag and class bits) is in the reserved demo range. Only the
 * demo-specific behaviour uses this (`clearVirtual` when the demo stops, the role colours); the
 * registry, the marker pass and the beams treat every agent alike.
 */
export function isVirtualAgentId(id: number): boolean {
  const masked = (id >>> 0) & NODE_ID_MASK;
  return masked >= 0xd001 && masked <= 0xd0ff;
}

const GOLDEN = 0.6180339887498949;

/** The agent's angle on its hover ring (radians), a multiplicative hash of its id: stable, and
 *  consecutive ids (the six demo roles) land at least ~32 degrees apart. */
export function ringAngle(agentId: number): number {
  const slot = (agentId >>> 0) & NODE_ID_MASK;
  return ((slot * GOLDEN) % 1) * 2 * Math.PI;
}

/**
 * A stable point around an anchor (its target node, or the scene centre) for this agent to hover at: on a
 * ring of `radius` in the horizontal plane at height `lift`, at the angle its id hashes to, so agents
 * sharing a node spread out instead of stacking, and never move between frames.
 */
export function agentHoverOffset(
  agentId: number,
  radius: number,
  lift: number,
): { x: number; y: number; z: number } {
  const angle = ringAngle(agentId);
  return { x: Math.cos(angle) * radius, y: lift, z: Math.sin(angle) * radius };
}

/** Whether the agent has been heard from within the TTL. */
export function livePresence(agentId: number, seenAt: ReadonlyMap<number, number>, now: number): boolean {
  const t = seenAt.get(agentId);
  return t !== undefined && now - t < VIRTUAL_AGENT_TTL_MS;
}

/** Where an agent hovers: this far from the node it works on, this high above it. */
export const VIRTUAL_HOVER_RADIUS = 14;
export const VIRTUAL_HOVER_LIFT = 8;

const FALLBACK_COLOURS = ['#4fc3f7', '#ffd54f', '#81c784', '#ba68c8', '#ff8a65', '#4db6ac', '#f06292', '#9575cd'];

/**
 * The marker colour: a demo role keeps its own, a real agent takes its type's colour when `/bots/data`
 * knows the type (`typeColour` lets the caller apply the user's palette), otherwise a stable colour from
 * its id.
 */
export function agentMarkerColour(
  wireId: number,
  type?: string,
  typeColour: (t: string) => string = generateAgentTypeColor,
): string {
  const masked = (wireId >>> 0) & NODE_ID_MASK;
  const role = VIRTUAL_ROLES.find(r => r.suffix === masked);
  if (role) return role.colour;
  if (type) return typeColour(type);
  return FALLBACK_COLOURS[Math.floor(((masked * GOLDEN) % 1) * FALLBACK_COLOURS.length)];
}

/** The role label of a demo agent, when it is one. */
export function demoRoleLabel(wireId: number): string | undefined {
  const masked = (wireId >>> 0) & NODE_ID_MASK;
  return VIRTUAL_ROLES.find(r => r.suffix === masked)?.label;
}
