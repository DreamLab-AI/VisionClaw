/**
 * Client for the live RuVector memory cloud (`/api/memory-cloud*`).
 *
 * Requests are authenticated through `computeAuthHeaders`, the same NIP-98 /
 * dev-session branch the shared UnifiedApiClient interceptor uses, so this
 * module never touches a token itself. It issues `fetch` directly rather than
 * going through UnifiedApiClient because that client replaces the caller's
 * AbortSignal with its own controller and retries 5xx responses three times;
 * the memory explorer needs cancellable requests (a newer query or snapshot
 * supersedes an older one) and a binary vectors blob.
 */

import { computeAuthHeaders } from '@/services/api/authInterceptor';
import type {
  MemoryCloudHealth,
  MemoryCloudQueryRequest,
  MemoryCloudQueryResponse,
  MemoryCloudSnapshot,
} from './types';
import type { VectorSet } from '../memoryTrajectory/types';

export const MEMORY_CLOUD_BASE = '/api/memory-cloud';

export type MemoryCloudErrorKind =
  /** non-2xx response not covered by a more specific kind */
  | 'http'
  /** 401/403: the cloud needs power-user access; a quiet state, never a toast */
  | 'forbidden'
  /** 503: the sample is building or the cloud is disabled; honour `retryAfterMs` */
  | 'unavailable'
  /** the request never produced a response */
  | 'network'
  /** the caller aborted */
  | 'abort'
  /** the response did not match the wire contract */
  | 'invalid'
  /** the snapshot was rebuilt twice while fetching its vectors */
  | 'stale';

export class MemoryCloudApiError extends Error {
  readonly kind: MemoryCloudErrorKind;
  readonly status?: number;
  /** 503 only: the server's `Retry-After`, in milliseconds */
  readonly retryAfterMs?: number;
  /** 409 only: the snapshot the server says is current */
  readonly currentSnapshotId?: string;

  constructor(
    kind: MemoryCloudErrorKind,
    message: string,
    status?: number,
    extra: { retryAfterMs?: number; currentSnapshotId?: string } = {},
  ) {
    super(message);
    this.name = kind === 'abort' ? 'AbortError' : 'MemoryCloudApiError';
    this.kind = kind;
    this.status = status;
    this.retryAfterMs = extra.retryAfterMs;
    this.currentSnapshotId = extra.currentSnapshotId;
  }
}

/**
 * `Retry-After` in milliseconds: delta-seconds or an HTTP-date relative to
 * `now`. Undefined when absent or malformed; a past date is 0.
 */
export function parseRetryAfter(value: string | null, now = Date.now()): number | undefined {
  if (value === null) return undefined;
  const v = value.trim();
  if (/^\d+$/.test(v)) return Number(v) * 1000;
  if (!/[a-z]/i.test(v)) return undefined;
  const at = Date.parse(v);
  return Number.isFinite(at) ? Math.max(0, at - now) : undefined;
}

export const isAbortError = (e: unknown): boolean =>
  (e instanceof MemoryCloudApiError && e.kind === 'abort') ||
  (e instanceof Error && e.name === 'AbortError');

export interface RequestOptions {
  signal?: AbortSignal;
}

/**
 * Resolve an API path to an absolute URL. NIP-98 signs the absolute URL, so
 * the URL that is signed and the URL that is fetched must be the same string.
 * A bare relative path (`vectors?snapshot=…`) resolves under the memory-cloud
 * base, which is how the server may phrase `vectorsUrl`.
 */
