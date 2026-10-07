import { describe, it, expect, beforeAll } from 'vitest';
import { buildHnsw, searchHnsw, exactTopK, recallAtK } from '../hnsw';
import { packGraph, unpackGraph } from '../graphCodec';
import type { HnswGraph, HnswParams, VectorSet } from '../types';
import { clusteredVectors, perturbedQueries, naiveTopK } from './fixtures';

const PARAMS: HnswParams = { M: 12, efConstruction: 80, seed: 1234 };

function meanRecall(g: HnswGraph, vs: VectorSet, queries: Float32Array[], ef: number, k: number): number {
  let sum = 0;
  for (const q of queries) {
    const r = searchHnsw(g, vs, q, ef, k);
    sum += recallAtK(r.top, exactTopK(vs, q, k), k);
  }
  return sum / queries.length;
}

describe('buildHnsw / searchHnsw on clustered 2000 x 32', () => {
  const vs = clusteredVectors(2000, 32, 24, 7);
  let g: HnswGraph;
  beforeAll(async () => {
    g = await buildHnsw(vs, PARAMS);
  });

  it('reaches recall@10 >= 0.95 at ef = 64', () => {
    const r = meanRecall(g, vs, perturbedQueries(vs, 100, 99), 64, 10);
    expect(r).toBeGreaterThanOrEqual(0.95);
  });

  it('builds a structurally valid graph', () => {
    expect(g.levels.length).toBe(vs.count);
    expect(g.links.length).toBe(vs.count);
    expect(g.levels[g.ep]).toBe(g.maxL);
    for (let i = 0; i < vs.count; i++) {
      expect(g.links[i].length).toBe(g.levels[i] + 1);
      for (let l = 0; l <= g.levels[i]; l++) {
        const cap = l === 0 ? 2 * PARAMS.M : PARAMS.M;
        expect(g.links[i][l].length).toBeLessThanOrEqual(cap);
        for (const n of g.links[i][l]) {
          expect(n).not.toBe(i);
          expect(g.levels[n]).toBeGreaterThanOrEqual(l);
        }
        expect(new Set(g.links[i][l]).size).toBe(g.links[i][l].length);
      }
    }
  });

  it('is deterministic for a fixed seed and differs for another seed', async () => {
    const again = await buildHnsw(vs, PARAMS);
    expect(again.ep).toBe(g.ep);
    expect(again.maxL).toBe(g.maxL);
    expect(Array.from(again.levels)).toEqual(Array.from(g.levels));
    expect(again.links).toEqual(g.links);
    const other = await buildHnsw(vs, { ...PARAMS, seed: 99 });
    expect(Array.from(other.levels)).not.toEqual(Array.from(g.levels));
  });

  it('reports progress and finishes at total', async () => {
    const seen: [number, number][] = [];
    await buildHnsw(clusteredVectors(300, 16, 4, 3), PARAMS, {
      yieldEvery: 50,
      onProgress: (d, t) => seen.push([d, t]),
    });
    expect(seen.length).toBeGreaterThan(2);
    expect(seen[seen.length - 1]).toEqual([300, 300]);
    for (let i = 1; i < seen.length; i++) expect(seen[i][0]).toBeGreaterThanOrEqual(seen[i - 1][0]);
  });

  it('rejects when aborted', async () => {
    const ac = new AbortController();
    const p = buildHnsw(vs, PARAMS, { signal: ac.signal, yieldEvery: 64 });
    ac.abort();
    await expect(p).rejects.toThrow(/abort/i);
  });

  it('records a trace in which every eval.from was popped earlier or is the entry', () => {
    for (const q of perturbedQueries(vs, 10, 5)) {
      const r = searchHnsw(g, vs, q, 48, 10, { trace: true });
      expect(r.trace.length).toBeGreaterThan(0);
      const popped = new Set<number>();
      for (const e of r.trace) {
        if (e.t === 'pop') popped.add(e.n);
        else expect(popped.has(e.from) || e.from === g.ep).toBe(true);
      }
      expect(r.top.length).toBe(10);
      expect(r.beam.length).toBe(48);
      expect(r.beam.slice(0, 10)).toEqual(r.top);
      expect(r.hops.length).toBe(g.maxL);
      expect(r.distanceEvals).toBeGreaterThan(0);
    }
  });

  it('leaves the trace empty unless requested', () => {
    const r = searchHnsw(g, vs, perturbedQueries(vs, 1, 3)[0], 32, 5);
    expect(r.trace).toEqual([]);
  });

  it('injects hints as learned evals from the entry point', () => {
    const q = perturbedQueries(vs, 1, 11)[0];
    const truth = exactTopK(vs, q, 2);
    const r = searchHnsw(g, vs, q, 16, 10, { trace: true, hints: [...truth, -1, vs.count + 5] });
    const learned = r.trace.filter((e) => e.t === 'eval' && e.learned);
    expect(learned.length).toBeGreaterThan(0);
    for (const e of learned) {
      if (e.t !== 'eval') continue;
      expect(e.from).toBe(g.ep);
      expect(e.l).toBe(0);
      expect(truth).toContain(e.n);
    }
    expect(r.top).toContain(truth[0]);
  });

  it('returns top results ordered nearest first', () => {
    const q = perturbedQueries(vs, 1, 21)[0];
    const r = searchHnsw(g, vs, q, 64, 10);
    const d = r.top.map((i) => {
      let s = 0;
      for (let k = 0; k < vs.dim; k++) s += q[k] * vs.data[i * vs.dim + k];
      return 1 - s;
    });
    for (let i = 1; i < d.length; i++) expect(d[i]).toBeGreaterThanOrEqual(d[i - 1] - 1e-7);
  });
});

