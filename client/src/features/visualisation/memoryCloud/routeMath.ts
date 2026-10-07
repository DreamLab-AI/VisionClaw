/**
 * Pure maths for drawing a search route through the memory cloud: easing,
 * route sampling along Bézier edges or hyperbolic geodesics, the root → mid →
 * tip colour gradient, the comet tail, bead pop-in and the reveal timeline.
 *
 * Adapted from the RuVector Explorer (https://github.com/ruvnet/RuVector,
 * docs/explorer/explorer.js, MIT licence): `ease`, `pathSamples`,
 * `drawPathAnim`, `pathStyle` and the `drawTopo` reveal clock, lifted from
 * 2-D canvas coordinates to 3-D scene coordinates. No three.js here, so every
 * function is unit-testable.
 */

import type { LayoutResult, Vec3 } from '../memoryTrajectory/types';

/** Explorer palette (ember), with the mint used for kept / learned / agreement marks. */
export const ROUTE_PALETTE = {
  root: '#6f9bff',
  mid: '#ffffff',
  tip: '#ff7a3d',
  mint: '#7ef0cf',
  /** faint rejected twigs */
  prune: '#ff7a3d',
  /** kept tree edges */
  edge: '#d6def0',
  /** upper-layer (greedy descent) edges */
  upper: '#6f9bff',
  /** sidecar's own top-k */
  sidecar: '#ffd36e',
  /** local top-k disagreeing with exact search */
  miss: '#ff5f6e',
  background: '#05060a',
} as const;

/** where the white core sits on the route gradient */
export const GRADIENT_MID = 0.42;

export const clamp01 = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v);

export const ease = {
  /** quadratic ease-in-out */
  io: (t: number): number => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2),
  /** inverse of `io` */
  ioInv: (y: number): number => (y < 0.5 ? Math.sqrt(y / 2) : 1 - Math.sqrt((1 - y) * 2) / 2),
  /** cubic ease-out */
  out: (t: number): number => 1 - Math.pow(1 - t, 3),
  /** ease-out-back: overshoots then settles (bead pop) */
  back: (t: number): number => {
    const c = 1.70158;
    return 1 + (c + 1) * Math.pow(t - 1, 3) + c * Math.pow(t - 1, 2);
  },
};

export const lerp3 = (a: Vec3, b: Vec3, t: number): Vec3 => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

export const dist3 = (a: Vec3, b: Vec3): number =>
  Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

/** Quadratic Bézier a → b with control c. */
export function quadPoint(a: Vec3, c: Vec3, b: Vec3, t: number): Vec3 {
  const u = 1 - t;
  return [
    u * u * a[0] + 2 * u * t * c[0] + t * t * b[0],
    u * u * a[1] + 2 * u * t * c[1] + t * t * b[1],
    u * u * a[2] + 2 * u * t * c[2] + t * t * b[2],
  ];
}

/** Resample a polyline to `n` equal arc-length segments (`n + 1` points). */
export function resamplePolyline(poly: Vec3[], n: number): Vec3[] {
  if (poly.length === 0) return [];
  if (poly.length === 1) return Array.from({ length: n + 1 }, () => [...poly[0]] as Vec3);
  const cum = [0];
  for (let i = 1; i < poly.length; i++) cum.push(cum[i - 1] + dist3(poly[i - 1], poly[i]));
  const total = cum[cum.length - 1];
  if (total <= 1e-12) return Array.from({ length: n + 1 }, () => [...poly[0]] as Vec3);
  const out: Vec3[] = [];
  let seg = 1;
  for (let k = 0; k <= n; k++) {
    const s = (total * k) / n;
    while (seg < poly.length - 1 && cum[seg] < s) seg++;
    const len = cum[seg] - cum[seg - 1];
    const f = len > 0 ? (s - cum[seg - 1]) / len : 0;
    out.push(lerp3(poly[seg - 1], poly[seg], clamp01(f)));
  }
  return out;
}