export function resolveApiUrl(path: string): string {
  const origin = typeof window !== 'undefined' ? window.location.origin : 'http://localhost';
  if (/^https?:\/\//.test(path)) return path;
  if (path.startsWith('/')) return new URL(path, origin).href;
  return new URL(path, `${origin}${MEMORY_CLOUD_BASE}/`).href;
}

function abortError(): MemoryCloudApiError {
  return new MemoryCloudApiError('abort', 'Request aborted');
}

async function request(
  path: string,
  init: { method?: 'GET' | 'POST'; body?: string; accept: string },
  signal?: AbortSignal,
): Promise<Response> {
  if (signal?.aborted) throw abortError();
  const url = resolveApiUrl(path);
  const method = init.method ?? 'GET';
  const headers: Record<string, string> = { Accept: init.accept };
  if (init.body !== undefined) headers['Content-Type'] = 'application/json';
  try {
    Object.assign(headers, await computeAuthHeaders(url, method, init.body));
  } catch {
    // Signing failure: send unsigned and let the server answer 401, which
    // surfaces as a typed http error rather than a silent hang.
  }
  if (signal?.aborted) throw abortError();
  let res: Response;
  try {
    res = await fetch(url, { method, headers, body: init.body, signal });
  } catch (e) {
    if (signal?.aborted || (e instanceof Error && e.name === 'AbortError')) throw abortError();
    throw new MemoryCloudApiError('network', `Network error: ${e instanceof Error ? e.message : String(e)}`);
  }
  return res;
}

async function httpError(res: Response): Promise<MemoryCloudApiError> {
  let detail = res.statusText;
  let currentSnapshotId: string | undefined;
  try {
    const text = await res.text();
    try {
      const j = JSON.parse(text) as { error?: string; message?: string; currentSnapshotId?: unknown };
      detail = j.error ?? j.message ?? (text || detail);
      if (typeof j.currentSnapshotId === 'string') currentSnapshotId = j.currentSnapshotId;
    } catch {
      if (text) detail = text;
    }
  } catch {
    /* body unreadable: keep statusText */
  }
  const kind: MemoryCloudErrorKind =
    res.status === 409
      ? 'stale'
      : res.status === 401 || res.status === 403
        ? 'forbidden'
        : res.status === 503
          ? 'unavailable'
          : 'http';
  return new MemoryCloudApiError(kind, `HTTP ${res.status}: ${detail}`, res.status, {
    retryAfterMs: res.status === 503 ? parseRetryAfter(res.headers.get('Retry-After')) : undefined,
    currentSnapshotId,
  });
}

async function readJson<T>(res: Response, signal?: AbortSignal): Promise<T> {
  if (!res.ok) throw await httpError(res);
  try {
    return (await res.json()) as T;
  } catch (e) {
    if (signal?.aborted) throw abortError();
    throw new MemoryCloudApiError('invalid', `Response was not JSON: ${String(e)}`, res.status);
  }
}

function validateSnapshot(s: MemoryCloudSnapshot): MemoryCloudSnapshot {
  const fail = (why: string) => {
    throw new MemoryCloudApiError('invalid', `Malformed memory-cloud snapshot: ${why}`);
  };
  if (!s || typeof s !== 'object') fail('not an object');
  if (typeof s.snapshotId !== 'string' || !s.snapshotId) fail('missing snapshotId');
  if (!Number.isInteger(s.count) || s.count < 0) fail('count is not a non-negative integer');
  if (!Number.isInteger(s.dim) || s.dim <= 0) fail('dim is not a positive integer');
  if (!Array.isArray(s.positions) || s.positions.length !== s.count * 3)
    fail(`positions has ${s.positions?.length} values, expected ${s.count * 3}`);
  if (!Array.isArray(s.metadata) || s.metadata.length !== s.count)
    fail(`metadata has ${s.metadata?.length} rows, expected ${s.count}`);
  if (typeof s.vectorsUrl !== 'string' || !s.vectorsUrl) fail('missing vectorsUrl');
  return {
    ...s,
    namespaces: s.namespaces ?? [],
    sourceTypes: s.sourceTypes ?? [],
    strata: s.strata ?? [],
    excludedNamespaces: s.excludedNamespaces ?? [],
  };
}

/** `GET /api/memory-cloud` */
export async function fetchSnapshot(opts: RequestOptions = {}): Promise<MemoryCloudSnapshot> {
  const res = await request(MEMORY_CLOUD_BASE, { accept: 'application/json' }, opts.signal);
  return validateSnapshot(await readJson<MemoryCloudSnapshot>(res, opts.signal));
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
export function decodeVectors(buf: ArrayBuffer, count: number, dim: number): VectorSet {
  const expected = count * dim * 4;
  if (buf.byteLength !== expected) {
    throw new MemoryCloudApiError(
      'invalid',
      `Vectors blob is ${buf.byteLength} bytes, expected ${expected} (${count} × ${dim} × 4)`,
    );
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
      throw new MemoryCloudApiError('invalid', `Vectors blob holds a non-finite value at ${i}`);
    }
  }
  const step = Math.max(1, Math.floor(count / NORM_PROBES));
  for (let r = 0; r < count; r += step) {
    let s = 0;
    const o = r * dim;
    for (let k = 0; k < dim; k++) s += data[o + k] * data[o + k];
    if (Math.abs(Math.sqrt(s) - 1) > NORM_TOLERANCE) {
      throw new MemoryCloudApiError(
        'invalid',
        `Vector row ${r} has norm ${Math.sqrt(s).toFixed(4)}; rows must be L2-normalised little-endian f32`,
      );
    }
  }
  return { count, dim, data };
}

/** When the server states the blob's shape in a header, it must match the snapshot. */
function checkShapeHeader(res: Response, name: string, expected: number): void {
  const v = res.headers.get(name);
  if (v === null) return;
  if (Number(v.trim()) !== expected) {
    throw new MemoryCloudApiError('invalid', `${name} is ${v}, but the snapshot says ${expected}`);
  }
}

export interface VectorBundle {
  /** the snapshot the vectors belong to: the one passed in, or its replacement after a 409 */
  snapshot: MemoryCloudSnapshot;
  vectors: VectorSet;
}

/**
 * `GET {snapshot.vectorsUrl}`. A 409 means the sample was rebuilt since the
 * snapshot was read: the snapshot is refetched once and its vectors fetched
 * instead. The bundle returns whichever snapshot the vectors belong to, so the
 * caller never pairs positions from one sample with vectors from another.
 */
export async function fetchVectors(
  snapshot: MemoryCloudSnapshot,
  opts: RequestOptions = {},
): Promise<VectorBundle> {
  let current = snapshot;
  for (let attempt = 0; attempt < 2; attempt++) {
    // request() signs each attempt afresh: NIP-98 tokens are single-use.
    const res = await request(current.vectorsUrl, { accept: 'application/octet-stream' }, opts.signal);
    if (res.status === 409 && attempt === 0) {
      // The body names the current snapshot. When it is the one already held
      // (a rebuild raced the request), retry the same URL; otherwise the
      // vectors must pair with that snapshot's positions and metadata, so
      // refetch it whole.
      const conflict = await httpError(res);
      if (conflict.currentSnapshotId !== current.snapshotId) current = await fetchSnapshot(opts);
      continue;
    }
    if (!res.ok) throw await httpError(res);
    checkShapeHeader(res, 'X-Memory-Cloud-Dim', current.dim);
    checkShapeHeader(res, 'X-Memory-Cloud-Count', current.count);
    let buf: ArrayBuffer;
    try {
      buf = await res.arrayBuffer();
    } catch (e) {
      if (opts.signal?.aborted) throw abortError();
      throw new MemoryCloudApiError('network', `Vectors download failed: ${String(e)}`);
    }
    return { snapshot: current, vectors: decodeVectors(buf, current.count, current.dim) };
  }
  // unreachable: the second iteration either returns or throws
  throw new MemoryCloudApiError('stale', 'Snapshot rebuilt while fetching vectors', 409);
}

export interface QueryOptions extends RequestOptions {
  /** when known, the snapshot dimension the query vector must match */
  expectDim?: number;
}

/** `POST /api/memory-cloud/query` */
export async function postQuery(
  req: MemoryCloudQueryRequest,
  opts: QueryOptions = {},
): Promise<MemoryCloudQueryResponse> {
  const text = req.text?.trim() ?? '';
  if (!text) throw new MemoryCloudApiError('invalid', 'Query text is empty');
  const body: MemoryCloudQueryRequest = { text };
  if (req.k !== undefined) body.k = Math.max(1, Math.min(50, Math.round(req.k)));
  if (req.namespace) body.namespace = req.namespace;
  const json = JSON.stringify(body);
  const res = await request(
    `${MEMORY_CLOUD_BASE}/query`,
    { method: 'POST', body: json, accept: 'application/json' },
    opts.signal,
  );
  const out = await readJson<MemoryCloudQueryResponse>(res, opts.signal);
  const vec = out?.query?.vector;
  if (!Array.isArray(vec) || vec.length === 0) {
    throw new MemoryCloudApiError('invalid', 'Query response carries no embedding');
  }
  if (opts.expectDim !== undefined && vec.length !== opts.expectDim) {
    throw new MemoryCloudApiError(
      'invalid',
      `Query embedding has ${vec.length} dimensions, snapshot has ${opts.expectDim}`,
    );
  }
  if (!Array.isArray(out.sidecar?.results)) {
    throw new MemoryCloudApiError('invalid', 'Query response carries no sidecar results');
  }
  return out;
}

/** `GET /api/memory-cloud/health` */
export async function fetchHealth(opts: RequestOptions = {}): Promise<MemoryCloudHealth> {
  const res = await request(`${MEMORY_CLOUD_BASE}/health`, { accept: 'application/json' }, opts.signal);
  return readJson<MemoryCloudHealth>(res, opts.signal);
}
