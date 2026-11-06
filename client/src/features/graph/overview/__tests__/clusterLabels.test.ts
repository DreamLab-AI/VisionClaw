import { describe, it, expect } from 'vitest';
import {
  assignGroups, computeClusterCandidates, createClusterCandidates, createClusterGroups, createClusterScratch,
  clusterLabelText, MIN_CLUSTER_MEMBERS, CLUSTER_LABEL_MAX, type Projector,
} from '../clusterLabels';
import { selectLabels, createLabelSelection, CollisionGrid, TIER_OVERVIEW, TIER_SELECTION } from '../labelBudget';
import { ANALYTICS_STRIDE, ANALYTICS_CLUSTER_OFFSET } from '../../../analytics/store/nodeAnalyticsStore';
import { HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_SELECTED } from '../../state/highlightMask';

/** Local i = render index i; cluster ids per node; x = 10·i. */
function setup(clusterIds: number[]) {
  const n = clusterIds.length;
  const analytics = new Float32Array(n * ANALYTICS_STRIDE);
  clusterIds.forEach((c, i) => { analytics[i * ANALYTICS_STRIDE + ANALYTICS_CLUSTER_OFFSET] = c; });
  const renderIndex = Int32Array.from({ length: n }, (_, i) => i);
  const groups = assignGroups(renderIndex, n, analytics, createClusterGroups());
  const world = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) world[i * 3] = 10 * i;
  return { n, groups, world };
}
// orthographic stand-in: x → sx, everything on screen
const project: Projector = (x, _y, _z, out) => { out.sx = x; out.sy = 50; return true; };
const order = (n: number) => Uint32Array.from({ length: n }, (_, i) => i); // priority = index order

function candidates(clusterIds: number[], bits?: Uint8Array, minDistance = 0, camX = 0) {
  const { n, groups, world } = setup(clusterIds);
  return computeClusterCandidates({
    count: n, groups, order: order(n), bits: bits ?? new Uint8Array(n), world,
    camX, camY: 0, camZ: 0, minDistance, project,
  }, createClusterScratch(), createClusterCandidates());
}

describe('cluster labels (ADR-2186 D2)', () => {
  it('groups by wire cluster id; 0 = unassigned', () => {
    const { groups } = setup([0, 5, 5, 9]);
    expect(groups.slots).toBe(2);
    expect(Array.from(groups.slotOf.subarray(0, 4))).toEqual([-1, 0, 0, 1]);
    expect(Array.from(groups.idOf.subarray(0, 2))).toEqual([5, 9]);
  });

  it('ranks clusters by live members, centroid at the mean, named after the top member', () => {
    const c = candidates([7, 7, 7, 3, 3, 3, 3, 0]);
    expect(c.count).toBe(2);
    expect(c.id[0]).toBe(3); expect(c.members[0]).toBe(4); expect(c.top[0]).toBe(3);
    expect(c.x[0]).toBeCloseTo((30 + 40 + 50 + 60) / 4);
    expect(c.id[1]).toBe(7); expect(c.members[1]).toBe(3); expect(c.top[1]).toBe(0);
  });

  it(`clusters under ${MIN_CLUSTER_MEMBERS} live members, and greyed members, do not count`, () => {
    const bits = new Uint8Array(6);
    bits[0] = HIGHLIGHT_FILTERED_OUT; // greys one of cluster 4's three
    const c = candidates([4, 4, 4, 8, 8, 8], bits);
    expect(c.count).toBe(1);
    expect(c.id[0]).toBe(8);
    // top member skips the greyed node
    const c2 = candidates([4, 4, 4, 4], bits);
    expect(c2.top[0]).toBe(1);
  });

  it('close zoom (centroid within minDistance) gets no cluster label', () => {
    expect(candidates([1, 1, 1], undefined, 100, 10).count).toBe(0);
    expect(candidates([1, 1, 1], undefined, 5, 1000).count).toBe(1);
  });

  it('cluster labels outrank node labels and come out of the overview budget', () => {
    const c = candidates([2, 2, 2, 2, 6, 6, 6, 0, 0, 0]);
    const n = 10;
    const bits = new Uint8Array(n); bits[9] = HIGHLIGHT_SELECTED;
    const out = createLabelSelection(32);
    selectLabels({
      count: n, order: order(n), priority: new Float32Array(n), bits,
      sx: Float32Array.from({ length: n }, (_, i) => 100 * i), sy: new Float32Array(n).fill(200),
      onScreen: new Uint8Array(n).fill(1), viewWidth: 2000, viewHeight: 400, cellW: 40, cellH: 20,
      clusters: c,
    }, { budget: 4, dynamicBudget: 0, selectedBudget: 2 }, new CollisionGrid(), new Uint8Array(n), out);
    expect(out.clusterCount).toBe(2);
    const overview = Array.from(out.tiers.subarray(0, out.count)).filter((t) => t === TIER_OVERVIEW).length;
    expect(overview).toBe(2); // budget 4 − 2 cluster labels
    expect(out.tiers[0]).toBe(TIER_SELECTION); // the selection still comes first
  });

  it(`never more than CLUSTER_LABEL_MAX (${CLUSTER_LABEL_MAX}) or the budget`, () => {
    const ids: number[] = [];
    for (let k = 1; k <= 20; k++) ids.push(k, k, k);
    const c = candidates(ids);
    expect(c.count).toBe(20);
    const n = ids.length;
    const run = (budget: number) => {
      const out = createLabelSelection(64);
      selectLabels({
        count: n, order: order(n), priority: new Float32Array(n), bits: new Uint8Array(n),
        sx: new Float32Array(n), sy: new Float32Array(n), onScreen: new Uint8Array(n),
        viewWidth: 1e5, viewHeight: 100, cellW: 1, cellH: 1, clusters: { count: c.count, sx: Float32Array.from({ length: 20 }, (_, j) => 1000 * j), sy: new Float32Array(20) },
      }, { budget, dynamicBudget: 0, selectedBudget: 0 }, new CollisionGrid(), new Uint8Array(n), out);
      return out.clusterCount;
    };
    expect(run(100)).toBe(CLUSTER_LABEL_MAX);
    expect(run(3)).toBe(3);
  });

  it('label text: top member and the rest of the cluster', () => {
    expect(clusterLabelText('Ontology', 42)).toBe('Ontology +41');
    expect(clusterLabelText('x'.repeat(40), 2)).toBe(`${'x'.repeat(29)}... +1`);
  });
});
