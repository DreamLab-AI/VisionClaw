/**
 * Seeded synthetic vector sets for the memory trajectory tests.
 *
 * Clustered, L2-normalised rows give HNSW a realistic neighbourhood
 * structure; every generator is deterministic for a given seed.
 */
import type { VectorSet } from '../types';

export function seededRandom(seed: number): () => number {
  let a = seed | 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function gaussian(rand: () => number): number {
  const u = Math.max(rand(), 1e-12);
  const v = rand();
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
}

export function normaliseInPlace(v: Float32Array, offset = 0, dim = v.length): void {
  let s = 0;
  for (let k = 0; k < dim; k++) s += v[offset + k] * v[offset + k];
  const inv = s > 0 ? 1 / Math.sqrt(s) : 0;
  for (let k = 0; k < dim; k++) v[offset + k] *= inv;
}

/** `count` rows of `dim` components around `clusters` random centres. */
export function clusteredVectors(
  count: number,
  dim: number,
  clusters: number,
  seed: number,
  spread = 0.35,
): VectorSet {
  const rand = seededRandom(seed);
  const centres: Float32Array[] = [];
  for (let c = 0; c < clusters; c++) {
    const v = new Float32Array(dim);
    for (let k = 0; k < dim; k++) v[k] = gaussian(rand);
    normaliseInPlace(v);
    centres.push(v);
  }
  const data = new Float32Array(count * dim);
  for (let i = 0; i < count; i++) {
    const c = centres[Math.floor(rand() * clusters)];
    for (let k = 0; k < dim; k++) data[i * dim + k] = c[k] + (spread * gaussian(rand)) / Math.sqrt(dim) * 3;
    normaliseInPlace(data, i * dim, dim);
  }
  return { count, dim, data };
}

/** Queries made by perturbing random rows of `vs`. */
export function perturbedQueries(vs: VectorSet, n: number, seed: number, noise = 0.15): Float32Array[] {
  const rand = seededRandom(seed);
  const out: Float32Array[] = [];
  for (let j = 0; j < n; j++) {
    const i = Math.floor(rand() * vs.count);
    const q = new Float32Array(vs.dim);
    for (let k = 0; k < vs.dim; k++) q[k] = vs.data[i * vs.dim + k] + (noise * gaussian(rand)) / Math.sqrt(vs.dim);
    normaliseInPlace(q);
    out.push(q);
  }
  return out;
}

/** Plain-language reference: sort every row by `1 - dot`, ties by id. */
export function naiveTopK(vs: VectorSet, q: Float32Array, k: number): number[] {
  const rows: [number, number][] = [];
  for (let i = 0; i < vs.count; i++) {
    let s = 0;
    for (let d = 0; d < vs.dim; d++) s += q[d] * vs.data[i * vs.dim + d];
    rows.push([1 - s, i]);
  }
  rows.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return rows.slice(0, k).map((r) => r[1]);
}