/** Edge parent → child as `n + 1` samples: geodesic, Bézier or straight. */
export function sampleEdge(layout: LayoutResult, parent: number, child: number, n: number): Vec3[] | null {
  const a = layout.positions.get(parent);
  const b = layout.positions.get(child);
  if (!a || !b) return null;
  const geo = layout.geodesics?.get(child);
  if (geo && geo.length >= 2) return resamplePolyline(geo, n);
  const c = layout.controls.get(child);
  const out: Vec3[] = [];
  for (let k = 0; k <= n; k++) {
    const t = k / n;
    out.push(c ? quadPoint(a, c, b, t) : lerp3(a, b, t));
  }
  return out;
}

export interface RouteSamples {
  /** dense polyline root → … → best result */
  pts: Vec3[];
  /** sample index of each node on the path (the first is 0) */
  knots: number[];
}

/**
 * Sample the winning path into a dense polyline. Mirrors Explorer
 * `pathSamples`: `n` samples per hop, shared endpoints counted once.
 */
export function sampleRoute(path: number[], layout: LayoutResult, n = 16): RouteSamples {
  const pts: Vec3[] = [];
  const knots = [0];
  if (path.length < 2) return { pts, knots };
  for (let i = 0; i < path.length - 1; i++) {
    const seg = sampleEdge(layout, path[i], path[i + 1], n);
    if (!seg) return { pts: [], knots: [0] };
    for (let k = i ? 1 : 0; k <= n; k++) pts.push(seg[k]);
    knots.push(pts.length - 1);
  }
  return { pts, knots };
}

/** Point at a fractional sample index, clamped to the polyline. */
export function pointAt(pts: Vec3[], idx: number): Vec3 {
  const last = pts.length - 1;
  if (idx <= 0) return [...pts[0]] as Vec3;
  if (idx >= last) return [...pts[last]] as Vec3;
  const i = Math.floor(idx);
  return lerp3(pts[i], pts[i + 1], idx - i);
}

