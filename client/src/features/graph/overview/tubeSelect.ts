/**
 * Glass-tube subset (ADR-2181 D3): which plan edges are drawn as glass tubes
 * while the screen-space overview edges are enabled (pending or live; see
 * resolveTubeLod).
 *
 * Tubes are the close level of detail, so their CPU feed must scale with the
 * tube budget, never with E. The selection therefore walks a per-plan node →
 * edge adjacency (CSR) from a small set of seed nodes instead of scanning
 * every edge:
 *
 *   1. emphasis edges, always first: every edge touching a HOVERED node, and
 *      every edge whose two endpoints are both emphasised (SELECTED, HOVERED or
 *      FOCUS_NEIGHBOUR), i.e. the focus → neighbour edges. Edges touching a
 *      SELECTED node are left out: the highlight GlassEdges (on the bloom
 *      layer) already draws them, and drawing them twice z-fights;
 *   2. close-zoom edges: edges touching the nodes nearest the camera within
 *      `maxDistance` (nearest node first), while the budget lasts.
 *
 * The total never exceeds `budget`. The result is in plan edge order so the
 * fill is a forward walk. The node scan in step 2 is O(N) and runs only at the
 * LOD cadence (useEdgeBufferComputation), never per frame.
 */
import {
  HIGHLIGHT_FOCUS_NEIGHBOUR, HIGHLIGHT_HOVERED, HIGHLIGHT_SELECTED,
} from '../state/highlightMask';
import type { EdgePlan } from '../hooks/edgeBufferPlan';
import type { LiveState } from './overviewLive';

/** Close-zoom tube LOD. `maxDistance` 0 = emphasis tubes only. */
export interface GlassTubeLod {
  maxDistance: number;
  budget: number;
}

/** Tubes drawn at most while the overview edges are enabled and not faulted. */
export const TUBE_BUDGET = 4096;

/**
 * The tube LOD for the frame loop, or null to feed every edge as a tube.
 *
 * Null only when the overview edges are off or have faulted: then the tubes
 * are the whole edge layer (the safety valve). While the overview edges are
 * enabled and merely pending (positions not yet in the texture, material
 * still building), the tubes already take the subset: feeding every edge in
 * that window grew GlassEdges to its ceiling and warned on large graphs, only
 * to shrink a few frames later. The trade-off is that the far edges are
 * blank for those first frames instead of drawn as tubes.
 */
export function resolveTubeLod(s: {
  enabled: boolean;
  /** The overviewLive snapshot (useOverviewLive). Its `edges` flag is not read: pending counts. */
  live: Pick<LiveState, 'faults'>;
  glassAtCloseZoom: boolean;
  gemLodDistance: number;
}): GlassTubeLod | null {
  if (!s.enabled || s.live.faults.edges !== undefined) return null;
  return { maxDistance: s.glassAtCloseZoom ? s.gemLodDistance : 0, budget: TUBE_BUDGET };
}

const EMPHASIS = HIGHLIGHT_SELECTED | HIGHLIGHT_HOVERED | HIGHLIGHT_FOCUS_NEIGHBOUR;

/** Node → incident plan edges, compressed sparse rows. Built once per plan. */
export interface EdgeAdjacency {
  /** offsets[n]..offsets[n+1] index into `edges`; length nodeCount + 1. */
  offsets: Int32Array;
  edges: Int32Array;
  nodeCount: number;
}

export function buildEdgeAdjacency(plan: EdgePlan, nodeCount: number): EdgeAdjacency {
  const offsets = new Int32Array(nodeCount + 1);
  for (let i = 0; i < plan.count; i++) {
    const s = plan.srcIdx[i], t = plan.tgtIdx[i];
    if (s >= 0 && s < nodeCount) offsets[s + 1]++;
    if (t !== s && t >= 0 && t < nodeCount) offsets[t + 1]++;
  }
  for (let n = 0; n < nodeCount; n++) offsets[n + 1] += offsets[n];
  const edges = new Int32Array(offsets[nodeCount]);
  const fill = offsets.slice(0, nodeCount);
  for (let i = 0; i < plan.count; i++) {
    const s = plan.srcIdx[i], t = plan.tgtIdx[i];
    if (s >= 0 && s < nodeCount) edges[fill[s]++] = i;
    if (t !== s && t >= 0 && t < nodeCount) edges[fill[t]++] = i;
  }
  return { offsets, edges, nodeCount };
}

/**
 * Indices of the nodes carrying any emphasis bit, rebuilt only when the mask
 * version changes (O(N) then, nothing otherwise).
 */
export function emphasisNodes(mask: Uint8Array, out: Int32Array): number {
  let n = 0;
  for (let i = 0; i < mask.length && n < out.length; i++) if (mask[i] & EMPHASIS) out[n++] = i;
  return n;
}

