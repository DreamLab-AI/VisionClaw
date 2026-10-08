/**
 * Timing for one memory-cloud load, from `loadSnapshot()` to a ready engine.
 *
 * Each step records the first time it happens, in ms from the load start.
 * The finished record is logged once as a single `console.debug` line
 * (DevTools level "Verbose") and kept on the store (`lastLoadTiming`) so
 * tests and the console can read it. Each mark is also a
 * `performance.mark('memory-cloud:<step>')` for the DevTools timeline.
 *
 * Steps, in load order:
 * - `snapshot`: `GET /api/memory-cloud` parsed
 * - `vectorsStart` / `vectorsHeaders` / `vectorsBody`: the vectors request
 *   sent, its headers received, its body fully read
 * - `decoded`: blob decoded and validated
 *   (these three are timed in the vectors worker, on the page clock)
 * - `vectorsDelivered`: the decoded vectors reached the main thread; the gap
 *   from `decoded` is how long the busy main thread took to take them
 * - `trajectoryModule`: the lazily imported HNSW module resolved (imported
 *   alongside the vectors fetch, so not ordered against the vectors steps)
 * - `engineCreated`: engine constructed; the vectors are copied and
 *   transferred to the build worker on its first microtask
 * - `firstProgress`: the worker's first progress tick
 * - `ready`: the HNSW graph is built
 */

export const LOAD_STEPS = [
  'snapshot',
  'vectorsStart',
  'vectorsHeaders',
  'vectorsBody',
  'decoded',
  'vectorsDelivered',
  'trajectoryModule',
  'engineCreated',
  'firstProgress',
  'ready',
] as const;

export type LoadStep = (typeof LOAD_STEPS)[number];

/** Steps reported by `fetchVectors` through `RequestOptions.onStep`. */
export type VectorsStep = Extract<
  LoadStep,
  'vectorsStart' | 'vectorsHeaders' | 'vectorsBody' | 'decoded' | 'vectorsDelivered'
>;

export type LoadOutcome = 'ready' | 'error' | 'aborted' | 'forbidden' | 'unavailable';

export interface LoadTiming {
  snapshotId: string | null;
  outcome: LoadOutcome;
  /** ms from the load start to the first occurrence of each step */
  marks: Partial<Record<LoadStep, number>>;
  /** vectors requests sent (2 when a 409 forced a retry) */
  vectorsRequests: number;
  /** true when the held vectors were reused and no request was made */
  vectorsReused: boolean;
  totalMs: number;
}

export interface LoadTimer {
  /** Record `step` at `at` (same clock as `now`), or now when absent. */
  mark(step: LoadStep, at?: number): void;
  setSnapshotId(id: string): void;
  setVectorsReused(): void;
  finish(outcome: LoadOutcome): LoadTiming;
}

const defaultNow = (): number =>
  typeof performance !== 'undefined' && typeof performance.now === 'function' ? performance.now() : Date.now();

export function createLoadTimer(now: () => number = defaultNow): LoadTimer {
  const t0 = now();
  const marks: Partial<Record<LoadStep, number>> = {};
  let snapshotId: string | null = null;
  let vectorsRequests = 0;
  let vectorsReused = false;
  let finished: LoadTiming | null = null;
  return {
    mark(step, at) {
      if (finished) return;
      if (step === 'vectorsStart') vectorsRequests++;
      if (marks[step] !== undefined) return;
      const when = at ?? now();
      marks[step] = Math.round(when - t0);
      try {
        performance.mark(`memory-cloud:${step}`, at === undefined ? undefined : { startTime: when });
      } catch {
        // no User Timing API (old runtime): the record still holds the mark
      }
    },
    setSnapshotId(id) {
      snapshotId = id;
    },
    setVectorsReused() {
      vectorsReused = true;
    },
    finish(outcome) {
      if (!finished) {
        finished = {
          snapshotId,
          outcome,
          marks: { ...marks },
          vectorsRequests,
          vectorsReused,
          totalMs: Math.round(now() - t0),
        };
      }
      return finished;
    },
  };
}

/** One log line: `snapshot=120 vectorsStart=121 … ready=9000`, in step order. */
export function formatLoadTiming(t: LoadTiming): string {
  const steps = LOAD_STEPS.filter((s) => t.marks[s] !== undefined)
    .map((s) => `${s}=${t.marks[s]}`)
    .join(' ');
  const vec = t.vectorsReused ? 'reused' : `${t.vectorsRequests} request(s)`;
  return `[memoryCloud] load ${t.snapshotId ?? '?'} ${t.outcome} in ${t.totalMs} ms; vectors ${vec}; ms: ${steps}`;
}
