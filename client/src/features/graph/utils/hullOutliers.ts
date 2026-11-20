/**
 * Robust point selection for community hulls (ADR-2206).
 *
 * A hull outlines where a theme lives. The display places page nodes and
 * ontology classes in separate bodies, so a core with a handful of members in
 * the other body stretched one hull into a slab across the whole scene. A member
 * is kept when its distance from the community's median centre is at most
 * `OUTLIER_FACTOR` times the median member distance: relative to the community's
 * own spread, so compact and elongated communities both stay whole.
 */
export type Point3 = [number, number, number];

/** A member further than this many median distances from the centre is a stray. */
export const OUTLIER_FACTOR = 4;
/** Below this many points there is no meaningful spread to judge against. */
const MIN_POINTS_TO_TRIM = 8;

function median(values: number[]): number {
  const s = [...values].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

export function keepHullPoints(points: ReadonlyArray<Point3>): Point3[] {
  const finite = points.filter(
    (p) => Number.isFinite(p[0]) && Number.isFinite(p[1]) && Number.isFinite(p[2]),
  ) as Point3[];
  if (finite.length < MIN_POINTS_TO_TRIM) return finite;

  const cx = median(finite.map((p) => p[0]));
  const cy = median(finite.map((p) => p[1]));
  const cz = median(finite.map((p) => p[2]));
  const dist = finite.map((p) => Math.hypot(p[0] - cx, p[1] - cy, p[2] - cz));
  const typical = median(dist);
  if (typical === 0) return finite;

  const limit = typical * OUTLIER_FACTOR;
  return finite.filter((_, i) => dist[i] <= limit);
}

/** Mean of the kept points, or null when there are none. */
export function hullCentre(points: ReadonlyArray<Point3>): Point3 | null {
  const kept = keepHullPoints(points);
  if (kept.length === 0) return null;
  let x = 0, y = 0, z = 0;
  for (const p of kept) { x += p[0]; y += p[1]; z += p[2]; }
  return [x / kept.length, y / kept.length, z / kept.length];
}
