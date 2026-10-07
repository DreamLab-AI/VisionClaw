/**
 * 3-D layouts of a search tree in scene units.
 *
 * - `space`  : nodes sit at their cloud positions.
 * - `canopy` : the Explorer's dendritic fan turned into a dome. The root sits
 *              at the base (y = -radius / 2); radius grows with depth and
 *              discovery rank, the Explorer's fan angle becomes the azimuth
 *              and a second, hash-seeded angle spreads nodes in elevation.
 * - `tree`   : layered rows in the XY plane, root row at the bottom.
 * - `hyper`  : a Poincaré disk in the XZ plane (y = 0) with a Möbius focus.
 *
 * Adapted from the RuVector Explorer's `layoutFor`, `hyperFrame`,
 * `geoPath`, `hypCtrl`, `mob` and `mobInv`
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence).
 */
import type { LayoutOptions, LayoutResult, SearchTree, SearchTreeNode, Vec3 } from './types';
import { hash01 } from './vec';
import { CANOPY_HALF_ANGLE } from './tree';

export type Complex = [number, number];

const cdiv = (a: Complex, b: Complex): Complex => {
  const d = b[0] * b[0] + b[1] * b[1] || 1e-12;
  return [(a[0] * b[0] + a[1] * b[1]) / d, (a[1] * b[0] - a[0] * b[1]) / d];
};

/** Möbius map of the unit disk taking `a` to the origin: (z - a) / (1 - conj(a) z). */
export function mobius(z: Complex, a: Complex): Complex {
  return cdiv([z[0] - a[0], z[1] - a[1]], [1 - (a[0] * z[0] + a[1] * z[1]), -(a[0] * z[1] - a[1] * z[0])]);
}

/** Inverse of {@link mobius}: takes the origin back to `a`. */
export function mobiusInverse(w: Complex, a: Complex): Complex {
  return mobius(w, [-a[0], -a[1]]);
}

export const DEFAULT_HYPER_STEP = 0.55;
const HYPER_STEP_MIN = 0.2;
const HYPER_STEP_MAX = 0.9;
/** largest focus modulus; nearer the rim the map degenerates numerically */
const HYPER_FOCUS_MAX = 0.95;
/** samples per geodesic edge (segments; the polyline has one more point) */
export const GEODESIC_SEGMENTS = 8;

const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);
const finiteOr = (v: number, f: number) => (Number.isFinite(v) ? v : f);

/** Lays the tree out for `opts.view`; every node gets finite coordinates and every non-root node a control point. */
export function layoutTree(tree: SearchTree, opts: LayoutOptions): LayoutResult {
  const radius = Number.isFinite(opts.radius) && opts.radius > 0 ? opts.radius : 100;
  switch (opts.view) {
    case 'space':
      return spaceLayout(tree, opts.spacePositions);
    case 'canopy':
      return canopyLayout(tree, radius);
    case 'tree':
      return rowLayout(tree, radius);
    case 'hyper':
      return hyperLayout(tree, radius, opts.hyperStep, opts.hyperFocus);
    default:
      throw new RangeError(`unknown trajectory view ${String(opts.view)}`);
  }
}

// ---------------------------------------------------------------------------
// space

