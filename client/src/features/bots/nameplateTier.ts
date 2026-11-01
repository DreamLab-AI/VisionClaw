/**
 * Agent nameplate level of detail (W3D), kept from the retired BotsNode; used by AgentNameplate.
 *
 * Nameplate level-of-detail tiers (W3D). A 3-line HTML nameplate per agent
 * overlaps illegibly once ~19 agents cluster, so each agent's label is gated to
 * one of three tiers by camera distance:
 *   - 'full'   3-line nameplate (name / type / status|health)
 *   - 'name'   single name line
 *   - 'hidden' no nameplate
 */
export type NameplateTier = 'full' | 'name' | 'hidden';

export interface NameplateTierOpts {
  /** D1 — camera distance under which the full nameplate shows. */
  fullDistance: number;
  /** D2 — camera distance under which the name-only line shows; hidden beyond it. */
  nameDistance: number;
  /** Fractional dead-band applied to BOTH boundaries (0.1 = ±10%). */
  hysteresis: number;
  /** Pin to 'full' regardless of distance (hovered / selected / queen). */
  forceFull?: boolean;
  /** Screen-density guard: cap a would-be 'full' tier at 'name' (never overrides
   *  forceFull). Set when too many agents crowd the near field. */
  capName?: boolean;
}

/**
 * Pure distance→tier classifier with directional hysteresis. A label only
 * *promotes* (richer tier) once distance drops below boundary·(1−h) and only
 * *demotes* once it rises above boundary·(1+h); inside a dead-band it holds
 * prevTier, so an agent sitting on a boundary never flickers frame-to-frame.
 * Assumes fullDistance < nameDistance with non-overlapping bands (guaranteed by
 * the 2.25× spacing the caller uses against the ±10% band).
 */
export function computeNameplateTier(
  distance: number,
  prevTier: NameplateTier,
  opts: NameplateTierOpts,
): NameplateTier {
  if (opts.forceFull) return 'full';

  const { fullDistance: d1, nameDistance: d2, hysteresis: h } = opts;
  const d1Lo = d1 * (1 - h);
  const d1Hi = d1 * (1 + h);
  const d2Lo = d2 * (1 - h);
  const d2Hi = d2 * (1 + h);

  let tier: NameplateTier;
  if (distance < d1Lo) {
    tier = 'full';
  } else if (distance >= d2Hi) {
    tier = 'hidden';
  } else if (distance >= d1Hi && distance < d2Lo) {
    tier = 'name';
  } else if (distance < d1Hi) {
    // Dead-band around D1: hold full↔name.
    tier = prevTier === 'full' ? 'full' : 'name';
  } else {
    // Dead-band around D2 (d2Lo ≤ distance < d2Hi): hold name↔hidden.
    tier = prevTier === 'hidden' ? 'hidden' : 'name';
  }

  if (opts.capName && tier === 'full') tier = 'name';
  return tier;
}

/** Spacing of the name-only band relative to the full-nameplate distance (D1):
 *  D2 = D1 × this. 2.25× turns the default 40u full radius into a ~90u name band. */
export const NAMEPLATE_NAME_DISTANCE_FACTOR = 2.25;
/** ±10% hysteresis dead-band on each LOD boundary. */
export const NAMEPLATE_HYSTERESIS = 0.1;
/** More than this many agents inside D1 → non-queen/non-hovered agents drop to
 *  name-only (near-field declutter). */
export const NAMEPLATE_DENSITY_LIMIT = 8;

/**
 * Frame-shared near-field agent tally. Every nameplate increments `accum` when it
 * sits within D1 of the camera; all nodes read `published`, which holds LAST
 * frame's total. This is intentionally ONE FRAME STALE — R3F runs each node's
 * useFrame sequentially with no barrier, so a live count would depend on render
 * order and could itself induce flicker. The ±1-frame lag is imperceptible at
 * 60fps and the crowded state is stable while a swarm stays clustered.
 */
const nameplateDensity = { frameTime: -1, accum: 0, published: 0 };

export function tickNameplateDensity(frameTime: number, withinFull: boolean): number {
  if (frameTime !== nameplateDensity.frameTime) {
    // First node of a new frame: publish the previous tally, reset the accumulator.
    nameplateDensity.published = nameplateDensity.accum;
    nameplateDensity.accum = 0;
    nameplateDensity.frameTime = frameTime;
  }
  if (withinFull) nameplateDensity.accum += 1;
  return nameplateDensity.published;
}
