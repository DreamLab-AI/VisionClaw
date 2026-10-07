/**
 * The trajectory engine: builds the HNSW over the snapshot's vectors (in a
 * module worker when one is available, otherwise on the main thread in
 * slices), then runs traced, self-learning queries against it.
 *
 * The query loop follows the RuVector Explorer's search-and-learn cycle
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence):
 * recall hints → traced search → exact top-k → recall → learn → tree.
 */
import type { HnswGraph, HnswParams, LearningState, QueryRun, VectorSet } from './types';
import { buildHnsw, exactTopK, recallAtK, searchHnsw } from './hnsw';
import { unpackGraph } from './graphCodec';
import { buildSearchTree } from './tree';
import { createLearningState, learnFrom, recallHints, restoreLearning, serializeLearning } from './learning';
import { assertVectorSet, seedFromString } from './vec';
import type { BuildRequest, BuildResponse } from './workerProtocol';

export interface TrajectoryEngineOptions {
  params?: Partial<HnswParams>;
  /** build in a module worker when available (default true) */
  useWorker?: boolean;
  /** persists learning under this key and seeds the build from it */
  storageKey?: string;
}

export interface TrajectoryQueryOptions {
  /** result count (default 10) */
  k?: number;
  /** beam breadth when learning has not set one (default 48) */
  ef?: number;
  /** feed the outcome back into learning (default true) */
  learn?: boolean;
}

export interface TrajectoryEngine {
  /** resolves once the graph is built; rejects on build failure or dispose */
  ready: Promise<void>;
  onProgress(cb: (done: number, total: number) => void): () => void;
  query(q: Float32Array, opts?: TrajectoryQueryOptions): Promise<QueryRun>;
  /** live learning state; the object identity survives `resetLearning` */
  learning: LearningState;
  resetLearning(): void;
  dispose(): void;
  graph(): HnswGraph | null;
}

export const DEFAULT_HNSW_PARAMS: Omit<HnswParams, 'seed'> = { M: 12, efConstruction: 80 };
export const DEFAULT_EF = 48;
export const DEFAULT_K = 10;
const DEFAULT_SEED_KEY = 'memory-trajectory';
const SAVE_DELAY_MS = 500;

/** localStorage key holding the learning state for `storageKey`. */
export function learningStorageKey(storageKey: string): string {
  return `visionclaw.memoryTrajectory.learning.v1.${storageKey}`;
}

function storage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

function loadLearning(storageKey: string | undefined, vs: VectorSet): LearningState | null {
  if (!storageKey) return null;
  let raw: string | null = null;
  try {
    raw = storage()?.getItem(learningStorageKey(storageKey)) ?? null;
  } catch {
    return null;
  }
  if (!raw) return null;
  const s = restoreLearning(raw, vs.dim);
  if (!s) return null;
  // a snapshot with fewer rows than when the state was saved: drop answers that no longer exist
  s.memory = s.memory.filter((m) => m.ids.every((id) => id < vs.count));
  for (const id of [...s.hubCounts.keys()]) if (id >= vs.count) s.hubCounts.delete(id);
  return s;
}

function disposedError(): Error {
  if (typeof DOMException === 'function') return new DOMException('trajectory engine disposed', 'AbortError');
  const e = new Error('trajectory engine disposed');
  e.name = 'AbortError';
  return e;
}

