/**
 * Memory explorer state: the live snapshot and its vectors, the in-browser
 * trajectory engine built over them, sidecar health, the current query with
 * its route and layout, the view, playback and cinematic state.
 *
 * Dependencies (API calls and the trajectory module) are injected so the
 * store is testable without a network or an HNSW build. The app's instance,
 * wired to the real API and a lazily imported trajectory module, lives in
 * `memoryCloudInstance.ts`.
 */

import { create, type StoreApi, type UseBoundStore } from 'zustand';
import type * as api from './api';
import { isAbortError, type VectorBundle } from './api';
import type {
  MemoryCloudHealth,
  MemoryCloudHit,
  MemoryCloudQueryRequest,
  MemoryCloudQueryResponse,
  MemoryCloudSnapshot,
} from './types';
import type { LayoutResult, QueryRun, TrajectoryView, VectorSet } from '../memoryTrajectory/types';
import type { TrajectoryEngine } from '../memoryTrajectory';
import type { FlashTargets } from './cloudData';

// ── trajectory module contract (memoryTrajectory/index.ts) ──
// Type-only: the store never imports the engine at runtime, it is injected.

export type { TrajectoryEngine };

export type TrajectoryModule = Pick<
  typeof import('../memoryTrajectory'),
  'createTrajectoryEngine' | 'layoutTree' | 'interpolateLayouts'
>;

export interface MemoryCloudDeps {
  fetchSnapshot: (o?: api.RequestOptions) => Promise<MemoryCloudSnapshot>;
  fetchVectors: (s: MemoryCloudSnapshot, o?: api.RequestOptions) => Promise<VectorBundle>;
  postQuery: (r: MemoryCloudQueryRequest, o?: api.QueryOptions) => Promise<MemoryCloudQueryResponse>;
  fetchHealth: (o?: api.RequestOptions) => Promise<MemoryCloudHealth>;
  loadTrajectory: () => Promise<TrajectoryModule>;
}

// ── non-reactive per-frame channels ──
// Written every frame by the scene, read every frame by other scene parts.
// Kept out of zustand so 60 Hz updates never re-render React.

/** beat state, written by the cinematic controller, read for glow and comet pulse */
export const beatState = { on: false, pulse: 0, bar: 0, phase: 0 };

/**
 * Director override: while the cinematic director runs it owns the playback
 * clock (`el`), so the camera timeline and the route reveal stay in lock-step.
 */
export const directorClock = { active: false, el: 0 };

/** Route polyline in cloud-local coordinates, published by TrajectoryLayer for the director. */
export const routeChannel: { pts: Array<[number, number, number]>; seq: number } = { pts: [], seq: 0 };

// ── state ──

export const LAYOUT_RADIUS = 90;
export const RECALL_HISTORY = 30;
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 4;

export type LoadStatus = 'idle' | 'loading' | 'building' | 'ready' | 'error';
export type QueryStatus = 'idle' | 'running' | 'done' | 'error';

export interface QueryState {
  text: string;
  k: number;
  namespace: string | null;
  status: QueryStatus;
  error: string | null;
  response: MemoryCloudQueryResponse | null;
  run: QueryRun | null;
  layout: LayoutResult | null;
  /** layout before the last view change, for the morph */
  prevLayout: LayoutResult | null;
  /** increments on every view change so the scene restarts its morph */
  morphSeq: number;
  /** increments on every completed query */
  seq: number;
}

export interface PlaybackState {
  /** normalised playback seconds (speed already applied) */
  el: number;
  playing: boolean;
  speed: number;
  /** increments on an explicit seek so the scene adopts `el` */
  seekSeq: number;
}

export interface LearningSettings {
  enabled: boolean;
  targetRecall: number;
  learningRate: number;
}

export interface AudioTrackInfo {
  name: string;
  duration: number;
  bpm: number;
  offset: number;
  confidence: number;
}

