/**
 * Cluster labels (ADR-2186 D2): at overview zoom the server's clusters are
 * named at their centroids, and those labels outrank node labels.
 *
 * Membership comes from the wire analytics (cluster id per render index,
 * 0 = unassigned). A cluster is a candidate when it has at least
 * MIN_CLUSTER_MEMBERS non-greyed members, its centroid is on screen, and the
 * centroid is at least `minDistance` from the camera (beyond that, close
 * zoom, the node labels say more). Candidates are ranked by live member
 * count; each is named after its highest-priority non-greyed member.
 * selectLabels then places them, after the always-on selection tier and
 * before the node tiers, out of the overview budget.
 *
 * Grouping is rebuilt only when the ranking inputs change (assignGroups); the
 * per-refresh pass is O(N) over the label population, like the rest of the
 * label runner, and allocation-free once warmed.
 */
import { ANALYTICS_STRIDE, ANALYTICS_CLUSTER_OFFSET } from '../../analytics/store/nodeAnalyticsStore';
import { HIGHLIGHT_LEVEL_GREYED, highlightLevel } from '../state/highlightMask';

/** Fewer live members than this: not worth a cluster label. */
export const MIN_CLUSTER_MEMBERS = 3;
/** Cluster labels drawn at most per refresh (they also come out of `budget`). */
export const CLUSTER_LABEL_MAX = 12;

/** Group slot per local node (-1 = none), and the cluster id per slot. */
export interface ClusterGroups {
  slotOf: Int32Array;
  idOf: Int32Array;
  slots: number;
}

/** Assign each local node a dense group slot from its wire cluster id. */
export function assignGroups(
  renderIndex: ArrayLike<number>, count: number,
  analytics: Float32Array | null | undefined, out: ClusterGroups,
): ClusterGroups {
  if (out.slotOf.length < count) out.slotOf = new Int32Array(count);
  out.slots = 0;
  out.slotOf.fill(-1, 0, count);
  if (!analytics) return out;
  const slotById = new Map<number, number>(); // rebuilt with the ranking only
  for (let i = 0; i < count; i++) {
    const ri = renderIndex[i];
    if (ri < 0) continue;
    const o = ri * ANALYTICS_STRIDE + ANALYTICS_CLUSTER_OFFSET;
    const id = o < analytics.length ? Math.round(analytics[o]) : 0;
    if (!(id > 0)) continue;
    let s = slotById.get(id);
    if (s === undefined) {
      s = out.slots++;
      slotById.set(id, s);
      if (out.idOf.length <= s) { const g = new Int32Array(Math.max(16, s * 2)); g.set(out.idOf); out.idOf = g; }
      out.idOf[s] = id;
    }
    out.slotOf[i] = s;
  }
  return out;
}

export function createClusterGroups(): ClusterGroups {
  return { slotOf: new Int32Array(0), idOf: new Int32Array(0), slots: 0 };
}

/** Ranked cluster label candidates for one refresh. */
export interface ClusterCandidates {
  count: number;
  /** Cluster id, live member count, top member (local index). */
  id: Int32Array;
  members: Int32Array;
  top: Int32Array;
  /** World centroid and its screen position (CSS px). */
  x: Float32Array; y: Float32Array; z: Float32Array;
  sx: Float32Array; sy: Float32Array;
}

export function createClusterCandidates(): ClusterCandidates {
  const f = () => new Float32Array(0), i = () => new Int32Array(0);
  return { count: 0, id: i(), members: i(), top: i(), x: f(), y: f(), z: f(), sx: f(), sy: f() };
}

/** Projects a world point; returns false when it is off screen. Writes CSS px to `out`. */
export type Projector = (x: number, y: number, z: number, out: { sx: number; sy: number }) => boolean;

export interface ClusterFrame {
  count: number;
  groups: ClusterGroups;
  /** Priority order (rankByPriority) over the local nodes. */
  order: ArrayLike<number>;
  bits: ArrayLike<number>;
  /** World position per local node, stride 3. */
  world: ArrayLike<number>;
  camX: number; camY: number; camZ: number;
  minDistance: number;
  project: Projector;
}

/** Scratch sized per slot; grow-only. */
export interface ClusterScratch {
  sum: Float64Array;
  n: Int32Array;
  top: Int32Array;
  rank: Uint32Array;
  byMembers: (a: number, b: number) => number;
  p: { sx: number; sy: number };
}

export function createClusterScratch(): ClusterScratch {
  const s: ClusterScratch = {
    sum: new Float64Array(0), n: new Int32Array(0), top: new Int32Array(0), rank: new Uint32Array(0),
    byMembers: (a, b) => (s.n[b] - s.n[a]) || (a - b),
    p: { sx: 0, sy: 0 },
  };
  return s;
}

export function computeClusterCandidates(f: ClusterFrame, scratch: ClusterScratch, out: ClusterCandidates): ClusterCandidates {
  const S = f.groups.slots;
  out.count = 0;
  if (S === 0) return out;
  if (scratch.n.length < S) {
    scratch.sum = new Float64Array(S * 3); scratch.n = new Int32Array(S); scratch.top = new Int32Array(S); scratch.rank = new Uint32Array(S);
  }
  const { sum, n, top } = scratch;
  sum.fill(0, 0, S * 3); n.fill(0, 0, S); top.fill(-1, 0, S);
  // walk in priority order, so the first live member seen is the top one
  for (let r = 0; r < f.count; r++) {
    const i = f.order[r];
    const s = f.groups.slotOf[i];
    if (s < 0 || highlightLevel(f.bits[i]) === HIGHLIGHT_LEVEL_GREYED) continue;
    if (top[s] < 0) top[s] = i;
    n[s]++;
    sum[s * 3] += f.world[i * 3]; sum[s * 3 + 1] += f.world[i * 3 + 1]; sum[s * 3 + 2] += f.world[i * 3 + 2];
  }
  const rank = scratch.rank.subarray(0, S);
  for (let s = 0; s < S; s++) rank[s] = s;
  rank.sort(scratch.byMembers);
  if (out.id.length < S) {
    out.id = new Int32Array(S); out.members = new Int32Array(S); out.top = new Int32Array(S);
    out.x = new Float32Array(S); out.y = new Float32Array(S); out.z = new Float32Array(S);
    out.sx = new Float32Array(S); out.sy = new Float32Array(S);
  }
  const min2 = Math.max(0, f.minDistance) ** 2;
  let k = 0;
  for (let r = 0; r < S; r++) {
    const s = rank[r];
    if (n[s] < MIN_CLUSTER_MEMBERS) break; // ranked by members: the rest are smaller
    const cx = sum[s * 3] / n[s], cy = sum[s * 3 + 1] / n[s], cz = sum[s * 3 + 2] / n[s];
    const dx = cx - f.camX, dy = cy - f.camY, dz = cz - f.camZ;
    if (dx * dx + dy * dy + dz * dz < min2) continue;
    if (!f.project(cx, cy, cz, scratch.p)) continue;
    out.id[k] = f.groups.idOf[s]; out.members[k] = n[s]; out.top[k] = top[s];
    out.x[k] = cx; out.y[k] = cy; out.z[k] = cz;
    out.sx[k] = scratch.p.sx; out.sy[k] = scratch.p.sy;
    k++;
  }
  out.count = k;
  return out;
}

/** The text of a cluster label: its top member's name and the live size. */
export function clusterLabelText(topName: string, members: number): string {
  const name = topName.length > 32 ? `${topName.slice(0, 29)}...` : topName;
  return `${name} +${(members - 1).toLocaleString('en-GB')}`;
}
