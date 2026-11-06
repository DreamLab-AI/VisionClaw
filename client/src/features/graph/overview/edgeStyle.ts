/**
 * Overview edge styling and capacity ordering (ADR-2181 D2, D4).
 *
 * The shaders (OverviewEdgeMaterial) mirror `fadeAlpha` and `edgeWidthPx`
 * exactly; these TS versions are the tested reference and are used on the CPU
 * for the glass-tube close-zoom selection.
 */

/**
 * Alpha by on-screen length: 1 up to `nearPx`, falling linearly to `minOpacity`
 * at `farPx` and beyond (cosmos.gl's linkVisibilityDistanceRange /
 * linkVisibilityMinTransparency). A degenerate range (far <= near) is a step.
 */
export function fadeAlpha(lenPx: number, nearPx: number, farPx: number, minOpacity: number): number {
  const min = minOpacity < 0 ? 0 : minOpacity > 1 ? 1 : minOpacity;
  if (!(lenPx > nearPx)) return 1;
  if (!(farPx > nearPx) || lenPx >= farPx) return min;
  const t = (lenPx - nearPx) / (farPx - nearPx);
  return 1 + (min - 1) * t;
}

/** Overview line alpha at the shipped Edge Opacity default (ADR-2181 D2). */
export const OVERVIEW_EDGE_BASE_OPACITY = 0.6;

/**
 * Overview line alpha from the one Edge Opacity setting
 * (`visualisation.graphs.knowledge.edges.opacity`, the same control that sets
 * the close-zoom glass tubes). Scaled so the shipped default reproduces
 * OVERVIEW_EDGE_BASE_OPACITY, linear to 0 at 0, clamped to [0, 1]. A missing
 * or non-finite setting gives the base alpha.
 */
export function overviewEdgeOpacity(setting: number | undefined, settingDefault: number): number {
  if (typeof setting !== 'number' || !Number.isFinite(setting) || !(settingDefault > 0)) {
    return OVERVIEW_EDGE_BASE_OPACITY;
  }
  const a = (setting / settingDefault) * OVERVIEW_EDGE_BASE_OPACITY;
  return a < 0 ? 0 : a > 1 ? 1 : a;
}

/** Weight at which an edge reaches `maxWidthPx` (the tube mapping's 2× cap, squared). */
export const WEIGHT_AT_MAX_WIDTH = 4;

/**
 * Width in CSS pixels from weight: `widthPx` for weight ≤ 1 (and missing or
 * bad weights), growing with √weight to `maxWidthPx` at WEIGHT_AT_MAX_WIDTH —
 * the same sqrt-area reasoning as the glass tubes' radius factor. Never under
 * 1 device pixel (the shader floors at 1 / devicePixelRatio CSS px).
 */
export function edgeWidthPx(weight: number, widthPx: number, maxWidthPx: number): number {
  const lo = widthPx > 0 ? widthPx : 1;
  const hi = maxWidthPx > lo ? maxWidthPx : lo;
  if (!(weight > 1)) return lo;
  const t = Math.min(1, (Math.sqrt(weight) - 1) / (Math.sqrt(WEIGHT_AT_MAX_WIDTH) - 1));
  return lo + (hi - lo) * t;
}

/**
 * Which planned edges to draw when the budget is short (ADR-2181 D4: highest
 * weight first). Returns plan indices: all of them, in plan order, when they
 * fit; otherwise the `capacity` heaviest, ties broken by plan order (stable).
 * Allocates — call on plan or capacity change, never per frame.
 */
export function selectDrawnEdges(weights: ArrayLike<number>, count: number, capacity: number): Uint32Array {
  const n = Math.max(0, Math.min(count, weights.length));
  const cap = Math.max(0, Math.floor(capacity));
  if (n <= cap) {
    const all = new Uint32Array(n);
    for (let i = 0; i < n; i++) all[i] = i;
    return all;
  }
  const order = new Uint32Array(n);
  for (let i = 0; i < n; i++) order[i] = i;
  const w = (i: number) => {
    const v = weights[i];
    return Number.isFinite(v) ? v : 0;
  };
  order.sort((a, b) => (w(b) - w(a)) || (a - b));
  const chosen = order.slice(0, cap);
  chosen.sort();
  return chosen;
}
