import { describe, it, expect, beforeAll } from 'vitest';
import { buildHnsw, searchHnsw } from '../hnsw';
import { buildSearchTree } from '../tree';
import type { HnswGraph, SearchResult, SearchTree } from '../types';
import { clusteredVectors, perturbedQueries } from './fixtures';

describe('buildSearchTree', () => {
  const vs = clusteredVectors(1500, 32, 16, 77);
  let g: HnswGraph;
  const runs: { result: SearchResult; tree: SearchTree }[] = [];

  beforeAll(async () => {
    g = await buildHnsw(vs, { M: 12, efConstruction: 80, seed: 5 });
    for (const q of perturbedQueries(vs, 8, 12)) {
      const result = searchHnsw(g, vs, q, 48, 10, { trace: true });
      runs.push({ result, tree: buildSearchTree(result, g.ep) });
    }
  });

  it('roots the tree at the entry point', () => {
    for (const { tree } of runs) {
      expect(tree.root).toBe(g.ep);
      const root = tree.nodes.get(g.ep)!;
      expect(root.parent).toBe(-1);
      expect(root.depth).toBe(0);
      expect(root.kept).toBe(true);
      expect(root.order).toBe(0);
      expect(tree.list[0]).toBe(root);
    }
  });

  it('builds a path from the root to top[0]', () => {
    for (const { result, tree } of runs) {
      expect(tree.path[0]).toBe(g.ep);
      expect(tree.path[tree.path.length - 1]).toBe(result.top[0]);
      for (let i = 1; i < tree.path.length; i++) {
        expect(tree.nodes.get(tree.path[i])!.parent).toBe(tree.path[i - 1]);
      }
    }
  });

  it('marks kept and rejected nodes exactly as the trace says', () => {
    for (const { result, tree } of runs) {
      const kept = new Set<number>([g.ep]);
      const seen = new Set<number>([g.ep]);
      for (const e of result.trace) {
        if (e.t === 'pop') kept.add(e.n);
        else {
          seen.add(e.n);
          if (e.ok) kept.add(e.n);
        }
      }
      expect(tree.nodes.size).toBe(seen.size);
      for (const n of tree.list) expect(n.kept).toBe(kept.has(n.id));
      expect(tree.keptTotal).toBe(Math.max(1, [...tree.nodes.values()].filter((n) => n.kept).length));
    }
  });

  it('keeps parent links, depths, order and ranks consistent', () => {
    for (const { tree } of runs) {
      let lastOrder = -1;
      let rank = 0;
      for (const n of tree.list) {
        expect(n.order).toBeGreaterThan(lastOrder);
        lastOrder = n.order;
        if (n.kept) expect(n.rank).toBe(rank++);
        else expect(n.rank).toBe(-1);
        if (n.parent !== -1) {
          const p = tree.nodes.get(n.parent)!;
          expect(n.depth).toBe(p.depth + 1);
          expect(p.children).toContain(n);
          expect(p.order).toBeLessThan(n.order);
        }
        expect(n.hash).toBeGreaterThanOrEqual(0);
        expect(n.hash).toBeLessThan(1);
        expect(Number.isFinite(n.leafX)).toBe(true);
        expect(Number.isFinite(n.angle)).toBe(true);
      }
    }
  });

  it('sizes subtrees and sectors from kept children', () => {
    for (const { tree } of runs) {
      for (const n of tree.list) {
        const keptKids = n.children.filter((c) => c.kept);
        if (!n.kept) {
          expect(n.subtreeSize).toBe(1);
          continue;
        }
        expect(n.subtreeSize).toBe(1 + keptKids.reduce((s, c) => s + c.subtreeSize, 0));
        for (const c of keptKids) {
          expect(Math.abs(c.angle)).toBeLessThanOrEqual(1.42 + 1e-9);
        }
      }
      expect(tree.nodes.get(tree.root)!.subtreeSize).toBe(tree.keptTotal);
      const keptLeaves = tree.list.filter((n) => n.kept && !n.children.some((c) => c.kept)).length;
      expect(tree.leaves).toBe(Math.max(2, keptLeaves));
      expect(tree.maxDepth).toBe(Math.max(1, ...tree.list.filter((n) => n.kept).map((n) => n.depth)));
    }
  });

  it('marks learned nodes from hint evals', () => {
    const q = perturbedQueries(vs, 1, 300)[0];
    const result = searchHnsw(g, vs, q, 32, 10, { trace: true, hints: [17, 42] });
    const tree = buildSearchTree(result, g.ep);
    const learnedIds = tree.list.filter((n) => n.learned).map((n) => n.id);
    const hinted = result.trace.filter((e) => e.t === 'eval' && e.learned).map((e) => e.n);
    expect(learnedIds.sort()).toEqual(hinted.sort());
    for (const id of learnedIds) expect(tree.nodes.get(id)!.parent).toBe(g.ep);
  });

  it('copes with an empty trace', () => {
    const empty: SearchResult = { top: [], beam: [], hops: [], distanceEvals: 0, trace: [] };
    const tree = buildSearchTree(empty, 3);
    expect(tree.nodes.size).toBe(1);
    expect(tree.path).toEqual([3]);
    expect(tree.leaves).toBe(2);
    expect(tree.keptTotal).toBe(1);
  });
});
