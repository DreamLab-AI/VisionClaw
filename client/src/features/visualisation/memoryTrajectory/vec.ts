/**
 * Vector and hashing primitives for the memory trajectory engine.
 *
 * The seeded generator and the integer hash follow the RuVector Explorer
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence).
 */
import type { VectorSet } from './types';

/**
 * Dot product of `q` with row `row` of `data`, unrolled by four. Rows are
 * L2-normalised, so `1 - dot` is the cosine distance used everywhere.
 */
export function dotRow(q: Float32Array, data: Float32Array, row: number, dim: number): number {
  const o = row * dim;
  let s0 = 0;
  let s1 = 0;
  let s2 = 0;
  let s3 = 0;
  let k = 0;
  const end = dim - 3;
  for (; k < end; k += 4) {
    s0 += q[k] * data[o + k];
    s1 += q[k + 1] * data[o + k + 1];
    s2 += q[k + 2] * data[o + k + 2];
    s3 += q[k + 3] * data[o + k + 3];
  }
  for (; k < dim; k++) s0 += q[k] * data[o + k];
  return s0 + s1 + s2 + s3;
}

/** Dot product of rows `a` and `b` of the same matrix, unrolled by four. */
export function dotRows(data: Float32Array, a: number, b: number, dim: number): number {
  const oa = a * dim;
  const ob = b * dim;
  let s0 = 0;
  let s1 = 0;
  let s2 = 0;
  let s3 = 0;
  let k = 0;
  const end = dim - 3;
  for (; k < end; k += 4) {
    s0 += data[oa + k] * data[ob + k];
    s1 += data[oa + k + 1] * data[ob + k + 1];
    s2 += data[oa + k + 2] * data[ob + k + 2];
    s3 += data[oa + k + 3] * data[ob + k + 3];
  }
  for (; k < dim; k++) s0 += data[oa + k] * data[ob + k];
  return s0 + s1 + s2 + s3;
}

/** Dot product of two equal-length vectors. */
export function dot(a: Float32Array, b: Float32Array): number {
  return dotRow(a, b, 0, Math.min(a.length, b.length));
}

/** Row `i` of a vector set as a view (no copy). */
export function rowOf(vs: VectorSet, i: number): Float32Array {
  return vs.data.subarray(i * vs.dim, (i + 1) * vs.dim);
}

/** Throws unless `vs` is internally consistent. */
export function assertVectorSet(vs: VectorSet): void {
  if (!Number.isInteger(vs.count) || vs.count < 0) throw new RangeError(`invalid vector count ${vs.count}`);
  if (!Number.isInteger(vs.dim) || vs.dim <= 0) throw new RangeError(`invalid vector dimension ${vs.dim}`);
  if (!(vs.data instanceof Float32Array)) throw new TypeError('vector data must be a Float32Array');
  if (vs.data.length !== vs.count * vs.dim) {
    throw new RangeError(`vector data holds ${vs.data.length} values, expected ${vs.count} x ${vs.dim}`);
  }
}

/** mulberry32: small, fast, seeded uniform generator on [0, 1). */
export function seededRandom(seed: number): () => number {
  let a = seed | 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Deterministic [0, 1) jitter from an integer id (murmur3 finaliser). */
export function hash01(i: number): number {
  let x = Math.imul((i + 7) ^ 0x9e3779b9, 0x85ebca6b);
  x ^= x >>> 13;
  x = Math.imul(x, 0xc2b2ae35);
  return ((x ^ (x >>> 16)) >>> 0) / 4294967296;
}

/** 32-bit FNV-1a hash of a string, used to derive a build seed from a storage key. */
export function seedFromString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