function spaceLayout(tree: SearchTree, sp: Float32Array | number[] | undefined): LayoutResult {
  const positions = new Map<number, Vec3>();
  const controls = new Map<number, Vec3>();
  for (const n of tree.list) {
    const o = n.id * 3;
    const ok = !!sp && o + 2 < sp.length && Number.isFinite(sp[o]) && Number.isFinite(sp[o + 1]) && Number.isFinite(sp[o + 2]);
    if (ok) positions.set(n.id, [sp![o], sp![o + 1], sp![o + 2]]);
    else {
      const p = positions.get(n.parent);
      positions.set(n.id, p ? [p[0], p[1], p[2]] : [0, 0, 0]);
    }
  }
  for (const n of tree.list) {
    if (n.parent === -1) continue;
    const a = positions.get(n.parent)!;
    const b = positions.get(n.id)!;
    const len = Math.hypot(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    // midpoint drawn a quarter of the way back toward the parent and lifted, so edges bow out of the cloud
    controls.set(n.id, [
      (a[0] + b[0]) / 2 + ((a[0] - b[0]) / 2) * 0.25,
      (a[1] + b[1]) / 2 + ((a[1] - b[1]) / 2) * 0.25 + 0.12 * len,
      (a[2] + b[2]) / 2 + ((a[2] - b[2]) / 2) * 0.25,
    ]);
  }
  return { positions, controls };
}

// ---------------------------------------------------------------------------
// canopy

interface Polar {
  r: number;
  t: number;
  el: number;
}

function canopyLayout(tree: SearchTree, radius: number): LayoutResult {
  const positions = new Map<number, Vec3>();
  const controls = new Map<number, Vec3>();
  const norm = new Map<number, number>();
  const polar = new Map<number, Polar>();
  const onPath = new Set(tree.path);
  const keptKidCount = (n: SearchTreeNode) => n.children.reduce((s, c) => s + (c.kept ? 1 : 0), 0);

  // dendritic radius: grows with depth and discovery rank, never shrinks toward a child
  for (const n of tree.list) {
    if (n.parent === -1) {
      norm.set(n.id, 0);
      continue;
    }
    if (!n.kept) continue;
    const pn = norm.get(n.parent) ?? 0;
    const v = 0.07 + 0.93 * ((0.55 * n.depth) / Math.max(1, tree.maxDepth) + 0.45 * Math.sqrt(Math.max(0, n.rank) / Math.max(1, tree.keptTotal)));
    norm.set(n.id, Math.max(v, pn + 0.035));
  }
  // crown: a monotone remap puts the best result at 0.93 and squeezes anything beyond it into the rim band
  let rm = 0;
  for (const n of tree.list) if (n.kept) rm = Math.max(rm, norm.get(n.id) ?? 0);
  const tipId = tree.path[tree.path.length - 1];
  const tipN = norm.get(tipId) ?? 0;
  const rt = tree.path.length > 1 && tipN > 0 ? tipN : rm > 0 ? rm : 1;
  const CROWN = 0.93;
  const remap = (v: number) => (v <= rt ? (v * CROWN) / rt : CROWN + ((1 - CROWN) * (v - rt)) / Math.max(1e-6, rm - rt));
  const azimuth = Math.PI / CANOPY_HALF_ANGLE;
  const base: Vec3 = [0, -radius / 2, 0];
  const place = (p: Polar): Vec3 => {
    const phi = p.t * azimuth;
    const ce = Math.cos(p.el);
    return [base[0] + radius * p.r * ce * Math.sin(phi), base[1] + radius * p.r * Math.sin(p.el), base[2] + radius * p.r * ce * Math.cos(phi)];
  };

  for (const n of tree.list) {
    const p = n.parent === -1 ? undefined : polar.get(n.parent);
    let pol: Polar;
    if (!p) pol = { r: 0, t: n.angle, el: 0.8 };
    else if (n.kept) {
      const leaf = keptKidCount(n) === 0 && !onPath.has(n.id);
      let r = remap(norm.get(n.id) ?? p.r);
      if (leaf) r = Math.min(r, p.r + 0.1);
      const want = (n.angle - p.t) * (leaf ? 0.3 : 0.35 + 0.65 * r);
      // the swing toward the own sector is softly capped by the radial gain, so a hub's fan stays a short spray
      const mx = (0.06 + 1.2 * (r - p.r)) / Math.max(r, 0.15);
      const t = p.t + mx * Math.tanh(want / mx);
      const elTarget = 0.3 + 0.9 * hash01(n.id * 13 + 5);
      const el = clamp(p.el + (elTarget - p.el) * (0.25 + 0.35 * r), 0.12, 1.45);
      pol = { r, t, el };
    } else {
      const h = hash01(n.id * 7 + 3);
      const r = Math.min(1.06, p.r + 0.05 + 0.06 * h);
      pol = { r, t: p.t + (n.hash - 0.5) * 0.06, el: clamp(p.el + (hash01(n.id * 11 + 1) - 0.5) * 0.08, 0.12, 1.45) };
    }
    polar.set(n.id, pol);
    positions.set(n.id, place(pol));
  }
  for (const n of tree.list) {
    if (n.parent === -1) continue;
    const a = polar.get(n.parent)!;
    const b = polar.get(n.id)!;
    controls.set(n.id, place({ r: a.r + (b.r - a.r) * 0.5, t: a.t * 0.6 + b.t * 0.4, el: a.el * 0.6 + b.el * 0.4 }));
  }
  return { positions, controls };
}

// ---------------------------------------------------------------------------
// tree

function rowLayout(tree: SearchTree, radius: number): LayoutResult {
  const positions = new Map<number, Vec3>();
  const controls = new Map<number, Vec3>();
  // rejected nodes sit one row beyond their parent, so the deepest row counts them too
  const maxDA = tree.list.reduce((m, n) => Math.max(m, n.depth), 1);
  const span = Math.max(1, tree.leaves - 1);
  for (const n of tree.list) {
    const x = (clamp(n.leafX, 0, span) / span - 0.5) * 2 * radius;
    const y = -radius + (n.depth / maxDA) * 2 * radius;
    positions.set(n.id, [x, y, 0]);
  }
  for (const n of tree.list) {
    if (n.parent === -1) continue;
    const a = positions.get(n.parent)!;
    const b = positions.get(n.id)!;
    controls.set(n.id, [b[0], a[1] + (b[1] - a[1]) * 0.15, 0]);
  }
  return { positions, controls };
}

// ---------------------------------------------------------------------------
// hyper

function hyperLayout(tree: SearchTree, radius: number, stepIn?: number, focusIn?: [number, number]): LayoutResult {
  const st = clamp(finiteOr(stepIn ?? DEFAULT_HYPER_STEP, DEFAULT_HYPER_STEP), HYPER_STEP_MIN, HYPER_STEP_MAX);
  let focus: Complex = [finiteOr(focusIn?.[0] ?? 0, 0), finiteOr(focusIn?.[1] ?? 0, 0)];
  const fm = Math.hypot(focus[0], focus[1]);
  if (fm > HYPER_FOCUS_MAX) focus = [(focus[0] / fm) * HYPER_FOCUS_MAX, (focus[1] / fm) * HYPER_FOCUS_MAX];
  const span = Math.max(1, tree.leaves - 1);
  const hr = new Map<number, number>();
  const disk = new Map<number, Complex>();
  const inDisk = (w: Complex): Complex => {
    const m = Math.hypot(w[0], w[1]);
    const cap = 1 - 1e-12;
    return m > cap ? [(w[0] / m) * cap, (w[1] / m) * cap] : w;
  };
  for (const n of tree.list) {
    const t = -Math.PI + (clamp(n.leafX, 0, span) / tree.leaves) * 2 * Math.PI;
    let h: number;
    if (n.parent === -1) h = 0;
    else if (n.kept) h = n.depth * st;
    else h = (hr.get(n.parent) ?? 0) + st * (0.5 + 0.4 * hash01(n.id * 7 + 3));
    hr.set(n.id, h);
    const r = Math.min(Math.tanh(h / 2), 1 - 1e-9);
    disk.set(n.id, inDisk(mobius([r * Math.cos(t - Math.PI / 2), r * Math.sin(t - Math.PI / 2)], focus)));
  }
  const toScene = (w: Complex): Vec3 => [w[0] * radius, 0, w[1] * radius];
  const positions = new Map<number, Vec3>();
  for (const [id, w] of disk) positions.set(id, toScene(w));
  const controls = new Map<number, Vec3>();
  const geodesics = new Map<number, Vec3[]>();
  for (const n of tree.list) {
    if (n.parent === -1) continue;
    const za = disk.get(n.parent)!;
    const zb = disk.get(n.id)!;
    const u = mobius(zb, za);
    const pts: Vec3[] = [positions.get(n.parent)!.slice() as Vec3];
    for (let k = 1; k < GEODESIC_SEGMENTS; k++) {
      pts.push(toScene(inDisk(mobiusInverse([(u[0] * k) / GEODESIC_SEGMENTS, (u[1] * k) / GEODESIC_SEGMENTS], za))));
    }
    pts.push(positions.get(n.id)!.slice() as Vec3);
    geodesics.set(n.id, pts);
    // quadratic control through the geodesic midpoint m: 2m - (a + b) / 2
    const m = inDisk(mobiusInverse([u[0] * 0.5, u[1] * 0.5], za));
    controls.set(n.id, [(2 * m[0] - (za[0] + zb[0]) / 2) * radius, 0, (2 * m[1] - (za[1] + zb[1]) / 2) * radius]);
  }
  return { positions, controls, geodesics };
}
