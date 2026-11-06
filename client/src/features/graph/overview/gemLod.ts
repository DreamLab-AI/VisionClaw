/**
 * Gem level of detail (ADR-2182 D2).
 *
 * At overview zoom every knowledge node is a sprite; a gem is drawn only for
 * a node within `gemLodDistance` of the camera, nearest first, up to a fixed
 * budget, plus every forced node (selected, hovered) regardless of distance.
 * The result is a per-instance flag array GemNodes reads in its transform
 * write. Run on camera or position change at a bounded cadence, not per frame.
 */

/** Gems drawn at most, besides forced ones. */
export const GEM_BUDGET = 4096;

/** Reusable scratch; grow-only. */
export interface GemLodScratch {
  idx: Uint32Array;
  dist: Float32Array;
}

export function createGemLodScratch(): GemLodScratch {
  return { idx: new Uint32Array(0), dist: new Float32Array(0) };
}

export interface GemLodInput {
  /** Drawn positions, xyz per render index. */
  positions: ArrayLike<number>;
  /** Local instance i → render index (GemNodes' posIdxCache). */
  renderIndex: ArrayLike<number>;
  /** Instances considered (GemNodes' visible count). */
  count: number;
  camX: number;
  camY: number;
  camZ: number;
  lodDistance: number;
  budget: number;
  /** Optional per-local-instance flag: non-zero = always a gem (selected, hovered). */
  forced?: ArrayLike<number> | null;
}

/**
 * Fill `out[i]` (local instance) with 1 = draw as a gem, 0 = sprite only.
 * Returns the number of gems. Allocates only when the scratch must grow.
 */
export function selectGemLod(input: GemLodInput, out: Uint8Array, scratch: GemLodScratch): number {
  const { positions, renderIndex, lodDistance, forced } = input;
  const n = Math.max(0, Math.min(input.count, out.length, renderIndex.length));
  const budget = Math.max(0, Math.floor(input.budget));
  const lim2 = lodDistance > 0 ? lodDistance * lodDistance : 0;
  if (scratch.idx.length < n) {
    scratch.idx = new Uint32Array(n);
    scratch.dist = new Float32Array(n);
  }
  const idx = scratch.idx;
  const dist = scratch.dist;
  let forcedCount = 0;
  let cand = 0;
  for (let i = 0; i < n; i++) {
    out[i] = 0;
    if (forced && forced[i]) { out[i] = 1; forcedCount++; continue; }
    if (lim2 === 0) continue;
    const r3 = renderIndex[i] * 3;
    if (r3 + 2 >= positions.length) continue;
    const dx = positions[r3] - input.camX;
    const dy = positions[r3 + 1] - input.camY;
    const dz = positions[r3 + 2] - input.camZ;
    const d2 = dx * dx + dy * dy + dz * dz;
    if (!(d2 <= lim2)) continue;
    idx[cand] = i;
    dist[cand] = d2;
    cand++;
  }
  if (cand <= budget) {
    for (let k = 0; k < cand; k++) out[idx[k]] = 1;
    return forcedCount + cand;
  }
  const order = idx.subarray(0, cand);
  // Sort candidate slots by distance: sort positions 0..cand-1 via their distances.
  const slot = new Uint32Array(cand);
  for (let k = 0; k < cand; k++) slot[k] = k;
  slot.sort((a, b) => dist[a] - dist[b]);
  for (let k = 0; k < budget; k++) out[order[slot[k]]] = 1;
  return forcedCount + budget;
}

/** True when the camera moved enough since `prev` to redo the LOD (and copies it). */
export function cameraMoved(
  prev: { x: number; y: number; z: number; init: boolean },
  x: number, y: number, z: number,
  epsilon: number,
): boolean {
  const dx = x - prev.x, dy = y - prev.y, dz = z - prev.z;
  if (prev.init && dx * dx + dy * dy + dz * dz <= epsilon * epsilon) return false;
  prev.x = x; prev.y = y; prev.z = z; prev.init = true;
  return true;
}
