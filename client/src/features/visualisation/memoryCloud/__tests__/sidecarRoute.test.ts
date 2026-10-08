import { describe, expect, it } from 'vitest';
import { placeHit, sidecarRoute, routeLegend } from '../sidecarRoute';
import type { MemoryCloudHit, MemoryCloudQueryResponse } from '../types';

// Three snapshot rows: 0 at (1,2,3), 1 at (4,5,6), 2 at (7,8,9).
const POSITIONS = Float32Array.from([1, 2, 3, 4, 5, 6, 7, 8, 9]);

const hit = (key: string, sampleIndex: number | null, position?: [number, number, number] | null): MemoryCloudHit => ({
  id: key,
  key,
  namespace: 'project-state',
  sourceType: 'memory',
  score: 0.8,
  snippet: '',
  sampleIndex,
  ...(position === undefined ? {} : { position }),
});

const resp = (hits: MemoryCloudHit[], position?: [number, number, number] | null): MemoryCloudQueryResponse => ({
  snapshotId: 's',
  embedModel: 'bge-small-en-v1.5',
  query: { text: 'q', vector: [1, 0, 0], ...(position === undefined ? {} : { position }) },
  sidecar: { results: hits, tookMs: 4, method: 'hnsw' },
});

describe('placeHit', () => {
  it('uses the hit position the server projected', () => {
    expect(placeHit(hit('a', null, [9, 9, 9]), POSITIONS)).toEqual([9, 9, 9]);
  });

  it('falls back to the sampled row for an older server', () => {
    expect(placeHit(hit('a', 1), POSITIONS)).toEqual([4, 5, 6]);
  });

  it('places nothing for an unsampled hit without a position, or a row out of range', () => {
    expect(placeHit(hit('a', null), POSITIONS)).toBeNull();
    expect(placeHit(hit('a', null, null), POSITIONS)).toBeNull();
    expect(placeHit(hit('a', 3), POSITIONS)).toBeNull();
  });

  it('rejects a non-finite position', () => {
    expect(placeHit(hit('a', null, [Number.NaN, 0, 0]), POSITIONS)).toBeNull();
  });
});

describe('sidecarRoute', () => {
  it('runs from the query point through every placed hit in rank order', () => {
    const r = sidecarRoute(
      resp([hit('a', 0, [1, 2, 3]), hit('ghost', null, [20, 0, 0]), hit('lost', null, null), hit('c', 2, [7, 8, 9])], [0, 0, 0]),
      POSITIONS,
    );
    expect(r.origin).toEqual([0, 0, 0]);
    expect(r.stops.map((s) => [s.rank, s.sampled])).toEqual([
      [1, true],
      [2, false],
      [4, true],
    ]);
    expect(r.points).toEqual([
      [0, 0, 0],
      [1, 2, 3],
      [20, 0, 0],
      [7, 8, 9],
    ]);
    expect([r.total, r.inSample, r.drawn]).toEqual([4, 2, 3]);
    expect(r.ghosts.map((s) => s.pos)).toEqual([[20, 0, 0]]);
  });

  it('draws a global query that has no hit in the sample', () => {
    const r = sidecarRoute(resp([hit('x', null, [1, 1, 1]), hit('y', null, [2, 2, 2]), hit('z', null, [3, 3, 3])], [0, 0, 0]), POSITIONS);
    expect(r.points).toHaveLength(4);
    expect(r.ghosts).toHaveLength(3);
    expect(r.caption).toBe('route: query point → sidecar top-k (3 drawn, 0 in sample; not a search path)');
  });

  it('keeps the sampled-row route when the server sends no hit positions', () => {
    const withOrigin = sidecarRoute(resp([hit('a', 0), hit('b', null), hit('c', 2)], [0, 0, 0]), POSITIONS);
    expect(withOrigin.points).toEqual([
      [0, 0, 0],
      [1, 2, 3],
      [7, 8, 9],
    ]);
    expect(withOrigin.ghosts).toEqual([]);
    expect(withOrigin.caption).toBe('route: query point → sidecar top-k (not a search path)');

    const noOrigin = sidecarRoute(resp([hit('a', 0), hit('c', 2)]), POSITIONS);
    expect(noOrigin.origin).toBeNull();
    expect(noOrigin.points).toEqual([
      [1, 2, 3],
      [7, 8, 9],
    ]);
    expect(noOrigin.caption).toBe('route: sidecar top-k in rank order (not a search path)');
  });

  it('says why nothing is drawn', () => {
    expect(sidecarRoute(resp([hit('b', null)], [0, 0, 0]), POSITIONS).caption).toBe('no route: no hit in the sample');
    expect(sidecarRoute(resp([hit('a', 0)]), POSITIONS).caption).toBe('no route: fewer than 2 hits in the sample');
    expect(sidecarRoute(resp([hit('a', 0)]), POSITIONS).points).toEqual([]);
    expect(sidecarRoute(null, POSITIONS).points).toEqual([]);
  });
});

describe('routeLegend', () => {
  it('matches the XR Memory line wording', () => {
    const r = sidecarRoute(resp([hit('a', 0, [1, 2, 3]), hit('g', null, [5, 5, 5])], [0, 0, 0]), POSITIONS);
    expect(routeLegend(r)).toBe('Route: query point → sidecar top-k (2 drawn, 1 in sample)');
    expect(routeLegend(sidecarRoute(resp([hit('a', 0), hit('c', 2)]), POSITIONS))).toBe('Route: sidecar top-k in rank order');
  });
});
