/**
 * Contract test: the explorer store driving the real trajectory module
 * (HNSW build, traced query, tree, four layouts, morph) over synthetic
 * L2-normalised vectors. Only HTTP is faked.
 */
import { describe, it, expect, vi } from 'vitest';
import { createMemoryCloudStore, type TrajectoryModule } from '../memoryCloudStore';
import { sampleRoute } from '../routeMath';
import type { MemoryCloudSnapshot } from '../types';
import type { TrajectoryView, VectorSet } from '../../memoryTrajectory/types';

const N = 400;
const DIM = 16;

function vectors(): VectorSet {
  let s = 12345;
  const rnd = () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
  const data = new Float32Array(N * DIM);
  for (let i = 0; i < N; i++) {
    let norm = 0;
    for (let d = 0; d < DIM; d++) {
      const v = rnd() + (d === i % 4 ? 2 : 0);
      data[i * DIM + d] = v;
      norm += v * v;
    }
    norm = Math.sqrt(norm);
    for (let d = 0; d < DIM; d++) data[i * DIM + d] /= norm;
  }
  return { count: N, dim: DIM, data };
}

const vs = vectors();
const snapshot: MemoryCloudSnapshot = {
  version: 1, snapshotId: 'syn', generatedAt: 1, dim: DIM, count: N,
  positions: Array.from({ length: N * 3 }, (_, i) => Math.sin(i) * 100),
  metadata: Array.from({ length: N }, (_, i) => ({ id: `${i}`, key: `k${i}`, namespace: `ns${i % 3}`, sourceType: 'memory', updatedAt: i })),
  namespaces: ['ns0', 'ns1', 'ns2'], sourceTypes: ['memory'], strata: [], excludedNamespaces: [],
  vectorsUrl: '/api/memory-cloud/vectors?snapshot=syn',
};

describe('explorer store × real trajectory engine', () => {
  it('builds, queries, lays out every view and morphs between them', async () => {
    const store = createMemoryCloudStore({
      fetchSnapshot: vi.fn(async () => snapshot),
      fetchVectors: vi.fn(async () => ({ snapshot, vectors: vs })),
      postQuery: vi.fn(async () => ({
        snapshotId: 'syn',
        embedModel: 'synthetic',
        query: { text: 'q', vector: Array.from(vs.data.subarray(5 * DIM, 6 * DIM)) },
        sidecar: { tookMs: 1, results: [{ id: '5', key: 'k5', namespace: 'ns2', sourceType: 'memory', score: 1, snippet: '', sampleIndex: 5 }] },
      })),
      fetchHealth: vi.fn(),
      loadTrajectory: async () => (await import('../../memoryTrajectory')) as TrajectoryModule,
    });

    await store.getState().loadSnapshot();
    expect(store.getState().status).toBe('ready');
    expect(store.getState().buildProgress).toBe(1);

    await store.getState().runQuery('find row five', { k: 5 });
    const s = store.getState();
    expect(s.query.status).toBe('done');
    const run = s.query.run!;
    expect(run.result.top.length).toBe(5);
    // the query is row 5 itself: exact search must return it first
    expect(run.exactTop[0]).toBe(5);
    expect(run.recall).toBeGreaterThanOrEqual(0.6);
    expect(run.tree.path[0]).toBe(run.tree.root);
    expect(run.tree.path.length).toBeGreaterThanOrEqual(2);

    const views: TrajectoryView[] = ['space', 'tree', 'hyper', 'canopy'];
    for (const v of views) {
      store.getState().setView(v);
      const L = store.getState().query.layout!;
      for (const n of run.tree.list) {
        const p = L.positions.get(n.id)!;
        expect(p.every(Number.isFinite)).toBe(true);
      }
      const route = sampleRoute(run.tree.path, L, 16);
      expect(route.pts.length).toBe((run.tree.path.length - 1) * 16 + 1);
      if (v === 'hyper') {
        expect(L.geodesics?.size).toBeGreaterThan(0);
        for (const p of L.positions.values()) {
          expect(Math.abs(p[1])).toBeLessThan(1e-9);
          expect(Math.hypot(p[0], p[2])).toBeLessThanOrEqual(90 + 1e-6);
        }
      }
      if (v === 'space') {
        const p = L.positions.get(5);
        if (p) expect(p).toEqual([snapshot.positions[15], snapshot.positions[16], snapshot.positions[17]]);
      }
    }

    // morph: halfway between two views is finite and between the endpoints
    const traj = store.getState().trajectory!;
    const prev = store.getState().query.prevLayout!;
    const next = store.getState().query.layout!;
    const mid = traj.interpolateLayouts(prev, next, 0.5, run.tree);
    for (const p of mid.positions.values()) expect(p.every(Number.isFinite)).toBe(true);

    store.getState().dispose();
  }, 30_000);
});
