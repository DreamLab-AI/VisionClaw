/**
 * agentVisualConstants.ts
 * Single source of truth for the state→visual mappings shared across every agent
 * renderer: the instanced agent capsules (GemNodes metadata pack +
 * AgentCapsuleMaterial shader) and the agent overlay (BotsDataContext). These
 * are the two consolidated agent layers since AgentNodesLayer was retired.
 *
 * Centralising these here removes the byte-identical health→glow ramps the agent
 * renderers each carried, and gives the capsule shader a canonical status→activity
 * scalar so an idle swarm visibly rests while an active one visibly works.
 *
 * Identifiers keep the codebase's American `color` spelling for consistency with
 * the surrounding Three.js API and helpers; prose stays UK English.
 */

/** Canonical agent status enum — mirrors BotsAgent['status'] in BotsTypes.ts. */
export type AgentStatus =
  | 'idle'
  | 'busy'
  | 'active'
  | 'error'
  | 'initializing'
  | 'terminating'
  | 'offline';

/** Canonical status→base colour for all agent renderers. */
export const AGENT_STATUS_COLORS: Record<AgentStatus, string> = {
  active:       '#2ECC71',
  busy:         '#F39C12',
  idle:         '#95A5A6',
  error:        '#E74C3C',
  initializing: '#3498DB',
  terminating:  '#9B59B6',
  offline:      '#607D8B',
};

/** Muted grey used when a status string is missing or unrecognised. */
export const AGENT_STATUS_COLOR_FALLBACK = '#95A5A6';

/** Resolve a status string (possibly undefined / off-enum) to a base colour. */
export const agentStatusColor = (status: string | undefined): string =>
  AGENT_STATUS_COLORS[status as AgentStatus] ?? AGENT_STATUS_COLOR_FALLBACK;

/**
 * Four configurable health→glow stops (ported from the legacy BotsControlPanel
 * palette). They drive four of the six ramp tiers; the two intermediate tiers
 * (#F1C40F at ≥65, #E67E22 at ≥25) stay fixed. Optional per-field so a partial
 * override (e.g. only `critical`) leaves the rest on their canonical defaults.
 */
export interface HealthColorBands {
  excellent?: string;
  good?: string;
  warning?: string;
  critical?: string;
}

/**
 * Canonical stop colours. Chosen so `healthGlowColor` with no override reproduces
 * the historical six-tier ramp exactly: ≥95 excellent, ≥80 good, ≥50 warning,
 * <25 critical.
 */
export const DEFAULT_HEALTH_COLORS: Required<HealthColorBands> = {
  excellent: '#00FF00',
  good: '#2ECC71',
  warning: '#F39C12',
  critical: '#E74C3C',
};

/**
 * Six-tier health→glow colour (bioluminescent membrane hue). Four of the tiers are
 * user-configurable via `colors` (control-centre Agents → Health); the two
 * intermediate tiers stay fixed. Omitting `colors` (or any field) preserves the
 * exact ramp the agent renderers previously duplicated.
 */
export const healthGlowColor = (health: number, colors?: HealthColorBands): string => {
  const excellent = colors?.excellent ?? DEFAULT_HEALTH_COLORS.excellent;
  const good = colors?.good ?? DEFAULT_HEALTH_COLORS.good;
  const warning = colors?.warning ?? DEFAULT_HEALTH_COLORS.warning;
  const critical = colors?.critical ?? DEFAULT_HEALTH_COLORS.critical;
  if (health >= 95) return excellent;
  if (health >= 80) return good;
  if (health >= 65) return '#F1C40F';
  if (health >= 50) return warning;
  if (health >= 25) return '#E67E22';
  return critical;
};

/**
 * Status→activity scalar (0-1) packed into the agent capsule metadata channel
 * meta.w (see GemNodes metadata pack and AgentCapsuleMaterial). Drives shader
 * pulse speed and emissive brightness. Values are chosen so a NearestFilter float
 * texel isolates the error band (~0.9) cleanly from active/busy (1.0):
 *   idle rests, initializing/terminating idle-toward-work, error flickers, active
 *   works.
 */
export const AGENT_STATUS_ACTIVITY: Record<AgentStatus, number> = {
  idle:         0.15,
  busy:         1.0,
  active:       1.0,
  error:        0.9,
  initializing: 0.5,
  terminating:  0.3,
  offline:      0.0,
};

/** Resolve a status string to its packed activity scalar (idle default). */
export const agentStatusActivity = (status: string | undefined): number =>
  AGENT_STATUS_ACTIVITY[status as AgentStatus] ?? AGENT_STATUS_ACTIVITY.idle;
