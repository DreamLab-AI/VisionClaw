import { describe, it, expect } from 'vitest';
import { keepHullPoints } from '../hullOutliers';

/**
 * A community hull outlines where a theme lives. The display places page nodes
 * and ontology classes in separate bodies, so a core with a handful of members
 * in the other body (measured live: 7 of 1,150, at 383 units when 95% sit within
 * 60) stretched one hull into a slab across the whole scene.
 */
describe('keepHullPoints', () => {
  const cloud = (n: number, cx: number, r: number): Array<[number, number, number]> =>
    Array.from({ length: n }, (_, i) => {
      const a = (i / n) * Math.PI * 2;
      const rr = r * (0.3 + 0.7 * ((i * 37) % 100) / 100);
      return [cx + Math.cos(a) * rr, Math.sin(a) * rr, ((i * 13) % 20) - 10];
    });

  it('drops a few members that sit far outside the body of the community', () => {
    const body = cloud(400, 0, 60);
    const strays: Array<[number, number, number]> = [[380, 5, 0], [390, -8, 2], [400, 0, -3]];
    const kept = keepHullPoints([...body, ...strays]);
    expect(kept.length).toBe(400);
    expect(Math.max(...kept.map((p) => p[0]))).toBeLessThan(100);
  });

  it('keeps every member of a compact community', () => {
    const body = cloud(300, 50, 40);
    expect(keepHullPoints(body).length).toBe(300);
  });

  it('keeps an elongated community whole (no member is far relative to its own spread)', () => {
    const line: Array<[number, number, number]> = Array.from({ length: 200 }, (_, i) => [i * 2, (i % 5) - 2, 0]);
    expect(keepHullPoints(line).length).toBe(200);
  });

  it('leaves tiny sets and coincident points alone', () => {
    expect(keepHullPoints([[0, 0, 0], [1, 0, 0], [900, 0, 0]]).length).toBe(3);
    const same: Array<[number, number, number]> = Array.from({ length: 20 }, () => [5, 5, 5]);
    expect(keepHullPoints(same).length).toBe(20);
  });

  it('ignores non-finite positions', () => {
    const body = cloud(50, 0, 10);
    expect(keepHullPoints([...body, [NaN, 0, 0], [Infinity, 1, 1]]).length).toBe(50);
  });
});
