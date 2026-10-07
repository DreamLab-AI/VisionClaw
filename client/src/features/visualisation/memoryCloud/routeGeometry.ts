/**
 * Buffer writers for the trajectory meshes. The route ribbon and comet tail
 * are tubes rather than drei `Line` / `Line2`: Line2's instanced geometry
 * issues drawIndexed(Infinity) under the WebGPU renderer and kills the render
 * pass (see BotsNode.tsx), while a plain indexed mesh with vertex colours
 * renders on both backends and feeds the bloom pass like any other bright
 * surface.
 *
 * Writers fill preallocated Float32Arrays so a growing route or a moving
 * comet updates in place each frame without allocating.
 */

import type { Vec3 } from '../memoryTrajectory/types';

export const tubeVertexCount = (rings: number, radial: number): number => rings * radial;

/** Triangle indices for `rings` rings of `radial` vertices, closed around. */
export function tubeIndices(rings: number, radial: number): Uint32Array {
  const out = new Uint32Array(Math.max(0, rings - 1) * radial * 6);
  let o = 0;
  for (let i = 0; i < rings - 1; i++) {
    for (let k = 0; k < radial; k++) {
      const a = i * radial + k;
      const b = i * radial + ((k + 1) % radial);
      const c = (i + 1) * radial + k;
      const d = (i + 1) * radial + ((k + 1) % radial);
      out[o++] = a; out[o++] = c; out[o++] = b;
      out[o++] = b; out[o++] = c; out[o++] = d;
    }
  }
  return out;
}

const normalise = (v: Vec3): Vec3 => {
  const l = Math.hypot(v[0], v[1], v[2]);
  return l > 1e-12 ? [v[0] / l, v[1] / l, v[2] / l] : [0, 0, 0];
};
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/**
 * Write a tube along `pts` into `pos` / `col` (both sized for `capacity`
 * rings). Frames are parallel-transported from the first tangent so the tube
 * never twists. Rings past `pts.length` collapse onto the last point with
 * black colour; with additive blending they draw nothing.
 */
export function writeTube(
  pts: Vec3[],
  capacity: number,
  radial: number,
  radius: (i: number) => number,
  colour: (i: number) => Vec3,
  pos: Float32Array,
  col: Float32Array,
): void {
  const n = Math.min(pts.length, capacity);
  if (n === 0) {
    pos.fill(0);
    col.fill(0);
    return;
  }
  const tangentAt = (i: number): Vec3 => {
    const a = pts[Math.max(0, i - 1)];
    const b = pts[Math.min(n - 1, i + 1)];
    return normalise([b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
  };
  let t = tangentAt(0);
  if (dot(t, t) === 0) t = [1, 0, 0];
  // initial normal: any vector not parallel to t
  const seed: Vec3 = Math.abs(t[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
  let nrm = normalise(cross(cross(t, seed), t));
  for (let i = 0; i < capacity; i++) {
    const p = pts[Math.min(i, n - 1)];
    if (i >= n) {
      for (let k = 0; k < radial; k++) {
        const o = (i * radial + k) * 3;
        pos[o] = p[0]; pos[o + 1] = p[1]; pos[o + 2] = p[2];
        col[o] = 0; col[o + 1] = 0; col[o + 2] = 0;
      }
      continue;
    }
    if (i > 0) {
      const t2 = tangentAt(i);
      if (dot(t2, t2) > 0) {
        // parallel transport: remove the new tangent's component from the old normal
        const proj = dot(nrm, t2);
        const nn = normalise([nrm[0] - proj * t2[0], nrm[1] - proj * t2[1], nrm[2] - proj * t2[2]]);
        if (dot(nn, nn) > 0) nrm = nn;
        t = t2;
      }
    }
    const bin = normalise(cross(t, nrm));
    const r = radius(i);
    const c = colour(i);
    for (let k = 0; k < radial; k++) {
      const a = (k / radial) * Math.PI * 2;
      const ca = Math.cos(a) * r;
      const sa = Math.sin(a) * r;
      const o = (i * radial + k) * 3;
      pos[o] = p[0] + nrm[0] * ca + bin[0] * sa;
      pos[o + 1] = p[1] + nrm[1] * ca + bin[1] * sa;
      pos[o + 2] = p[2] + nrm[2] * ca + bin[2] * sa;
      col[o] = c[0]; col[o + 1] = c[1]; col[o + 2] = c[2];
    }
  }
}

export const edgeSegmentCount = (polys: Vec3[][]): number =>
  polys.reduce((s, p) => s + Math.max(0, p.length - 1), 0);

/**
 * Write polylines as LineSegments pairs. Returns the number of segments
 * written; the caller sets the draw range to `2 * segments` vertices.
 */
export function writeEdgeSegments(
  polys: Vec3[][],
  colour: (edge: number) => Vec3,
  pos: Float32Array,
  col: Float32Array,
): number {
  let s = 0;
  const cap = pos.length / 6;
  for (let e = 0; e < polys.length; e++) {
    const p = polys[e];
    const c = colour(e);
    for (let i = 0; i < p.length - 1 && s < cap; i++, s++) {
      const o = s * 6;
      pos[o] = p[i][0]; pos[o + 1] = p[i][1]; pos[o + 2] = p[i][2];
      pos[o + 3] = p[i + 1][0]; pos[o + 4] = p[i + 1][1]; pos[o + 5] = p[i + 1][2];
      col[o] = c[0]; col[o + 1] = c[1]; col[o + 2] = c[2];
      col[o + 3] = c[0]; col[o + 4] = c[1]; col[o + 5] = c[2];
    }
  }
  return s;
}