export function createTrajectoryEngine(vs: VectorSet, opts: TrajectoryEngineOptions = {}): TrajectoryEngine {
  const { storageKey } = opts;
  const params: HnswParams = {
    ...DEFAULT_HNSW_PARAMS,
    seed: seedFromString(storageKey ?? DEFAULT_SEED_KEY),
    ...opts.params,
  };
  const listeners = new Set<(done: number, total: number) => void>();
  const abort = new AbortController();
  let graph: HnswGraph | null = null;
  let disposed = false;
  let worker: Worker | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  const learning: LearningState = loadLearning(storageKey, vs) ?? createLearningState();

  const emit = (done: number, total: number) => {
    for (const cb of listeners) {
      try {
        cb(done, total);
      } catch (e) {
        console.error('[memoryTrajectory] progress listener failed', e);
      }
    }
  };

  const save = () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    if (!storageKey) return;
    try {
      storage()?.setItem(learningStorageKey(storageKey), serializeLearning(learning));
    } catch {
      // private window, quota or blocked storage: learning still works for this visit
    }
  };
  const scheduleSave = () => {
    if (!storageKey) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(save, SAVE_DELAY_MS);
  };

  const buildInWorker = () =>
    new Promise<HnswGraph>((resolve, reject) => {
      const w = new Worker(new URL('./hnsw.worker.ts', import.meta.url), { type: 'module' });
      worker = w;
      const finish = () => {
        abort.signal.removeEventListener('abort', onAbort);
        w.terminate();
        if (worker === w) worker = null;
      };
      const onAbort = () => {
        finish();
        reject(disposedError());
      };
      abort.signal.addEventListener('abort', onAbort);
      w.onmessage = (e: MessageEvent<BuildResponse>) => {
        const msg = e.data;
        if (msg.type === 'progress') emit(msg.done, msg.total);
        else if (msg.type === 'done') {
          finish();
          try {
            resolve(unpackGraph(msg.graph));
          } catch (err) {
            reject(err);
          }
        } else {
          finish();
          reject(new Error(`HNSW worker build failed: ${msg.message}`));
        }
      };
      w.onerror = (e: ErrorEvent) => {
        finish();
        reject(new Error(`HNSW worker crashed: ${e.message || 'unknown error'}`));
      };
      const data = vs.data.slice();
      const req: BuildRequest = { type: 'build', count: vs.count, dim: vs.dim, data, params };
      w.postMessage(req, [data.buffer]);
    });

  const build = async (): Promise<HnswGraph> => {
    // let the caller attach progress listeners (or dispose) before any work starts
    await Promise.resolve();
    if (disposed) throw disposedError();
    assertVectorSet(vs);
    if (opts.useWorker !== false && typeof Worker !== 'undefined') {
      try {
        return await buildInWorker();
      } catch (e) {
        if (disposed) throw e;
        console.warn('[memoryTrajectory] worker build failed, building on the main thread', e);
      }
    }
    return buildHnsw(vs, params, { onProgress: emit, signal: abort.signal });
  };

  const ready = build().then((g) => {
    if (disposed) throw disposedError();
    graph = g;
  });
  // consumers await `ready` themselves; this keeps an unobserved failure from surfacing as unhandled
  ready.catch(() => undefined);

  const engine: TrajectoryEngine = {
    ready,
    learning,

    onProgress(cb) {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },

    async query(q, o = {}) {
      if (disposed) throw disposedError();
      await ready;
      if (disposed || !graph) throw disposedError();
      if (!(q instanceof Float32Array) || q.length !== vs.dim) {
        throw new RangeError(`query dimension ${q?.length} does not match the vector dimension ${vs.dim}`);
      }
      const g = graph;
      const k = Math.max(1, Math.floor(o.k ?? DEFAULT_K));
      const learn = o.learn ?? true;
      const { hints } = recallHints(learning, q);
      const requested = learning.breadth ?? o.ef ?? DEFAULT_EF;
      const breadth = Math.max(k, Math.round(Number.isFinite(requested) ? requested : DEFAULT_EF));
      const result = searchHnsw(g, vs, q, breadth, k, { trace: true, hints });
      const exactTop = exactTopK(vs, q, k);
      const recall = recallAtK(result.top, exactTop, k);
      if (learn && learning.enabled) {
        learnFrom(learning, q, result, recall, k);
        scheduleSave();
      }
      const tree = buildSearchTree(result, g.ep);
      return { result, tree, exactTop, recall, breadth, hintsUsed: hints };
    },

    resetLearning() {
      const fresh = createLearningState({
        enabled: learning.enabled,
        capacity: learning.capacity,
        hintsPerQuery: learning.hintsPerQuery,
        targetRecall: learning.targetRecall,
        learningRate: learning.learningRate,
        controller: learning.controller,
      });
      Object.assign(learning, fresh);
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = null;
      if (storageKey) {
        try {
          storage()?.removeItem(learningStorageKey(storageKey));
        } catch {
          // blocked storage: nothing persisted to clear
        }
      }
    },

    dispose() {
      if (disposed) return;
      if (saveTimer) save();
      disposed = true;
      abort.abort();
      worker?.terminate();
      worker = null;
      listeners.clear();
    },

    graph() {
      return graph;
    },
  };
  return engine;
}
