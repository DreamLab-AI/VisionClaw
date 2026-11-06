/**
 * Label budget (ADR-2186 D1–D3, D5).
 *
 * Labels are chosen by importance, not proximity, in three tiers:
 *   1. selection tier (`selectedBudget`): SELECTED and HOVERED nodes always,
 *      then SEARCH_HIT, then FOCUS_NEIGHBOUR nodes by priority;
 *   1b. cluster labels (ADR-2186 D2, clusterLabels.ts): the largest server
 *      clusters named at their centroids at overview zoom, outranking node
 *      labels; at most CLUSTER_LABEL_MAX, taken out of `budget`;
 *   2. overview tier (`budget`): the top-`budget` nodes of the global priority
 *      ranking (less the cluster labels), wherever they are, drawn when on screen;
 *   3. dynamic tier (`dynamicBudget`): the most important on-screen nodes not
 *      already labelled (the viewport's own top nodes once zoomed in).
 * Greyed-out nodes (ADR-2183) never get a label. Except for selected and
 * hovered nodes, a label whose screen cell is taken by a higher-priority label
 * is skipped (a coarse grid over the projected positions) and the next one in
 * the ranking is tried.
 *
 * Priority is the wire centrality (analytics buffer) when any node has one,
 * else degree. `rankByPriority` sorts once per priority change; `selectLabels`
 * then runs in O(n) at the label cadence (LABEL_REFRESH_MS, ≈4 Hz, plus on
 * camera stop), never per frame.
 */
import {
  HIGHLIGHT_SELECTED,
  HIGHLIGHT_HOVERED,
  HIGHLIGHT_SEARCH_HIT,
  HIGHLIGHT_FOCUS_NEIGHBOUR,
  HIGHLIGHT_LEVEL_GREYED,
  highlightLevel,
} from '../state/highlightMask';
import { ANALYTICS_STRIDE, ANALYTICS_CENTRALITY_OFFSET } from '../../analytics/store/nodeAnalyticsStore';
import { CLUSTER_LABEL_MAX } from './clusterLabels';

/** Label selection cadence (ADR-2186 D5). */
export const LABEL_REFRESH_MS = 250;

export const TIER_SELECTION = 0;
export const TIER_OVERVIEW = 1;
export const TIER_DYNAMIC = 2;
export type LabelTier = 0 | 1 | 2;

export interface LabelBudgets {
  budget: number;
  dynamicBudget: number;
  selectedBudget: number;
}

/** Glyph capacity for a budget: every label at its longest (title + metadata lines). */
export const GLYPHS_PER_LABEL = 160;
export function glyphCapacityFor(b: LabelBudgets): number {
  const labels = Math.max(0, b.budget) + Math.max(0, b.dynamicBudget) + Math.max(0, b.selectedBudget) + 2;
  return labels * GLYPHS_PER_LABEL;
}

/**
 * Priority per local node (higher = more important): centrality from the
 * analytics buffer (indexed by render index) when any node has a positive one,
 * else degree. Writes `out` and returns which source was used.
 */
export function labelPriorities(
  renderIndex: ArrayLike<number>,
  count: number,
  degreeOf: (i: number) => number,
  analytics: Float32Array | null | undefined,
  out: Float32Array,
): 'centrality' | 'degree' {
  let any = false;
  if (analytics) {
    for (let i = 0; i < count; i++) {
      const o = renderIndex[i] * ANALYTICS_STRIDE + ANALYTICS_CENTRALITY_OFFSET;
      const c = o < analytics.length ? analytics[o] : 0;
      out[i] = c > 0 ? c : 0;
      if (c > 0) any = true;
    }
  }
  if (any) return 'centrality';
  for (let i = 0; i < count; i++) out[i] = degreeOf(i);
  return 'degree';
}

/** Local indices sorted by priority, highest first; ties keep input order. */
export function rankByPriority(priority: ArrayLike<number>, count: number): Uint32Array {
  const order = new Uint32Array(count);
  for (let i = 0; i < count; i++) order[i] = i;
  order.sort((a, b) => (priority[b] - priority[a]) || (a - b));
  return order;
}

export interface LabelFrame {
  count: number;
  /** Priority order from rankByPriority. */
  order: ArrayLike<number>;
  priority: ArrayLike<number>;
  /** Highlight bits per local node (highlightMask byte at its render index). */
  bits: ArrayLike<number>;
  /** Screen position (CSS px) per local node; ignored where onScreen[i] is 0. */
  sx: ArrayLike<number>;
  sy: ArrayLike<number>;
  onScreen: ArrayLike<number>;
  viewWidth: number;
  viewHeight: number;
  /** Collision cell size, CSS px. */
  cellW: number;
  cellH: number;
  /**
   * ADR-2186 D2: ranked cluster label candidates (clusterLabels.ts), screen
   * px. Placed after the selection tier and before the node tiers, out of
   * `budget`, at most CLUSTER_LABEL_MAX.
   */
  clusters?: { count: number; sx: ArrayLike<number>; sy: ArrayLike<number> } | null;
}