export interface CinematicState {
  /** director running (fly-along + orbit) */
  active: boolean;
  /** increments to (re)start the director */
  runSeq: number;
  audio: AudioTrackInfo | null;
  audioError: string | null;
  recording: boolean;
  exportProgress: number;
  exportError: string | null;
  lastExport: { name: string; url: string; mime: string } | null;
}

export interface MemoryCloudState {
  status: LoadStatus;
  error: string | null;
  snapshot: MemoryCloudSnapshot | null;
  vectors: VectorSet | null;
  engine: TrajectoryEngine | null;
  trajectory: TrajectoryModule | null;
  /** 0..1 */
  buildProgress: number;
  health: MemoryCloudHealth | null;
  healthError: string | null;
  query: QueryState;
  view: TrajectoryView;
  playback: PlaybackState;
  learning: LearningSettings;
  recallHistory: number[];
  flashes: Record<FlashTargets['match'], number>;
  cinematic: CinematicState;

  loadSnapshot(): Promise<void>;
  refreshHealth(): Promise<void>;
  runQuery(text: string, opts?: { k?: number; namespace?: string | null }): Promise<void>;
  replay(): void;
  clearQuery(): void;
  setView(view: TrajectoryView): void;
  setPlaying(playing: boolean): void;
  seek(el: number): void;
  setSpeed(speed: number): void;
  /** scene → store, throttled: does not bump seekSeq */
  reportPlayback(el: number, playing: boolean): void;
  setLearning(patch: Partial<LearningSettings>): void;
  resetLearning(): void;
  recordFlash(match: FlashTargets['match']): void;
  setCinematic(patch: Partial<CinematicState>): void;
  startCinematic(): void;
  stopCinematic(): void;
  dispose(): void;
}

const emptyQuery = (): QueryState => ({
  text: '',
  k: 10,
  namespace: null,
  status: 'idle',
  error: null,
  response: null,
  run: null,
  layout: null,
  prevLayout: null,
  morphSeq: 0,
  seq: 0,
});

const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** How the sidecar's own top-k relates to the local route over the sample. */
export interface SidecarAgreement {
  total: number;
  /** sidecar results present in the sample */
  inSample: number;
  /** of those, also in the local HNSW top-k */
  inLocal: number;
  /** of those, also in the exact top-k over the sample */
  inExact: number;
}

export function sidecarAgreement(results: MemoryCloudHit[], run: QueryRun | null): SidecarAgreement {
  const local = new Set(run?.result.top ?? []);
  const exact = new Set(run?.exactTop ?? []);
  let inSample = 0;
  let inLocal = 0;
  let inExact = 0;
  for (const r of results) {
    if (r.sampleIndex === null || r.sampleIndex === undefined) continue;
    inSample++;
    if (local.has(r.sampleIndex)) inLocal++;
    if (exact.has(r.sampleIndex)) inExact++;
  }
  return { total: results.length, inSample, inLocal, inExact };
}

const progressFraction = (done: number, total: number): number =>
  total > 0 ? Math.max(0, Math.min(1, done / total)) : 0;

