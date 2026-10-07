/**
 * Eased interpolation between two layouts of the same search, for gliding
 * the drawn route from one view to another.
 *
 * Follows the RuVector Explorer's `startMorph` view glide
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence); the
 * Explorer only glides shared nodes, here nodes present on one side only
 * grow out of (or shrink into) their nearest ancestor's position and fade.
 */
import type { LayoutResult, Vec3 } from './types';

/** Interpolated layout plus a per-node opacity (1 for shared nodes, fading for one-sided ones). */
export interface MorphFrame extends LayoutResult {
  opacity: Map<number, number>;
}

/** Parent lookup: a `child → parent` map, or anything shaped like a {@link SearchTree}'s `nodes`. */
export type ParentSource = Map<number, number> | { nodes: Map<number, { parent: number }> };

/** Cubic ease-in-out on [0, 1]. */
export function easeInOutCubic(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

const lerp3 = (a: Vec3, b: Vec3, t: number): Vec3 => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const copy3 = (v: Vec3): Vec3 => [v[0], v[1], v[2]];

function parentOf(src: ParentSource | undefined, id: number): number {
  if (!src) return -1;
  if (src instanceof Map) return src.get(id) ?? -1;
  return src.nodes.get(id)?.parent ?? -1;
}

/** Position of the nearest ancestor of `id` present in `side`, or undefined. */
function ancestorPosition(side: Map<number, Vec3>, src: ParentSource | undefined, id: number): Vec3 | undefined {
  const seen = new Set<number>([id]);
  let p = parentOf(src, id);
  while (p !== -1 && !seen.has(p)) {
    const pos = side.get(p);
    if (pos) return pos;
    seen.add(p);
    p = parentOf(src, p);
  }
  return undefined;
}

function snapshot(l: LayoutResult): MorphFrame {
  const out: MorphFrame = {
    positions: new Map([...l.positions].map(([k, v]) => [k, copy3(v)])),
    controls: new Map([...l.controls].map(([k, v]) => [k, copy3(v)])),
    opacity: new Map([...l.positions.keys()].map((k) => [k, 1])),
  };
  if (l.geodesics) out.geodesics = new Map([...l.geodesics].map(([k, pts]) => [k, pts.map(copy3)]));
  return out;
}

/**
 * Interpolates from `a` (t = 0) to `b` (t = 1) with a cubic ease; `t` is
 * clamped. At the ends the result equals the corresponding input. Pass
 * `parents` (a map or the search tree) so one-sided nodes move from or to
 * their ancestor's position; without it they hold still and only fade.
 */
export function interpolateLayouts(a: LayoutResult, b: LayoutResult, t: number, parents?: ParentSource): MorphFrame {
  const tc = Number.isFinite(t) ? Math.min(1, Math.max(0, t)) : 0;
  if (tc <= 0) return snapshot(a);
  if (tc >= 1) return snapshot(b);
  const e = easeInOutCubic(tc);
  const positions = new Map<number, Vec3>();
  const controls = new Map<number, Vec3>();
  const opacity = new Map<number, number>();
  const ids: number[] = [...a.positions.keys()];
  for (const id of b.positions.keys()) if (!a.positions.has(id)) ids.push(id);

  for (const id of ids) {
    const pa = a.positions.get(id);
    const pb = b.positions.get(id);
    let from: Vec3;
    let to: Vec3;
    if (pa && pb) {
      from = pa;
      to = pb;
      opacity.set(id, 1);
    } else if (pb) {
      from = ancestorPosition(a.positions, parents, id) ?? pb;
      to = pb;
      opacity.set(id, e);
    } else {
      from = pa!;
      to = ancestorPosition(b.positions, parents, id) ?? pa!;
      opacity.set(id, 1 - e);
    }
    positions.set(id, lerp3(from, to, e));
    const ca = a.controls.get(id) ?? (pa ? undefined : from);
    const cb = b.controls.get(id) ?? (pb ? undefined : to);
    if (ca && cb) controls.set(id, lerp3(ca, cb, e));
    else if (ca || cb) controls.set(id, copy3((ca ?? cb)!));
  }

  const out: MorphFrame = { positions, controls, opacity };
  if (a.geodesics && b.geodesics) {
    const geodesics = new Map<number, Vec3[]>();
    for (const [id, ga] of a.geodesics) {
      const gb = b.geodesics.get(id);
      if (!gb || gb.length !== ga.length) continue;
      geodesics.set(id, ga.map((p, i) => lerp3(p, gb[i], e)));
    }
    out.geodesics = geodesics;
  }
  return out;
}