describe('buildHnsw on 600 x 384', () => {
  it('reaches recall@10 >= 0.95 at ef = 64', async () => {
    const vs = clusteredVectors(600, 384, 12, 17);
    const g = await buildHnsw(vs, PARAMS);
    expect(meanRecall(g, vs, perturbedQueries(vs, 60, 8), 64, 10)).toBeGreaterThanOrEqual(0.95);
  });
});

describe('edge-case sets', () => {
  it('handles an empty set', async () => {
    const vs: VectorSet = { count: 0, dim: 8, data: new Float32Array(0) };
    const g = await buildHnsw(vs, PARAMS);
    expect(g.ep).toBe(-1);
    const r = searchHnsw(g, vs, new Float32Array(8), 10, 5, { trace: true });
    expect(r.top).toEqual([]);
    expect(exactTopK(vs, new Float32Array(8), 3)).toEqual([]);
  });

  it('handles a single vector and k larger than the set', async () => {
    const vs = clusteredVectors(3, 8, 1, 2);
    const g = await buildHnsw(vs, PARAMS);
    const r = searchHnsw(g, vs, vs.data.slice(0, 8), 10, 10);
    expect(r.top.sort()).toEqual([0, 1, 2]);
  });

  it('rejects mismatched data length', async () => {
    await expect(buildHnsw({ count: 4, dim: 8, data: new Float32Array(10) }, PARAMS)).rejects.toThrow();
  });
});

describe('exactTopK', () => {
  it('matches a naive sort', () => {
    const vs = clusteredVectors(500, 24, 6, 31);
    for (const q of perturbedQueries(vs, 20, 4)) {
      for (const k of [1, 5, 10, 37]) expect(exactTopK(vs, q, k)).toEqual(naiveTopK(vs, q, k));
    }
  });

  it('clamps k to the set size and returns [] for k <= 0', () => {
    const vs = clusteredVectors(4, 8, 1, 3);
    const q = vs.data.slice(0, 8);
    expect(exactTopK(vs, q, 10).length).toBe(4);
    expect(exactTopK(vs, q, 0)).toEqual([]);
    expect(exactTopK(vs, q, -3)).toEqual([]);
  });
});

describe('recallAtK', () => {
  it('counts the overlap of the first k entries', () => {
    expect(recallAtK([1, 2, 3, 4], [4, 3, 2, 1], 4)).toBe(1);
    expect(recallAtK([1, 2, 9, 8], [1, 2, 3, 4], 4)).toBe(0.5);
    expect(recallAtK([1, 2, 3, 4], [1, 2, 3, 4], 2)).toBe(1);
    expect(recallAtK([9, 1], [1, 2], 2)).toBe(0.5);
  });

  it('does not double-count duplicates in the approximate list', () => {
    expect(recallAtK([1, 1, 1], [1, 2, 3], 3)).toBeCloseTo(1 / 3);
  });

  it('divides by the attainable size when exact is shorter than k', () => {
    expect(recallAtK([0, 1], [1, 0], 10)).toBe(1);
    expect(recallAtK([0], [1, 0], 10)).toBe(0.5);
  });

  it('is vacuously complete for k <= 0 or an empty truth', () => {
    expect(recallAtK([], [], 10)).toBe(1);
    expect(recallAtK([1, 2], [1, 2], 0)).toBe(1);
  });

  it('is 0 when nothing was found', () => {
    expect(recallAtK([], [1, 2, 3], 3)).toBe(0);
  });
});

describe('graph codec', () => {
  it('round-trips a built graph through flat typed arrays', async () => {
    const vs = clusteredVectors(400, 16, 5, 41);
    const g = await buildHnsw(vs, PARAMS);
    const packed = packGraph(g);
    expect(packed.levels).toBeInstanceOf(Uint8Array);
    expect(packed.links).toBeInstanceOf(Int32Array);
    const back = unpackGraph(packed);
    expect(back.ep).toBe(g.ep);
    expect(back.maxL).toBe(g.maxL);
    expect(back.params).toEqual(g.params);
    expect(Array.from(back.levels)).toEqual(Array.from(g.levels));
    expect(back.links).toEqual(g.links);
  });

  it('rejects a truncated payload', async () => {
    const g = await buildHnsw(clusteredVectors(50, 8, 2, 1), PARAMS);
    const packed = packGraph(g);
    expect(() => unpackGraph({ ...packed, links: packed.links.subarray(0, packed.links.length - 3) })).toThrow();
  });
});
