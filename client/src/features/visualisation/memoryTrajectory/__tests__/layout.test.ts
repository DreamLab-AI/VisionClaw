import { describe, it, expect, beforeAll } from 'vitest';
import { buildHnsw, searchHnsw } from '../hnsw';
import { buildSearchTree } from '../tree';
import { layoutTree, mobius, mobiusInverse } from '../layout';
import { interpolateLayouts } from '../morph';
import type { LayoutResult, SearchTree, TrajectoryView, Vec3 } from '../types';
import { clusteredVectors, perturbedQueries, seededRandom } from './fixtures';

const finite = (v: Vec3) => v.length === 3 && v.every((x) => Number.isFinite(x));
const RADIUS = 80;

describe('layoutTree', () => {
  const vs = clusteredVectors(1200, 32, 12, 404);
  let tree: SearchTree;
  let spacePositions: Float32Array;

  beforeAll(async () => {
    const g = await buildHnsw(vs, { M: 12, efConstruction: 80, seed: 8 });
    const r = searchHnsw(g, vs, perturbedQueries(vs, 1, 77)[0], 64, 10, { trace: true, hints: [3, 900] });
    tree = buildSearchTree(r, g.ep);
    const rand = seededRandom(9);
    spacePositions = new Float32Array(vs.count * 3).map(() => (rand() - 0.5) * 200);
  });

  const views: TrajectoryView[] = ['space', 'canopy', 'tree', 'hyper'];
  for (const view of views) {
    it(`${view}: every node gets finite coordinates and every edge a finite control`, () => {
      const L = layoutTree(tree, { view, radius: RADIUS, spacePositions });
      for (const n of tree.list) {
        const p = L.positions.get(n.id);
        expect(p, `node ${n.id}`).toBeDefined();
        expect(finite(p!)).toBe(true);
        if (n.parent !== -1) {
          const c = L.controls.get(n.id);
          expect(c).toBeDefined();
          expect(finite(c!)).toBe(true);
        }
      }
      expect(L.controls.has(tree.root)).toBe(false);
    });
  }

  it('space: places nodes at their cloud positions', () => {
    const L = layoutTree(tree, { view: 'space', radius: RADIUS, spacePositions });
    for (const n of tree.list) {
      expect(L.positions.get(n.id)).toEqual([
        spacePositions[n.id * 3],
        spacePositions[n.id * 3 + 1],
        spacePositions[n.id * 3 + 2],
      ]);
    }
  });

  it('space: falls back to the parent position when a cloud position is missing', () => {
    const short = spacePositions.slice(0, 3 * 10);
    const L = layoutTree(tree, { view: 'space', radius: RADIUS, spacePositions: short });
    for (const n of tree.list) expect(finite(L.positions.get(n.id)!)).toBe(true);
  });

  it('canopy: radius grows monotonically along the route and stays within the dome', () => {
    const L = layoutTree(tree, { view: 'canopy', radius: RADIUS });
    const root = L.positions.get(tree.root)!;
    const dist = (id: number) => {
      const p = L.positions.get(id)!;
      return Math.hypot(p[0] - root[0], p[1] - root[1], p[2] - root[2]);
    };
    for (let i = 1; i < tree.path.length; i++) {
      expect(dist(tree.path[i])).toBeGreaterThan(dist(tree.path[i - 1]));
    }
    for (const n of tree.list) {
      expect(dist(n.id)).toBeLessThanOrEqual(RADIUS * 1.07);
      expect(L.positions.get(n.id)![1]).toBeGreaterThanOrEqual(root[1] - 1e-9);
    }
  });

  it('tree: rows in a vertical plane, rejected nodes one row beyond their parent', () => {
    const L = layoutTree(tree, { view: 'tree', radius: RADIUS });
    const ys = new Map<number, number>();
    for (const n of tree.list) {
      const p = L.positions.get(n.id)!;
      expect(p[2]).toBe(0);
      expect(Math.abs(p[0])).toBeLessThanOrEqual(RADIUS + 1e-9);
      if (ys.has(n.depth)) expect(p[1]).toBeCloseTo(ys.get(n.depth)!, 9);
      else ys.set(n.depth, p[1]);
      if (n.parent !== -1) expect(p[1]).toBeGreaterThan(L.positions.get(n.parent)![1]);
    }
  });

  it('hyper: points lie inside the disk on the XZ plane, geodesic endpoints equal positions', () => {
    for (const hyperFocus of [undefined, [0.3, -0.4] as [number, number], [0.99, 0.5] as [number, number]]) {
      for (const hyperStep of [0.2, 0.55, 0.9, 5]) {
        const L = layoutTree(tree, { view: 'hyper', radius: RADIUS, hyperFocus, hyperStep });
        expect(L.geodesics).toBeDefined();
        for (const n of tree.list) {
          const p = L.positions.get(n.id)!;
          expect(finite(p)).toBe(true);
          expect(p[1]).toBe(0);
          expect(Math.hypot(p[0], p[2])).toBeLessThanOrEqual(RADIUS);
          if (n.parent === -1) continue;
          const geo = L.geodesics!.get(n.id)!;
          expect(geo.length).toBeGreaterThanOrEqual(2);
          expect(geo[0]).toEqual(L.positions.get(n.parent));
          expect(geo[geo.length - 1]).toEqual(p);
          for (const s of geo) {
            expect(finite(s)).toBe(true);
            expect(Math.hypot(s[0], s[2])).toBeLessThanOrEqual(RADIUS * (1 + 1e-9));
          }
        }
      }
    }
  });

  it('hyper: the default focus puts the root at the centre', () => {
    const L = layoutTree(tree, { view: 'hyper', radius: RADIUS });
    const p = L.positions.get(tree.root)!;
    expect(Math.hypot(p[0], p[2])).toBeLessThan(1e-9);
  });

  it('is deterministic', () => {
    for (const view of views) {
      const a = layoutTree(tree, { view, radius: RADIUS, spacePositions });
      const b = layoutTree(tree, { view, radius: RADIUS, spacePositions });
      expect([...a.positions]).toEqual([...b.positions]);
    }
  });
});