export function hexToRgb01(hex: string): Vec3 {
  const n = parseInt(hex.replace('#', ''), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const ROOT_RGB = hexToRgb01(ROUTE_PALETTE.root);
const MID_RGB = hexToRgb01(ROUTE_PALETTE.mid);
const TIP_RGB = hexToRgb01(ROUTE_PALETTE.tip);

/** Route colour at u ∈ [0, 1]: root blue → white core at 0.42 → orange tip. */
export function routeGradient(u: number): Vec3 {
  const t = clamp01(u);
  if (t <= GRADIENT_MID) return lerp3(ROOT_RGB, MID_RGB, t / GRADIENT_MID);
  return lerp3(MID_RGB, TIP_RGB, (t - GRADIENT_MID) / (1 - GRADIENT_MID));
}

export interface TailSegment {
  a: Vec3;
  b: Vec3;
  /** 0..1 */
  alpha: number;
  /** relative stroke width, 0..1 */
  width: number;
}

/**
 * Comet tail behind a head at fractional sample `head`: `segments` pieces
 * covering 18 % of the route, fading in alpha and width towards the back.
 * Segment 0 ends at the head; each next segment ends where the last began.
 */
export function cometTail(pts: Vec3[], head: number, segments = 18): TailSegment[] {
  const last = pts.length - 1;
  if (last < 1) return [];
  const tail = Math.min(head, last * 0.18);
  const out: TailSegment[] = [];
  for (let s = 0; s < segments; s++) {
    const a0 = head - (tail * (s + 1)) / segments;
    const a1 = head - (tail * s) / segments;
    if (a0 < 0) break;
    const f = 1 - s / segments;
    out.push({ a: pointAt(pts, a0), b: pointAt(pts, a1), alpha: 0.9 * f, width: (2.6 * f + 0.4) / 3 });
  }
  return out;
}

/** Bead pop-in at knot sample `knot` as the head passes: 0 → overshoot → 1. */
export function beadScale(head: number, knot: number): number {
  const a = clamp01((head - knot) / 10 + 1);
  if (a <= 0) return 0;
  return Math.max(0, ease.back(a));
}

/** Seconds of normalised playback for the tree reveal and the path trace. */
export const REVEAL_DUR = 7;
export const PATH_DUR = 2.8;
/** seconds for a revealed edge to grow out of its parent */
export const GROW_DUR = 0.7;
export const TOTAL_DUR = REVEAL_DUR + PATH_DUR;

export interface RevealPhase {
  /** tree reveal progress 0..1 */
  prog: number;
  /** path trace progress 0..1 */
  pathT: number;
  done: boolean;
}

/**
 * Reveal clock. `el` is elapsed playback time already multiplied by the
 * playback speed, so changing speed mid-play never jumps the scene.
 */
export function revealPhases(el: number, reducedMotion = false): RevealPhase {
  if (reducedMotion) return { prog: 1, pathT: 1, done: true };
  const prog = clamp01(el / REVEAL_DUR);
  const pathT = clamp01((el - REVEAL_DUR) / PATH_DUR);
  return { prog, pathT, done: pathT >= 1 };
}

/** Number of trace nodes visible at reveal progress `prog` (root always). */
export function shownCount(prog: number, total: number): number {
  return Math.max(1, Math.min(total, Math.ceil(ease.io(prog) * total)));
}

/** Growth 0..1 of the k-th discovered node at elapsed `el`. */
export function nodeGrowth(k: number, total: number, el: number): number {
  if (k === 0) return 1;
  return clamp01((el - REVEAL_DUR * ease.ioInv(k / total)) / GROW_DUR);
}

/** Focus pull 0..0.6 once the path trace starts; dims everything off the route. */
export const focusPull = (pathT: number): number => (pathT > 0 ? Math.min(0.6, ease.io(pathT)) : 0);

/**
 * Which nodes to draw: every kept node, and rejected nodes thinned
 * deterministically (by their hash) to roughly `cap`, as the Explorer does to
 * keep the rejected twigs a faint texture rather than a wall.
 */
export function thinRejected(nodes: Array<{ kept: boolean; hash: number }>, cap: number): boolean[] {
  let rejected = 0;
  for (const n of nodes) if (!n.kept) rejected++;
  const p = rejected > 0 ? Math.min(1, cap / rejected) : 1;
  return nodes.map((n) => n.kept || n.hash < p);
}

/**
 * Fade for one node during a view morph: `interpolateLayouts` returns a
 * MorphFrame whose `opacity` map fades nodes that exist on only one side of
 * the morph. Materials here are additive, so callers multiply colour by this.
 * A settled layout (no map) and unknown ids are fully opaque.
 */
export function morphFade(
  layout: { opacity?: ReadonlyMap<number, number> },
  id: number,
): number {
  const a = layout.opacity?.get(id);
  if (a === undefined || !Number.isFinite(a)) return 1;
  return Math.max(0, Math.min(1, a));
}

/** Beat sample as published on `beatState` (memoryCloudStore). */
export interface BeatLike {
  on: boolean;
  pulse: number;
  bar: number;
  phase: number;
}

export interface BeatModulation {
  /** multiplier on the route glow */
  glow: number;
  /** comet lead ahead of the head, as a fraction of the route length */
  cometLead: number;
  /** multiplier on the comet size */
  cometScale: number;
  /** root ring scale; null leaves the caller's own idle breathing */
  rootScale: number | null;
  /** answer pulse-ring phase locked to the beat; null leaves its own clock */
  pulsePhase: number | null;
}

const NEUTRAL: BeatModulation = { glow: 1, cometLead: 0, cometScale: 1, rootScale: null, pulsePhase: null };
const STILL: BeatModulation = { glow: 1, cometLead: 0, cometScale: 1, rootScale: 1, pulsePhase: null };

/**
 * How the beat modulates the route. Under reduced motion the beat changes
 * nothing at all, as in the XR client (ADR-2107: the swell is held at exactly
 * 0); only the panel's beat readout still shows the tempo.
 */
export function beatModulation(beat: BeatLike, reducedMotion: boolean): BeatModulation {
  if (reducedMotion) return { ...STILL };
  if (!beat.on) return { ...NEUTRAL };
  return {
    glow: 1 + 0.2 * beat.pulse,
    cometLead: 0.015 * beat.pulse,
    cometScale: 1 + 0.35 * beat.pulse,
    rootScale: 1 + 0.12 * beat.bar,
    pulsePhase: beat.phase,
  };
}