export function createMemoryCloudStore(deps: MemoryCloudDeps): UseBoundStore<StoreApi<MemoryCloudState>> {
  let loadCtl: AbortController | null = null;
  let queryCtl: AbortController | null = null;
  let healthCtl: AbortController | null = null;
  let unsubProgress: (() => void) | null = null;
  let engineSnapshotId: string | null = null;
  let loadPromise: Promise<void> | null = null;

  return create<MemoryCloudState>()((set, get) => {
    const applyLearning = (engine: TrajectoryEngine | null, l: LearningSettings) => {
      if (!engine?.learning) return;
      engine.learning.enabled = l.enabled;
      engine.learning.targetRecall = l.targetRecall;
      engine.learning.learningRate = l.learningRate;
    };

    const releaseEngine = () => {
      unsubProgress?.();
      unsubProgress = null;
      get().engine?.dispose();
      engineSnapshotId = null;
    };

    const computeLayout = (run: QueryRun, view: TrajectoryView): LayoutResult | null => {
      const traj = get().trajectory;
      const snap = get().snapshot;
      if (!traj) return null;
      return traj.layoutTree(run.tree, {
        view,
        spacePositions: snap?.positions,
        radius: LAYOUT_RADIUS,
      });
    };

    const doLoad = async (): Promise<void> => {
      loadCtl?.abort();
      const ctl = new AbortController();
      loadCtl = ctl;
      set({ status: 'loading', error: null });
      try {
        const snap = await deps.fetchSnapshot({ signal: ctl.signal });
        let bundle: VectorBundle;
        if (get().snapshot?.snapshotId === snap.snapshotId && get().vectors && get().engine) {
          bundle = { snapshot: snap, vectors: get().vectors! };
        } else {
          bundle = await deps.fetchVectors(snap, { signal: ctl.signal });
        }
        const traj = get().trajectory ?? (await deps.loadTrajectory());
        if (ctl.signal.aborted) return;
        const id = bundle.snapshot.snapshotId;
        let engine = get().engine;
        if (!engine || engineSnapshotId !== id) {
          releaseEngine();
          engine = traj.createTrajectoryEngine(bundle.vectors, {
            useWorker: true,
            storageKey: `vc-memory-learning:${id}`,
          });
          engineSnapshotId = id;
          unsubProgress = engine.onProgress((done, total) => set({ buildProgress: progressFraction(done, total) }));
          applyLearning(engine, get().learning);
          set({
            snapshot: bundle.snapshot,
            vectors: bundle.vectors,
            engine,
            trajectory: traj,
            buildProgress: 0,
            status: 'building',
            query: { ...emptyQuery(), k: get().query.k, namespace: get().query.namespace },
          });
          await engine.ready;
          if (ctl.signal.aborted || get().engine !== engine) return;
          set({ status: 'ready', buildProgress: 1 });
        } else {
          set({ snapshot: bundle.snapshot, vectors: bundle.vectors, trajectory: traj, status: 'ready' });
        }
      } catch (e) {
        if (isAbortError(e) || ctl.signal.aborted) return;
        set({ status: 'error', error: errorText(e) });
      } finally {
        if (loadCtl === ctl) loadCtl = null;
      }
    };

    return {
      status: 'idle',
      error: null,
      snapshot: null,
      vectors: null,
      engine: null,
      trajectory: null,
      buildProgress: 0,
      health: null,
      healthError: null,
      query: emptyQuery(),
      view: 'canopy',
      playback: { el: 0, playing: false, speed: 1, seekSeq: 0 },
      learning: { enabled: true, targetRecall: 0.9, learningRate: 0.2 },
      recallHistory: [],
      flashes: { key: 0, namespace: 0, none: 0 },
      cinematic: {
        active: false,
        runSeq: 0,
        audio: null,
        audioError: null,
        recording: false,
        exportProgress: 0,
        exportError: null,
        lastExport: null,
      },

      loadSnapshot() {
        const p = doLoad();
        loadPromise = p;
        return p.finally(() => {
          if (loadPromise === p) loadPromise = null;
        });
      },

      async refreshHealth() {
        healthCtl?.abort();
        const ctl = new AbortController();
        healthCtl = ctl;
        try {
          const health = await deps.fetchHealth({ signal: ctl.signal });
          set({ health, healthError: null });
        } catch (e) {
          if (!isAbortError(e)) set({ healthError: errorText(e) });
        }
      },

      async runQuery(text, opts = {}) {
        queryCtl?.abort();
        const ctl = new AbortController();
        queryCtl = ctl;
        const k = opts.k ?? get().query.k;
        const namespace = opts.namespace === undefined ? get().query.namespace : opts.namespace;
        set((s) => ({ query: { ...s.query, text, k, namespace, status: 'running', error: null } }));
        try {
          if (loadPromise) await loadPromise;
          if (!get().engine) await get().loadSnapshot();
          const req: MemoryCloudQueryRequest = { text, k };
          if (namespace) req.namespace = namespace;
          const response = await deps.postQuery(req, { signal: ctl.signal, expectDim: get().snapshot?.dim });
          if (ctl.signal.aborted) return;
          if (response.snapshotId !== get().snapshot?.snapshotId) {
            // The sidecar answered against a newer sample: sampleIndex values
            // refer to it, so adopt it before drawing the route.
            await get().loadSnapshot();
            if (ctl.signal.aborted) return;
          }
          const engine = get().engine;
          if (!engine) throw new Error(get().error ?? 'Trajectory engine unavailable');
          await engine.ready;
          const run = await engine.query(Float32Array.from(response.query.vector), {
            k,
            learn: get().learning.enabled,
          });
          if (ctl.signal.aborted) return;
          const layout = computeLayout(run, get().view);
          set((s) => ({
            query: {
              ...s.query,
              status: 'done',
              response,
              run,
              layout,
              prevLayout: null,
              seq: s.query.seq + 1,
            },
            recallHistory: [...s.recallHistory, run.recall].slice(-RECALL_HISTORY),
            playback: { ...s.playback, el: 0, playing: true, seekSeq: s.playback.seekSeq + 1 },
          }));
        } catch (e) {
          if (isAbortError(e) || ctl.signal.aborted) return;
          set((s) => ({ query: { ...s.query, status: 'error', error: errorText(e) } }));
        } finally {
          if (queryCtl === ctl) queryCtl = null;
        }
      },

      replay() {
        set((s) => ({ playback: { ...s.playback, el: 0, playing: true, seekSeq: s.playback.seekSeq + 1 } }));
      },

      clearQuery() {
        queryCtl?.abort();
        set((s) => ({
          query: { ...emptyQuery(), k: s.query.k, namespace: s.query.namespace },
          playback: { ...s.playback, el: 0, playing: false, seekSeq: s.playback.seekSeq + 1 },
        }));
      },

      setView(view) {
        const { query } = get();
        if (view === get().view) return;
        const layout = query.run ? computeLayout(query.run, view) : null;
        set((s) => ({
          view,
          query: { ...s.query, prevLayout: s.query.layout, layout, morphSeq: s.query.morphSeq + 1 },
        }));
      },

      setPlaying(playing) {
        set((s) => ({ playback: { ...s.playback, playing } }));
      },

      seek(el) {
        set((s) => ({ playback: { ...s.playback, el: Math.max(0, el), seekSeq: s.playback.seekSeq + 1 } }));
      },

      setSpeed(speed) {
        const v = Math.max(MIN_SPEED, Math.min(MAX_SPEED, speed || MIN_SPEED));
        set((s) => ({ playback: { ...s.playback, speed: v } }));
      },

      reportPlayback(el, playing) {
        set((s) => ({ playback: { ...s.playback, el, playing } }));
      },

      setLearning(patch) {
        const learning = { ...get().learning, ...patch };
        learning.targetRecall = Math.max(0.5, Math.min(1, learning.targetRecall));
        learning.learningRate = Math.max(0.01, Math.min(1, learning.learningRate));
        applyLearning(get().engine, learning);
        set({ learning });
      },

      resetLearning() {
        get().engine?.resetLearning();
      },

      recordFlash(match) {
        set((s) => ({ flashes: { ...s.flashes, [match]: s.flashes[match] + 1 } }));
      },

      setCinematic(patch) {
        set((s) => ({ cinematic: { ...s.cinematic, ...patch } }));
      },

      startCinematic() {
        set((s) => ({ cinematic: { ...s.cinematic, active: true, runSeq: s.cinematic.runSeq + 1 } }));
      },

      stopCinematic() {
        set((s) => ({ cinematic: { ...s.cinematic, active: false } }));
      },

      dispose() {
        loadCtl?.abort();
        queryCtl?.abort();
        healthCtl?.abort();
        releaseEngine();
        set({ engine: null, status: 'idle' });
      },
    };
  });
}

