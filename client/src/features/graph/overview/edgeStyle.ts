/**
 * Overview edge styling and capacity ordering (ADR-2181 D2, D4).
 *
 * The on-screen length fade is written once, `fadeAlphaWith`, over a small
 * scalar interface: `fadeAlpha` evaluates it on numbers (CPU, the glass-tube
 * close-zoom selection, the tests) and OverviewEdgeMaterial evaluates it on TSL
 * nodes, so the shader and the CPU cannot drift. `edgeWidthPx` is mirrored by
 * the shader's width term.
 */

/**
 * The arithmetic `fadeAlphaWith` needs. `N` is a scalar (number or TSL float
 * node), `B` a boolean of the same kind.
 */
export interface ScalarOps<N, B> {
  lit(v: number): N;
  add(a: N, b: N): N;
  sub(a: N, b: N): N;
  mul(a: N, b: N): N;
  div(a: N, b: N): N;
  max(a: N, b: N): N;
  clamp01(a: N): N;
  gt(a: N, b: N): B;
  select(c: B, ifTrue: N, ifFalse: N): N;
}

/** Plain-number ScalarOps. */
export const numberOps: ScalarOps<number, boolean> = {
  lit: (v) => v,
  add: (a, b) => a + b,
  sub: (a, b) => a - b,
  mul: (a, b) => a * b,
  div: (a, b) => a / b,
  max: (a, b) => (a > b ? a : b),
  clamp01: (a) => (a < 0 ? 0 : a > 1 ? 1 : a),
  gt: (a, b) => a > b,
  select: (c, t, f) => (c ? t : f),
};

/** The TSL node methods `tslScalarOps` calls. */
interface TslScalar {
  add(b: unknown): TslScalar;
  sub(b: unknown): TslScalar;
  mul(b: unknown): TslScalar;
  div(b: unknown): TslScalar;
  greaterThan(b: unknown): unknown;
}

/** ScalarOps over TSL float nodes; `T` is the `three/tsl` module. */
export function tslScalarOps(T: {
  float(v: number): TslScalar;
  max(a: unknown, b: unknown): TslScalar;
  clamp(a: unknown, lo: number, hi: number): TslScalar;
  select(c: unknown, a: unknown, b: unknown): TslScalar;
}): ScalarOps<TslScalar, unknown> {
  return {
    lit: (v) => T.float(v),
    add: (a, b) => a.add(b),
    sub: (a, b) => a.sub(b),
    mul: (a, b) => a.mul(b),
    div: (a, b) => a.div(b),
    max: (a, b) => T.max(a, b),
    clamp01: (a) => T.clamp(a, 0, 1),
    gt: (a, b) => a.greaterThan(b),
    select: (c, t, f) => T.select(c, t, f),
  };
}

/** Smallest fade range: a degenerate range (far <= near) acts as a step at `near`. */
const MIN_FADE_RANGE_PX = 1e-4;

/**
 * Alpha by on-screen length: 1 up to `nearPx`, falling linearly to
 * `minOpacity` (clamped to [0, 1]) at `farPx` and beyond (cosmos.gl's
 * linkVisibilityDistanceRange / linkVisibilityMinTransparency). A NaN length
 * draws at full alpha.
 */
export function fadeAlphaWith<N, B>(o: ScalarOps<N, B>, lenPx: N, nearPx: N, farPx: N, minOpacity: N): N {
  const min = o.clamp01(minOpacity);
  const range = o.max(o.sub(farPx, nearPx), o.lit(MIN_FADE_RANGE_PX));
  const t = o.clamp01(o.div(o.sub(lenPx, nearPx), range));
  // min + (1 − min)(1 − t): exactly `min` at t = 1 (1 + (min − 1)t is not, in floats).
  return o.select(o.gt(lenPx, nearPx), o.add(min, o.mul(o.sub(o.lit(1), min), o.sub(o.lit(1), t))), o.lit(1));
}

/** `fadeAlphaWith` on numbers. */
export function fadeAlpha(lenPx: number, nearPx: number, farPx: number, minOpacity: number): number {
  return fadeAlphaWith(numberOps, lenPx, nearPx, farPx, minOpacity);
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
