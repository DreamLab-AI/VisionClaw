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
import type { VectorsStep } from './loadTiming';
import { decodeVectorBlob, VectorsInvalidError } from './vectorsCodec';
import { defaultVectorsTransport, type VectorsFetchResult, type VectorsTransport } from './vectorsFetch';

export const MEMORY_CLOUD_BASE = '/api/memory-cloud';

export type MemoryCloudErrorKind =
  /** non-2xx response not covered by a more specific kind */
  | 'http'
  /** 401/403: the cloud needs power-user access; a quiet state, never a toast */
  | 'forbidden'
  /** 503: the sample is building or the cloud is disabled; honour `retryAfterMs` */
  | 'unavailable'
  /**
   * 429: the per-signer query budget is spent. The limiter answers with a
   * plain-text body and usually no `Retry-After`; `retryAfterMs` is set only
   * when the header is present.
   */
  | 'rate_limited'
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
  /** 503 and 429 only: the server's `Retry-After`, in milliseconds */
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
  /**
   * Load-timing hook; `fetchVectors` reports its steps here (loadTiming.ts).
   * `at` is the step's time on this page's `performance.now()` clock when it
   * happened elsewhere (in the vectors worker); absent means "now".
   */
  onStep?: (step: VectorsStep, at?: number) => void;
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

/** `Accept` plus the NIP-98 headers for one request. Signing failure sends it unsigned. */
async function signedHeaders(url: string, method: string, accept: string, body?: string): Promise<Record<string, string>> {
  const headers: Record<string, string> = { Accept: accept };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  try {
    Object.assign(headers, await computeAuthHeaders(url, method, body));
  } catch {
    // Signing failure: send unsigned and let the server answer 401, which
    // surfaces as a typed http error rather than a silent hang.
  }
  return headers;
}

async function request(
  path: string,
  init: { method?: 'GET' | 'POST'; body?: string; accept: string },
  signal?: AbortSignal,
): Promise<Response> {
  if (signal?.aborted) throw abortError();
  const url = resolveApiUrl(path);
  const method = init.method ?? 'GET';
  const headers = await signedHeaders(url, method, init.accept, init.body);
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
  let text = '';
  try {
    text = await res.text();
  } catch {
    /* body unreadable: keep statusText */
  }
  return httpErrorFrom(res.status, res.statusText, text, res.headers.get('Retry-After'));
}

/** The typed error for a non-2xx answer, from its status, body text and `Retry-After`. */
function httpErrorFrom(status: number, statusText: string, text: string, retryAfter: string | null): MemoryCloudApiError {
  let detail = statusText;
  let currentSnapshotId: string | undefined;
  try {
    const j = JSON.parse(text) as { error?: string; message?: string; currentSnapshotId?: unknown };
    detail = j.error ?? j.message ?? (text || detail);
    if (typeof j.currentSnapshotId === 'string') currentSnapshotId = j.currentSnapshotId;
  } catch {
    if (text) detail = text;
  }
  const kind: MemoryCloudErrorKind =
    status === 409
      ? 'stale'
      : status === 401 || status === 403
        ? 'forbidden'
        : status === 503
          ? 'unavailable'
          : status === 429
            ? 'rate_limited'
            : 'http';
  return new MemoryCloudApiError(kind, `HTTP ${status}: ${detail}`, status, {
    retryAfterMs: status === 503 || status === 429 ? parseRetryAfter(retryAfter) : undefined,
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

/**
 * Decode a little-endian f32 row-major blob into a VectorSet, validating the
 * byte length, finiteness and (on a sample of rows) unit norm
 * (`vectorsCodec.ts`); a mismatch is an `invalid` error.
 */
export function decodeVectors(buf: ArrayBuffer, count: number, dim: number): VectorSet {
  try {
    return decodeVectorBlob(buf, count, dim);
  } catch (e) {
    if (e instanceof VectorsInvalidError) throw new MemoryCloudApiError('invalid', e.message);
    throw e;
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
 *
 * The request is signed here and run by `transport`: by default in a worker,
 * so reading and decoding 46 MB never waits on the busy main thread
 * (vectorsFetch.ts).
 */
export async function fetchVectors(
  snapshot: MemoryCloudSnapshot,
  opts: RequestOptions = {},
  transport: VectorsTransport = defaultVectorsTransport(),
): Promise<VectorBundle> {
  const { signal, onStep } = opts;
  let current = snapshot;
  for (let attempt = 0; attempt < 2; attempt++) {
    if (signal?.aborted) throw abortError();
    onStep?.('vectorsStart');
    // Signed afresh for each attempt: NIP-98 tokens are single-use.
    const url = resolveApiUrl(current.vectorsUrl);
    const headers = await signedHeaders(url, 'GET', 'application/octet-stream');
    if (signal?.aborted) throw abortError();
    const r: VectorsFetchResult = await transport({ url, headers, count: current.count, dim: current.dim }, signal);
    if (r.type === 'abort' || signal?.aborted) throw abortError();
    if (r.type === 'http') {
      const err = httpErrorFrom(r.status, r.statusText, r.text, r.retryAfter);
      if (r.status === 409 && attempt === 0) {
        // The body names the current snapshot. When it is the one already held
        // (a rebuild raced the request), retry the same URL; otherwise the
        // vectors must pair with that snapshot's positions and metadata, so
        // refetch it whole.
        if (err.currentSnapshotId !== current.snapshotId) current = await fetchSnapshot(opts);
        continue;
      }
      throw err;
    }
    if (r.type === 'invalid') throw new MemoryCloudApiError('invalid', r.message);
    if (r.type === 'network') {
      throw new MemoryCloudApiError(
        'network',
        r.phase === 'request' ? `Network error: ${r.message}` : `Vectors download failed: ${r.message}`,
      );
    }
    if (onStep) {
      const local = (t: number | undefined) => (t === undefined ? undefined : t - performance.timeOrigin);
      onStep('vectorsHeaders', local(r.at.headers));
      onStep('vectorsBody', local(r.at.body));
      onStep('decoded', local(r.at.decoded));
      onStep('vectorsDelivered');
    }
    return { snapshot: current, vectors: { count: current.count, dim: current.dim, data: r.data } };
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
