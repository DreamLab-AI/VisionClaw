/**
 * Fetch, read and decode the vectors blob off the main thread.
 *
 * Chrome drains a `fetch` body through tasks on the thread that reads it.
 * In the app the main thread is busy with the graph scene, and a 46 MB body
 * read there took 18.5 s; read in a worker, in the same tab and the same
 * moment, it took 1.9 s (2026-10-08, 30,000 x 384 snapshot). So the vectors
 * are fetched, validated and decoded in `vectors.worker.ts`, and the decoded
 * `Float32Array` is transferred back without a copy.
 *
 * Signing stays on the main thread (NIP-98 tokens are single-use and the
 * signer lives there); the worker gets the finished headers. 409, error typing
 * and step reporting stay in `api.ts`. When `Worker` is missing or the worker
 * script fails to load, the same `runVectorsFetch` runs in-thread.
 *
 * Dependency-light on purpose: the worker imports this module.
 */

import { decodeVectorBlob } from './vectorsCodec';

export interface VectorsFetchRequest {
  url: string;
  headers: Record<string, string>;
  count: number;
  dim: number;
}

/** Absolute times (`performance.timeOrigin + performance.now()`) of each step reached. */
export interface VectorsStepTimes {
  headers?: number;
  body?: number;
  decoded?: number;
}

export type VectorsFetchResult =
  | { type: 'ok'; data: Float32Array; at: VectorsStepTimes }
  | { type: 'http'; status: number; statusText: string; text: string; retryAfter: string | null; at: VectorsStepTimes }
  | { type: 'invalid'; message: string; at: VectorsStepTimes }
  | { type: 'network'; phase: 'request' | 'body'; message: string; at: VectorsStepTimes }
  | { type: 'abort' };

export type VectorsTransport = (req: VectorsFetchRequest, signal?: AbortSignal) => Promise<VectorsFetchResult>;

/** Absolute ms, comparable across the page and its workers. */
export function epochNow(): number {
  return performance.timeOrigin + performance.now();
}

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));
// By name: a DOMException is not an Error in every runtime.
const isAbort = (e: unknown, signal?: AbortSignal): boolean =>
  !!signal?.aborted || (typeof e === 'object' && e !== null && (e as { name?: unknown }).name === 'AbortError');

/** One request: headers, shape check, body, decode. Never throws. */
export async function runVectorsFetch(
  req: VectorsFetchRequest,
  fetchImpl: typeof fetch,
  signal?: AbortSignal,
  clock: () => number = epochNow,
): Promise<VectorsFetchResult> {
  const at: VectorsStepTimes = {};
  let res: Response;
  try {
    res = await fetchImpl(req.url, { method: 'GET', headers: req.headers, signal });
  } catch (e) {
    return isAbort(e, signal) ? { type: 'abort' } : { type: 'network', phase: 'request', message: message(e), at };
  }
  at.headers = clock();
  if (!res.ok) {
    let text = '';
    try {
      text = await res.text();
    } catch {
      /* body unreadable: statusText stands in */
    }
    return {
      type: 'http',
      status: res.status,
      statusText: res.statusText,
      text,
      retryAfter: res.headers.get('Retry-After'),
      at,
    };
  }
  // When the server states the blob's shape in a header, it must match the snapshot.
  for (const [name, expected] of [
    ['X-Memory-Cloud-Dim', req.dim],
    ['X-Memory-Cloud-Count', req.count],
  ] as const) {
    const v = res.headers.get(name);
    if (v !== null && Number(v.trim()) !== expected) {
      return { type: 'invalid', message: `${name} is ${v}, but the snapshot says ${expected}`, at };
    }
  }
  let buf: ArrayBuffer;
  try {
    buf = await res.arrayBuffer();
  } catch (e) {
    return isAbort(e, signal) ? { type: 'abort' } : { type: 'network', phase: 'body', message: String(e), at };
  }
  at.body = clock();
  try {
    const { data } = decodeVectorBlob(buf, req.count, req.dim);
    at.decoded = clock();
    return { type: 'ok', data, at };
  } catch (e) {
    return { type: 'invalid', message: message(e), at };
  }
}

/** Runs in the calling thread, against the global `fetch` at call time. */
export const inThreadTransport: VectorsTransport = (req, signal) =>
  runVectorsFetch(req, (input, init) => globalThis.fetch(input, init), signal);

/** Worker → page: `started` once the script has loaded, then one result. */
export type VectorsWorkerMessage = { type: 'started' } | VectorsFetchResult;

/**
 * Runs each request in a fresh worker from `spawn`, terminated when it
 * answers or the signal aborts. A worker that fails before it reports
 * `started` (script did not load) falls back to `fallback`; its signed
 * headers were never sent, so they are still unused.
 */
export function createWorkerTransport(
  spawn: () => Worker,
  fallback: VectorsTransport = inThreadTransport,
): VectorsTransport {
  return (req, signal) =>
    new Promise<VectorsFetchResult>((resolve) => {
      if (signal?.aborted) {
        resolve({ type: 'abort' });
        return;
      }
      const runFallback = (why: string) => {
        console.warn(`[memoryCloud] vectors worker unavailable (${why}); fetching on the main thread`);
        void fallback(req, signal).then(resolve);
      };
      let w: Worker;
      try {
        w = spawn();
      } catch (e) {
        runFallback(message(e));
        return;
      }
      let started = false;
      let settled = false;
      const settle = () => {
        settled = true;
        signal?.removeEventListener('abort', onAbort);
        w.terminate();
      };
      const onAbort = () => {
        if (settled) return;
        settle();
        resolve({ type: 'abort' });
      };
      signal?.addEventListener('abort', onAbort);
      w.onmessage = (e: MessageEvent<VectorsWorkerMessage>) => {
        if (settled) return;
        if (e.data.type === 'started') {
          started = true;
          return;
        }
        settle();
        resolve(e.data);
      };
      w.onerror = (e: ErrorEvent) => {
        e.preventDefault?.();
        if (settled) return;
        settle();
        if (!started) runFallback(e.message || 'worker script failed to load');
        else resolve({ type: 'network', phase: 'body', message: `vectors worker crashed: ${e.message || 'unknown error'}`, at: {} });
      };
      w.postMessage(req);
    });
}

/** Worker when the runtime has one (the browser), else in-thread (tests, SSR). */
export function defaultVectorsTransport(): VectorsTransport {
  if (typeof Worker === 'undefined') return inThreadTransport;
  return createWorkerTransport(
    () => new Worker(new URL('./vectors.worker.ts', import.meta.url), { type: 'module', name: 'memory-cloud-vectors' }),
  );
}