describe('Möbius maps', () => {
  it('mobiusInverse undoes mobius and maps the focus to the origin', () => {
    const a: [number, number] = [0.3, -0.2];
    for (const z of [[0, 0], [0.5, 0.1], [-0.7, 0.6]] as [number, number][]) {
      const w = mobiusInverse(mobius(z, a), a);
      expect(w[0]).toBeCloseTo(z[0], 12);
      expect(w[1]).toBeCloseTo(z[1], 12);
      const m = mobius(z, a);
      expect(Math.hypot(m[0], m[1])).toBeLessThan(1);
    }
    const o = mobius(a, a);
    expect(Math.hypot(o[0], o[1])).toBeLessThan(1e-12);
  });
});

describe('interpolateLayouts', () => {
  const a: LayoutResult = {
    positions: new Map<number, Vec3>([[0, [0, 0, 0]], [1, [10, 0, 0]], [2, [0, 10, 0]]]),
    controls: new Map<number, Vec3>([[1, [5, 1, 0]], [2, [0, 5, 1]]]),
  };
  const b: LayoutResult = {
    positions: new Map<number, Vec3>([[0, [0, 0, 0]], [1, [20, 0, 0]], [3, [0, 0, 30]]]),
    controls: new Map<number, Vec3>([[1, [10, 2, 0]], [3, [0, 1, 15]]]),
  };
  const parents = new Map<number, number>([[0, -1], [1, 0], [2, 1], [3, 1]]);

  it('returns the inputs at t = 0 and t = 1', () => {
    const at0 = interpolateLayouts(a, b, 0, parents);
    expect([...at0.positions]).toEqual([...a.positions]);
    expect([...at0.controls]).toEqual([...a.controls]);
    const at1 = interpolateLayouts(a, b, 1, parents);
    expect([...at1.positions]).toEqual([...b.positions]);
    expect([...at1.controls]).toEqual([...b.controls]);
    expect(at0.opacity.get(2)).toBe(1);
    expect(at1.opacity.get(3)).toBe(1);
  });

  it('clamps t outside [0, 1]', () => {
    expect([...interpolateLayouts(a, b, -2, parents).positions]).toEqual([...a.positions]);
    expect([...interpolateLayouts(a, b, 7, parents).positions]).toEqual([...b.positions]);
  });

  it('eases shared nodes and fades one-sided nodes from the parent position', () => {
    const mid = interpolateLayouts(a, b, 0.5, parents);
    expect(mid.positions.get(1)).toEqual([15, 0, 0]);
    // node 3 enters from its parent's position in a (node 1 at x = 10)
    const p3 = mid.positions.get(3)!;
    expect(p3[0]).toBeCloseTo(5, 9);
    expect(p3[2]).toBeCloseTo(15, 9);
    expect(mid.opacity.get(3)).toBeCloseTo(0.5, 9);
    // node 2 leaves toward its parent's position in b (node 1 at x = 20)
    const p2 = mid.positions.get(2)!;
    expect(p2[0]).toBeCloseTo(10, 9);
    expect(p2[1]).toBeCloseTo(5, 9);
    expect(mid.opacity.get(2)).toBeCloseTo(0.5, 9);
    const early = interpolateLayouts(a, b, 0.25, parents);
    expect(early.positions.get(1)![0]).toBeLessThan(12.5);
  });

  it('holds a one-sided node in place when no parent is known', () => {
    const mid = interpolateLayouts(a, b, 0.5);
    expect(mid.positions.get(3)).toEqual([0, 0, 30]);
    expect(mid.opacity.get(3)).toBeCloseTo(0.5, 9);
  });

  it('accepts a SearchTree-shaped parent source and interpolates geodesics of equal length', () => {
    const ga = { ...a, geodesics: new Map<number, Vec3[]>([[1, [[0, 0, 0], [10, 0, 0]]]]) };
    const gb = { ...b, geodesics: new Map<number, Vec3[]>([[1, [[0, 0, 0], [20, 0, 0]]]]) };
    const nodes = new Map([...parents].map(([id, parent]) => [id, { parent }]));
    const mid = interpolateLayouts(ga, gb, 0.5, { nodes });
    expect(mid.geodesics!.get(1)).toEqual([[0, 0, 0], [15, 0, 0]]);
    expect(mid.positions.get(3)![0]).toBeCloseTo(5, 9);
  });
});
