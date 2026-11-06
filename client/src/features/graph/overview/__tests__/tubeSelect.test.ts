import { describe, it, expect } from 'vitest';
import {
  buildEdgeAdjacency, createTubeSelectScratch, emphasisNodes, selectTubeEdges, type GlassTubeLod,
} from '../tubeSelect';
import { fillEdgeFrame, createEdgeFrameBuffers, type EdgePlan } from '../../hooks/edgeBufferPlan';
import {
  HIGHLIGHT_FOCUS_NEIGHBOUR, HIGHLIGHT_HOVERED, HIGHLIGHT_SELECTED,
} from '../../state/highlightMask';

/** Nodes 0..N-1 along x at 10-unit spacing; edges given as [s, t] pairs. */
function fixture(nodeCount: number, pairs: Array<[number, number]>) {
  const positions = new Float32Array(nodeCount * 3);
  for (let i = 0; i < nodeCount; i++) positions[i * 3] = i * 10;
  const n = pairs.length;
  const plan: EdgePlan = {
    count: n,
    srcIdx: Int32Array.from(pairs.map((p) => p[0])),
    tgtIdx: Int32Array.from(pairs.map((p) => p[1])),
    srcR: new Float64Array(n), tgtR: new Float64Array(n),
    colors: new Float32Array(n * 3), weights: new Float32Array(n).fill(1),
  };
  return { positions, plan, adjacency: buildEdgeAdjacency(plan, nodeCount) };
}

function run(f: ReturnType<typeof fixture>, mask: Uint8Array, lod: GlassTubeLod, cam = [0, 0, 0]) {
  const emphasis = new Int32Array(mask.length);
  const emphasisCount = emphasisNodes(mask, emphasis);
  const out = { idx: new Int32Array(0), count: 0 };
  selectTubeEdges({
    plan: f.plan, adjacency: f.adjacency, positions: f.positions, mask, emphasis, emphasisCount,
    camX: cam[0], camY: cam[1], camZ: cam[2], lod,
  }, out, createTubeSelectScratch());
  return Array.from(out.idx.subarray(0, out.count));
}

describe('tube subset (ADR-2181 D3)', () => {
  // chain 0-1-2-3-4-5, plus 2-5
  const pairs: Array<[number, number]> = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [2, 5]];

  it('builds a CSR adjacency covering both endpoints', () => {
    const f = fixture(6, pairs);
    const of = (n: number) => Array.from(f.adjacency.edges.subarray(f.adjacency.offsets[n], f.adjacency.offsets[n + 1])).sort();
    expect(of(2)).toEqual([1, 2, 5]);
    expect(of(0)).toEqual([0]);
    expect(f.adjacency.offsets[6]).toBe(pairs.length * 2);
  });

  it('close zoom: edges around the nearest nodes only, ascending', () => {
    const f = fixture(6, pairs);
    // camera at node 0; distance 12 reaches nodes 0 and 1
    expect(run(f, new Uint8Array(6), { maxDistance: 12, budget: 10 })).toEqual([0, 1]);
  });

  it('distance 0 = emphasis tubes only', () => {
    const f = fixture(6, pairs);
    expect(run(f, new Uint8Array(6), { maxDistance: 0, budget: 10 })).toEqual([]);
  });

  it('hovered node: every incident edge, even with zoom off', () => {
    const f = fixture(6, pairs);
    const mask = new Uint8Array(6); mask[2] = HIGHLIGHT_HOVERED;
    expect(run(f, mask, { maxDistance: 0, budget: 10 })).toEqual([1, 2, 5]);
  });

  it('focus neighbours: only edges between emphasised nodes; selected-node edges left to the highlight tubes', () => {
    const f = fixture(6, pairs);
    const mask = new Uint8Array(6);
    mask[2] = HIGHLIGHT_SELECTED;
    mask[1] = mask[3] = mask[5] = HIGHLIGHT_FOCUS_NEIGHBOUR;
    mask[4] = HIGHLIGHT_FOCUS_NEIGHBOUR;
    // 1-2, 2-3, 2-5 touch the selected node → excluded; 3-4, 4-5 join neighbours; 0-1 does not
    expect(run(f, mask, { maxDistance: 0, budget: 10 })).toEqual([3, 4]);
  });

  it('emphasis edges take priority over close-zoom edges under a tight budget', () => {
    const f = fixture(6, pairs);
    const mask = new Uint8Array(6); mask[5] = HIGHLIGHT_HOVERED; // far from the camera
    expect(run(f, mask, { maxDistance: 100, budget: 2 })).toEqual([4, 5]);
  });

  it('never exceeds the budget and is reusable across calls', () => {
    const f = fixture(6, pairs);
    const scratch = createTubeSelectScratch();
    const out = { idx: new Int32Array(0), count: 0 };
    const mask = new Uint8Array(6);
    const base = { plan: f.plan, adjacency: f.adjacency, positions: f.positions, mask, emphasis: new Int32Array(6), emphasisCount: 0, camX: 0, camY: 0, camZ: 0 };
    selectTubeEdges({ ...base, lod: { maxDistance: 1000, budget: 3 } }, out, scratch);
    expect(out.count).toBe(3);
    selectTubeEdges({ ...base, lod: { maxDistance: 1000, budget: 6 } }, out, scratch);
    expect(Array.from(out.idx.subarray(0, out.count))).toEqual([0, 1, 2, 3, 4, 5]);
    expect(Array.from(scratch.picked).every((v) => v === 0)).toBe(true);
  });

  it('fillEdgeFrame with a subset emits only those edges', () => {
    const f = fixture(6, pairs);
    const buf = createEdgeFrameBuffers();
    const all = fillEdgeFrame(f.plan, f.positions, buf);
    expect(all).toBe(6);
    const n = fillEdgeFrame(f.plan, f.positions, buf, Int32Array.from([2, 5]), 2);
    expect(n).toBe(2);
    expect(buf.points.slice(0, 6)).toEqual([20, 0, 0, 30, 0, 0]);
    expect(buf.points.slice(6, 12)).toEqual([20, 0, 0, 50, 0, 0]);
  });
});
