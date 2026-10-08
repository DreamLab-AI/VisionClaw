/**
 * Decode and validate the memory-cloud vectors blob. Dependency-free so the
 * vectors worker (vectors.worker.ts) can import it without pulling in the
 * API client, its auth interceptor or the store.
 */

import type { VectorSet } from '../memoryTrajectory/types';

/** The blob does not match the wire contract; `api.ts` maps it to kind `invalid`. */
export class VectorsInvalidError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'VectorsInvalidError';
  }
}

const HOST_IS_LITTLE_ENDIAN = new Uint8Array(new Uint32Array([1]).buffer)[0] === 1;

/** rows whose norm is checked; enough to catch a byte-swapped or mis-strided blob */
const NORM_PROBES = 8;
const NORM_TOLERANCE = 0.02;

/**
 * Decode a little-endian f32 row-major blob into a VectorSet, validating the
 * byte length, finiteness and (on a sample of rows) unit norm. Byte-swapped
 * floats are almost never unit length, so the norm probe doubles as the
 * endianness check.
 */
export function decodeVectorBlob(buf: ArrayBuffer, count: number, dim: number): VectorSet {
  const expected = count * dim * 4;
  if (buf.byteLength !== expected) {
    throw new VectorsInvalidError(`Vectors blob is ${buf.byteLength} bytes, expected ${expected} (${count} × ${dim} × 4)`);
  }
  let data: Float32Array;
  if (HOST_IS_LITTLE_ENDIAN) {
    data = new Float32Array(buf);
  } else {
    const dv = new DataView(buf);
    data = new Float32Array(count * dim);
    for (let i = 0; i < data.length; i++) data[i] = dv.getFloat32(i * 4, true);
  }
  for (let i = 0; i < data.length; i++) {
    if (!Number.isFinite(data[i])) {
      throw new VectorsInvalidError(`Vectors blob holds a non-finite value at ${i}`);
    }
  }
  const step = Math.max(1, Math.floor(count / NORM_PROBES));
  for (let r = 0; r < count; r += step) {
    let s = 0;
    const o = r * dim;
    for (let k = 0; k < dim; k++) s += data[o + k] * data[o + k];
    if (Math.abs(Math.sqrt(s) - 1) > NORM_TOLERANCE) {
      throw new VectorsInvalidError(
        `Vector row ${r} has norm ${Math.sqrt(s).toFixed(4)}; rows must be L2-normalised little-endian f32`,
      );
    }
  }
  return { count, dim, data };
}
