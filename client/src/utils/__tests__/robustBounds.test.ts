import { describe, it, expect } from 'vitest';
import { robustBounds } from '../robustBounds';

const flat = (pts: number[][]) => Float32Array.from(pts.flat());

describe('robustBounds', () => {
  it('is null for no points', () => {
    expect(robustBounds(new Float32Array(0), 0)).toBeNull();
    expect(robustBounds(new Float32Array(3), 0)).toBeNull();
  });

  it('ignores a few far outliers (5th–95th percentile per axis)', () => {
    const pts: number[][] = [];
    for (let i = 0; i < 100; i++) pts.push([(i % 10) * 10 - 45 + 100, Math.floor(i / 10) * 10 - 45, 0]);
    pts.push([5000, 5000, 5000], [-4000, 0, 0]);
    const b = robustBounds(flat(pts), pts.length)!;
    expect(b.centre[0]).toBeCloseTo(100, 0);
    expect(b.centre[1]).toBeCloseTo(0, 0);
    expect(b.radius).toBeLessThan(80);
    expect(b.radius).toBeGreaterThan(50);
  });

  it('reads only the first `count` rows of a larger buffer', () => {
    const buf = new Float32Array(30);
    buf.set([10, 10, 10, 12, 12, 12]);
    const b = robustBounds(buf, 2)!;
    expect(b.centre).toEqual([11, 11, 11]);
  });

  it('floors the radius at 1 for coincident points and accepts plain arrays', () => {
    const b = robustBounds([3, 4, 5, 3, 4, 5], 2)!;
    expect(b.centre).toEqual([3, 4, 5]);
    expect(b.radius).toBe(1);
  });

  it('skips non-finite rows', () => {
    const b = robustBounds([NaN, 0, 0, 1, 1, 1, 3, 3, 3], 3)!;
    expect(b.centre).toEqual([2, 2, 2]);
  });
});