export interface LabelSelection {
  /** Local indices to label, in draw-priority order. */
  nodes: Uint32Array;
  tiers: Uint8Array;
  count: number;
  /** Indices into LabelFrame.clusters of the cluster labels to draw. */
  clusters: Uint32Array;
  clusterCount: number;
}

export function createLabelSelection(capacity: number): LabelSelection {
  return {
    nodes: new Uint32Array(capacity), tiers: new Uint8Array(capacity), count: 0,
    clusters: new Uint32Array(CLUSTER_LABEL_MAX), clusterCount: 0,
  };
}

/** Grow-only collision grid. */
export class CollisionGrid {
  private cells = new Uint8Array(0);
  private cols = 0;
  private rows = 0;
  reset(viewW: number, viewH: number, cellW: number, cellH: number): void {
    this.cols = Math.max(1, Math.ceil(viewW / Math.max(1, cellW)));
    this.rows = Math.max(1, Math.ceil(viewH / Math.max(1, cellH)));
    const n = this.cols * this.rows;
    if (this.cells.length < n) this.cells = new Uint8Array(n);
    else this.cells.fill(0, 0, n);
    this.cw = Math.max(1, cellW);
    this.ch = Math.max(1, cellH);
  }
  private cw = 1;
  private ch = 1;
  private key(x: number, y: number): number {
    const gx = Math.min(this.cols - 1, Math.max(0, Math.floor(x / this.cw)));
    const gy = Math.min(this.rows - 1, Math.max(0, Math.floor(y / this.ch)));
    return gy * this.cols + gx;
  }
  taken(x: number, y: number): boolean { return this.cells[this.key(x, y)] === 1; }
  take(x: number, y: number): void { this.cells[this.key(x, y)] = 1; }
}

/**
 * Choose the labels for this refresh. `out` must hold at least
 * budget + dynamicBudget + selectedBudget + 2 entries. No allocation.
 */
export function selectLabels(f: LabelFrame, b: LabelBudgets, grid: CollisionGrid, mark: Uint8Array, out: LabelSelection): LabelSelection {
  const n = Math.min(f.count, mark.length);
  mark.fill(0, 0, n);
  grid.reset(f.viewWidth, f.viewHeight, f.cellW, f.cellH);
  let k = 0;
  const cap = out.nodes.length;
  const push = (i: number, tier: LabelTier) => {
    if (k >= cap) return;
    out.nodes[k] = i;
    out.tiers[k] = tier;
    k++;
    mark[i] = 1;
    grid.take(f.sx[i], f.sy[i]);
  };
  const eligible = (i: number) => f.onScreen[i] !== 0 && highlightLevel(f.bits[i]) !== HIGHLIGHT_LEVEL_GREYED;

  // 1. selection tier
  let sel = 0;
  const selCap = Math.max(0, b.selectedBudget);
  for (let i = 0; i < n; i++) {
    // selected / hovered: always (highlightLevel never greys them)
    if ((f.bits[i] & (HIGHLIGHT_SELECTED | HIGHLIGHT_HOVERED)) && f.onScreen[i] !== 0) { push(i, TIER_SELECTION); sel++; }
  }
  for (let r = 0; r < n && sel < selCap; r++) {
    const i = f.order[r];
    if (mark[i] || !eligible(i) || !(f.bits[i] & HIGHLIGHT_SEARCH_HIT)) continue;
    push(i, TIER_SELECTION); sel++;
  }
  for (let r = 0; r < n && sel < selCap; r++) {
    const i = f.order[r];
    if (mark[i] || !eligible(i) || !(f.bits[i] & HIGHLIGHT_FOCUS_NEIGHBOUR)) continue;
    if (grid.taken(f.sx[i], f.sy[i])) continue;
    push(i, TIER_SELECTION); sel++;
  }

  // 1b. cluster labels (ADR-2186 D2): outrank node labels, out of `budget`
  let nc = 0;
  const c = f.clusters;
  if (c) {
    const cCap = Math.min(CLUSTER_LABEL_MAX, Math.max(0, b.budget), out.clusters.length);
    for (let j = 0; j < c.count && nc < cCap; j++) {
      if (grid.taken(c.sx[j], c.sy[j])) continue;
      grid.take(c.sx[j], c.sy[j]);
      out.clusters[nc++] = j;
    }
  }
  out.clusterCount = nc;

  // 2. overview tier: the global top-`budget` (less the cluster labels), drawn where on screen
  const overviewSlots = Math.max(0, b.budget) - nc;
  let ov = 0;
  const top = Math.min(n, Math.max(0, b.budget));
  for (let r = 0; r < top && ov < overviewSlots; r++) {
    const i = f.order[r];
    if (mark[i] || !eligible(i) || grid.taken(f.sx[i], f.sy[i])) continue;
    push(i, TIER_OVERVIEW); ov++;
  }

  // 3. dynamic tier: the viewport's most important remaining nodes
  let dyn = 0;
  const dynCap = Math.max(0, b.dynamicBudget);
  for (let r = 0; r < n && dyn < dynCap; r++) {
    const i = f.order[r];
    if (mark[i] || !eligible(i) || grid.taken(f.sx[i], f.sy[i])) continue;
    push(i, TIER_DYNAMIC); dyn++;
  }
  out.count = k;
  return out;
}
