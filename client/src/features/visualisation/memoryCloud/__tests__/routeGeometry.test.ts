import { describe, it, expect } from 'vitest';
import { tubeIndices, writeTube, tubeVertexCount, writeEdgeSegments } from '../routeGeometry';
import type { Vec3 } from '../../memoryTrajectory/types';

describe('tube geometry', () => {
  it('builds a closed-ring index buffer: 6 indices per quad', () => {
    const idx = tubeIndices(3, 4);
    expect(idx.length).toBe((3 - 1) * 4 * 6);
    expect(Math.max(...idx)).toBe(3 * 4 - 1);
  });

  it('puts each ring on its centre point at the requested radius, perpendicular to the path', () => {
    const pts: Vec3[] = [[0, 0, 0], [1, 0, 0], [2, 1, 0], [2, 2, 1]];
    const radial = 8;
    const pos = new Float32Array(tubeVertexCount(pts.length, radial) * 3);
    const col = new Float32Array(pos.length);
    writeTube(pts, pts.length, radial, (i) => 0.1 * (i + 1), (i) => [i / 3, 0, 1 - i / 3], pos, col);
    for (let i = 0; i < pts.length; i++) {
      let cx = 0, cy = 0, cz = 0;
      for (let k = 0; k < radial; k++) {
        const o = (i * radial + k) * 3;
        cx += pos[o] / radial; cy += pos[o + 1] / radial; cz += pos[o + 2] / radial;
        const d = Math.hypot(pos[o] - pts[i][0], pos[o + 1] - pts[i][1], pos[o + 2] - pts[i][2]);
        expect(d).toBeCloseTo(0.1 * (i + 1), 5);
      }
      expect(cx).toBeCloseTo(pts[i][0], 5);
      expect(cy).toBeCloseTo(pts[i][1], 5);
      expect(cz).toBeCloseTo(pts[i][2], 5);
      expect(col[i * radial * 3]).toBeCloseTo(i / 3, 6);
    }
    // first ring is perpendicular to the first tangent (+x)
    for (let k = 0; k < radial; k++) expect(pos[k * 3]).toBeCloseTo(0, 5);
  });

  it('collapses unused rings onto the last point so a shorter route draws nothing extra', () => {
    const pts: Vec3[] = [[0, 0, 0], [1, 0, 0]];
    const pos = new Float32Array(tubeVertexCount(4, 4) * 3).fill(99);
    const col = new Float32Array(pos.length);
    writeTube(pts, 4, 4, () => 1, () => [1, 1, 1], pos, col);
    for (let v = 2 * 4; v < 4 * 4; v++) {
      expect([pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]]).toEqual([1, 0, 0]);
      expect(col[v * 3]).toBe(0);
    }
  });

  it('survives a degenerate (repeated-point) path without NaNs', () => {
    const pts: Vec3[] = [[1, 1, 1], [1, 1, 1], [1, 1, 1]];
    const pos = new Float32Array(tubeVertexCount(3, 6) * 3);
    writeTube(pts, 3, 6, () => 0.5, () => [1, 1, 1], pos, new Float32Array(pos.length));
    expect(pos.every(Number.isFinite)).toBe(true);
  });
});

describe('edge segments', () => {
  it('writes each polyline as consecutive line-segment pairs', () => {
    const polys: Vec3[][] = [[[0, 0, 0], [1, 0, 0], [2, 0, 0]], [[0, 1, 0], [0, 2, 0]]];
    const pos = new Float32Array(3 * 2 * 3);
    const col = new Float32Array(pos.length);
    const n = writeEdgeSegments(polys, (e) => (e === 0 ? [1, 0, 0] : [0, 0, 1]), pos, col);
    expect(n).toBe(3);
    expect(Array.from(pos.slice(0, 6))).toEqual([0, 0, 0, 1, 0, 0]);
    expect(Array.from(pos.slice(6, 12))).toEqual([1, 0, 0, 2, 0, 0]);
    expect(Array.from(pos.slice(12, 18))).toEqual([0, 1, 0, 0, 2, 0]);
    expect(Array.from(col.slice(12, 15))).toEqual([0, 0, 1]);
  });
});
