/**
 * Semantic-zoom and focus+context rules as pure functions (unit-tested; the
 * render loop calls them per region / per node).
 */
import type { Tier } from '../protocol';

/** Camera distance / scene radius thresholds for the global tier (edges, View requests). */
export const TIER_DISTANCE = { l0: 1.7, l1: 0.55 } as const;
/** Hysteresis band (fraction) so the View request does not flap at a boundary. */
export const TIER_HYSTERESIS = 0.08;

/**
 * Global tier from camera distance. `prev` adds hysteresis: leaving a tier
 * needs the distance to cross the boundary by `TIER_HYSTERESIS`.
 */
export function selectTier(distance: number, sceneRadius: number, prev?: Tier): Tier {
  const r = Math.max(1e-6, sceneRadius);
  const x = distance / r;
  const h = TIER_HYSTERESIS;
  const raw: Tier = x > TIER_DISTANCE.l0 ? 'l0' : x > TIER_DISTANCE.l1 ? 'l1' : 'l2';
  if (!prev || prev === raw) return raw;
  // Only switch when clearly past the boundary between prev and raw.
  if (prev === 'l0' && raw !== 'l0' && x > TIER_DISTANCE.l0 * (1 - h)) return 'l0';
  if (prev === 'l2' && raw !== 'l2' && x < TIER_DISTANCE.l1 * (1 + h)) return 'l2';
  if (prev === 'l1') {
    if (raw === 'l0' && x < TIER_DISTANCE.l0 * (1 + h)) return 'l1';
    if (raw === 'l2' && x > TIER_DISTANCE.l1 * (1 - h)) return 'l1';
  }
  return raw;
}

/** Continuous zoom level 0..2 from camera distance (edge-tier opacity). */
export function zoomFromDistance(distance: number, sceneRadius: number): number {
  const x = distance / Math.max(1e-6, sceneRadius);
  // 0 beyond l0 boundary*1.25, 2 inside l1 boundary*0.8, smooth in between (log scale).
  const hi = Math.log(TIER_DISTANCE.l0 * 1.25);
  const mid = Math.log(Math.sqrt(TIER_DISTANCE.l0 * TIER_DISTANCE.l1));
  const lo = Math.log(TIER_DISTANCE.l1 * 0.8);
  const lx = Math.log(Math.max(1e-6, x));
  if (lx >= hi) return 0;
  if (lx <= lo) return 2;
  if (lx >= mid) return (hi - lx) / (hi - mid);
  return 1 + (mid - lx) / (mid - lo);
}

/** Pixel radii at which a region (L0 aggregate) opens to files, then to symbols. */
export const REGION_PX = { openFiles: [50, 130], openSymbols: [260, 620] } as const;

function smooth(e0: number, e1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
}

/** Zoom level 0..2 for one region from its projected screen radius in pixels. */
export function regionZoom(radiusPx: number): number {
  return smooth(REGION_PX.openFiles[0], REGION_PX.openFiles[1], radiusPx) + smooth(REGION_PX.openSymbols[0], REGION_PX.openSymbols[1], radiusPx);
}

/**
 * Visibility weight per tier for a zoom level `z` in 0..2: the aggregate fades
 * out as files fade in; files stay as anchors while symbols fade in.
 */
export function tierWeights(z: number): [number, number, number] {
  const a = Math.min(1, Math.max(0, z));
  const b = Math.min(1, Math.max(0, z - 1));
  return [1 - a, a, b];
}

/** Context opacity for nodes outside the focus neighbourhood. */
export const CONTEXT_ALPHA = 0.15;

export interface ContextStyle {
  alpha: number;
  desaturate: boolean;
}

/**
 * Focus+context: with no focus every node is full; with a focus the k-hop
 * neighbourhood (hop ≤ k) is full and saturated, everything else 15% and grey.
 */
export function contextStyle(hasFocus: boolean, hop: number | undefined, k: number): ContextStyle {
  if (!hasFocus) return { alpha: 1, desaturate: false };
  if (hop !== undefined && hop <= k) return { alpha: 1, desaturate: false };
  return { alpha: CONTEXT_ALPHA, desaturate: true };
}

/** Exponential spring toward a target, frame-rate independent. */
export function springFactor(dtSeconds: number, halfLifeSeconds: number): number {
  if (halfLifeSeconds <= 0) return 1;
  return 1 - Math.pow(0.5, dtSeconds / halfLifeSeconds);
}

/**
 * Visibility weight of one node of tier `t` (0..2) in a region at zoom `z`,
 * when the server streams up to `streamTier` (plus symbols of the focused
 * file, `inFocusFile`). Unstreamed nodes collapse (0) instead of showing at
 * stale positions; an aggregate stays whole while nothing below it streams.
 */
export function streamedWeight(t: number, z: number, streamTier: number, inFocusFile: boolean): number {
  const streamed = t <= streamTier || (t === 2 && inFocusFile);
  if (!streamed) return 0;
  // Same arithmetic as tierWeights, without allocating (called per node per frame).
  const a = z < 0 ? 0 : z > 1 ? 1 : z;
  if (t === 0) return streamTier === 0 ? 1 : 1 - a;
  if (t === 1) return a;
  return z < 1 ? 0 : z > 2 ? 1 : z - 1;
}
