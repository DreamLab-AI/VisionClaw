/**
 * Timing benchmark at the real snapshot scale (6000 x 384). The numbers are
 * logged, not asserted strictly: CI machines vary too much. The only hard
 * checks are that the build finishes and the query returns a sane result.
 */
import { describe, it, expect } from 'vitest';
import { buildHnsw, searchHnsw, exactTopK, recallAtK } from '../hnsw';
import { buildSearchTree } from '../tree';
import { layoutTree } from '../layout';
import { clusteredVectors, perturbedQueries } from './fixtures';

describe('benchmark 6000 x 384', () => {
  it('builds and runs one traced query', async () => {
    const vs = clusteredVectors(6000, 384, 40, 6000);
    const t0 = performance.now();
    const g = await buildHnsw(vs, { M: 12, efConstruction: 80, seed: 1 });
    const t1 = performance.now();
    const queries = perturbedQueries(vs, 20, 2);
    const t2 = performance.now();
    const r = searchHnsw(g, vs, queries[0], 48, 10, { trace: true });
    const t3 = performance.now();
    const tree = buildSearchTree(r, g.ep);
    const t4 = performance.now();
    for (const view of ['canopy', 'tree', 'hyper'] as const) layoutTree(tree, { view, radius: 100 });
    const t5 = performance.now();
    const ex = exactTopK(vs, queries[0], 10);
    const t6 = performance.now();
    let recall = 0;
    for (const q of queries) recall += recallAtK(searchHnsw(g, vs, q, 48, 10).top, exactTopK(vs, q, 10), 10) / queries.length;

    const report = {
      buildMs: +(t1 - t0).toFixed(1),
      tracedQueryMs: +(t3 - t2).toFixed(2),
      traceEvents: r.trace.length,
      distanceEvals: r.distanceEvals,
      treeMs: +(t4 - t3).toFixed(2),
      treeNodes: tree.nodes.size,
      threeLayoutsMs: +(t5 - t4).toFixed(2),
      exactTopKMs: +(t6 - t5).toFixed(2),
      maxLayer: g.maxL,
      meanRecallAt10ef48: +recall.toFixed(3),
    };
    console.log('[memoryTrajectory benchmark]', JSON.stringify(report));
    expect(r.top.length).toBe(10);
    expect(ex.length).toBe(10);
    expect(recall).toBeGreaterThan(0.8);
  }, 300_000);
});