/** Grow-only working memory for selectTubeEdges; one per caller. */
export interface TubeSelectScratch {
  /** Per plan edge: 1 while picked in the current call (reset before return). */
  picked: Uint8Array;
  near: Int32Array;
  nearD: Float32Array;
  order: Uint32Array;
  byDist: (a: number, b: number) => number;
}

export function createTubeSelectScratch(): TubeSelectScratch {
  const s: TubeSelectScratch = {
    picked: new Uint8Array(0), near: new Int32Array(0), nearD: new Float32Array(0), order: new Uint32Array(0),
    byDist: (a, b) => s.nearD[a] - s.nearD[b],
  };
  return s;
}

export interface TubeSelectInput {
  plan: EdgePlan;
  adjacency: EdgeAdjacency;
  /** Drawn positions, 3 floats per render index. */
  positions: Float32Array;
  /** Highlight bits per render index (highlightMask.getMask()); may be shorter. */
  mask: Uint8Array;
  /** Emphasised node indices (emphasisNodes) and their count. */
  emphasis: Int32Array;
  emphasisCount: number;
  camX: number; camY: number; camZ: number;
  lod: GlassTubeLod;
}

/**
 * Write the chosen plan edge indices, ascending, to `out` (grown if needed;
 * the grown array is returned with the count). Never more than `lod.budget`.
 */
export function selectTubeEdges(
  input: TubeSelectInput, out: { idx: Int32Array; count: number }, scratch: TubeSelectScratch,
): void {
  const { plan, adjacency, positions, mask, emphasis, emphasisCount, lod } = input;
  const budget = Math.max(0, Math.floor(lod.budget));
  if (out.idx.length < budget) out.idx = new Int32Array(budget);
  if (scratch.picked.length < plan.count) scratch.picked = new Uint8Array(plan.count);
  const picked = scratch.picked;
  const idx = out.idx;
  const { offsets, edges } = adjacency;
  const bitsOf = (n: number): number => (n < mask.length ? mask[n] : 0);
  let count = 0;

  const take = (e: number): boolean => {
    if (picked[e]) return true;
    const s = plan.srcIdx[e], t = plan.tgtIdx[e];
    // Selected-node edges belong to the highlight tubes.
    if ((bitsOf(s) | bitsOf(t)) & HIGHLIGHT_SELECTED) return true;
    if (count >= budget) return false;
    picked[e] = 1;
    idx[count++] = e;
    return true;
  };

  // 1. emphasis edges
  for (let k = 0; k < emphasisCount && count < budget; k++) {
    const n = emphasis[k];
    if (n >= adjacency.nodeCount) continue;
    const hovered = (bitsOf(n) & HIGHLIGHT_HOVERED) !== 0;
    for (let j = offsets[n]; j < offsets[n + 1]; j++) {
      const e = edges[j];
      const other = plan.srcIdx[e] === n ? plan.tgtIdx[e] : plan.srcIdx[e];
      if (!hovered && !(bitsOf(other) & EMPHASIS)) continue;
      if (!take(e)) break;
    }
  }

  // 2. close-zoom edges around the nearest nodes
  const lim2 = lod.maxDistance > 0 ? lod.maxDistance * lod.maxDistance : 0;
  if (lim2 > 0 && count < budget) {
    const nodeCount = Math.min(adjacency.nodeCount, Math.floor(positions.length / 3));
    let m = 0;
    for (let n = 0; n < nodeCount; n++) {
      if (offsets[n + 1] === offsets[n]) continue; // no edges
      const dx = positions[n * 3] - input.camX, dy = positions[n * 3 + 1] - input.camY, dz = positions[n * 3 + 2] - input.camZ;
      const d2 = dx * dx + dy * dy + dz * dz;
      if (!(d2 <= lim2)) continue;
      if (scratch.near.length <= m) {
        const grow = Math.max(256, m * 2);
        const near = new Int32Array(grow); near.set(scratch.near); scratch.near = near;
        const nd = new Float32Array(grow); nd.set(scratch.nearD); scratch.nearD = nd;
      }
      scratch.near[m] = n; scratch.nearD[m] = d2; m++;
    }
    if (scratch.order.length < m) scratch.order = new Uint32Array(Math.max(m, scratch.near.length));
    const order = scratch.order.subarray(0, m);
    for (let k = 0; k < m; k++) order[k] = k;
    order.sort(scratch.byDist);
    outer: for (let k = 0; k < m; k++) {
      const n = scratch.near[order[k]];
      for (let j = offsets[n]; j < offsets[n + 1]; j++) if (!take(edges[j])) break outer;
    }
  }

  for (let k = 0; k < count; k++) picked[idx[k]] = 0;
  idx.subarray(0, count).sort();
  out.count = count;
}
