import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { MemoryCloudApiError } from '../api';
import { createMemoryCloudStore, sidecarAgreement, describeAgreement, QUERY_RETRY_DEFAULT_MS, type MemoryCloudDeps, type TrajectoryModule } from '../memoryCloudStore';
import type { MemoryCloudSnapshot, MemoryCloudQueryResponse, MemoryCloudHit } from '../types';
import type { QueryRun, SearchTree, LayoutResult, LearningState, VectorSet } from '../../memoryTrajectory/types';

const DIM = 4;

function snapshot(id: string, count = 3): MemoryCloudSnapshot {
  return {
    version: 1, snapshotId: id, generatedAt: 1, dim: DIM, count,
    positions: Array.from({ length: count * 3 }, (_, i) => i),
    metadata: Array.from({ length: count }, (_, i) => ({ id: `id${i}`, key: `k${i}`, namespace: 'ns', sourceType: 'memory', updatedAt: i })),
    namespaces: ['ns'], sourceTypes: ['memory'], strata: [], excludedNamespaces: [],
    vectorsUrl: `/api/memory-cloud/vectors?snapshot=${id}`,
  };
}

const vectors = (count: number): VectorSet => ({ count, dim: DIM, data: new Float32Array(count * DIM) });

function hit(sampleIndex: number | null, key = 'k'): MemoryCloudHit {
  return { id: key, key, namespace: 'ns', sourceType: 'memory', score: 0.9, snippet: 's', sampleIndex };
}

function response(snapshotId = 's1', results: MemoryCloudHit[] = [hit(0), hit(2), hit(null)]): MemoryCloudQueryResponse {
  return {
    snapshotId, embedModel: 'bge',
    query: { text: 'q', vector: [1, 0, 0, 0] },
    sidecar: { results, tookMs: 3, method: 'hnsw' },
  };
}

const tree = { root: 0, nodes: new Map(), list: [], path: [0, 2], maxDepth: 1, leaves: 1, keptTotal: 2 } as unknown as SearchTree;
const run = (recall = 0.5): QueryRun => ({
  result: { top: [2, 1], beam: [2, 1], hops: [0], distanceEvals: 12, trace: [] },
  tree, exactTop: [2, 0], recall, breadth: 32, hintsUsed: [],
});
const layoutFor = (view: string): LayoutResult => ({
  positions: new Map([[0, [view.length, 0, 0]]]), controls: new Map(),
});

function learning(): LearningState {
  return {
    enabled: true, memory: [], capacity: 64, hintsPerQuery: 3, targetRecall: 0.9, learningRate: 0.2,
    breadth: null, controller: 'ewma', ewma: null, hubCounts: new Map(),
  };
}

function makeDeps() {
  const engines: Array<{ dispose: ReturnType<typeof vi.fn>; query: ReturnType<typeof vi.fn>; learning: LearningState; storageKey?: string; resetLearning: ReturnType<typeof vi.fn>; emit: (done: number, total: number) => void }> = [];
  const traj: TrajectoryModule = {
    createTrajectoryEngine: vi.fn((_vs: VectorSet, opts?: { storageKey?: string }) => {
      let cb: ((done: number, total: number) => void) | null = null;
      const e = {
        ready: Promise.resolve(),
        onProgress: (f: (done: number, total: number) => void) => { cb = f; return () => { cb = null; }; },
        query: vi.fn(async () => run()),
        learning: learning(),
        resetLearning: vi.fn(),
        dispose: vi.fn(),
        graph: () => null,
        storageKey: opts?.storageKey,
        emit: (done: number, total: number) => cb?.(done, total),
      };
      engines.push(e);
      return e as never;
    }),
    layoutTree: vi.fn((_t: SearchTree, o: { view: string }) => layoutFor(o.view)),
    interpolateLayouts: vi.fn((a: LayoutResult) => a),
  } as unknown as TrajectoryModule;
  const deps: MemoryCloudDeps = {
    fetchSnapshot: vi.fn(async () => snapshot('s1')),
    fetchVectors: vi.fn(async (s: MemoryCloudSnapshot) => ({ snapshot: s, vectors: vectors(s.count) })),
    postQuery: vi.fn(async () => response()),
    fetchHealth: vi.fn(async () => ({ snapshotId: 's1' } as never)),
    loadTrajectory: vi.fn(async () => traj),
  };
  return { deps, traj, engines };
}

describe('memoryCloudStore', () => {
  let env: ReturnType<typeof makeDeps>;
  beforeEach(() => { env = makeDeps(); });

  it('loads snapshot and vectors, then builds one engine keyed by snapshot', async () => {
    const store = createMemoryCloudStore(env.deps);
    expect(store.getState().status).toBe('idle');
    const p = store.getState().loadSnapshot();
    expect(store.getState().status).toBe('loading');
    await p;
    const s = store.getState();
    expect(s.status).toBe('ready');
    expect(s.snapshot?.snapshotId).toBe('s1');
    expect(s.vectors?.count).toBe(3);
    expect(env.engines.length).toBe(1);
    expect(env.engines[0].storageKey).toBe('vc-memory-learning:s1');
  });

  it('records the sample size from the snapshot header before the vectors arrive', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const fetchVectors = env.deps.fetchVectors;
    env.deps.fetchVectors = vi.fn(async (snap, opts) => {
      await gate;
      return fetchVectors(snap, opts);
    });
    const store = createMemoryCloudStore(env.deps);
    const p = store.getState().loadSnapshot();
    await vi.waitFor(() => expect(store.getState().loadingCount).toBe(3));
    expect(store.getState().status).toBe('loading');
    expect(store.getState().loadingBytes).toBe(3 * DIM * 4);
    release();
    await p;
    expect(store.getState().status).toBe('ready');
  });

  it('records build progress from the engine', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    env.engines[0].emit(2, 5);
    expect(store.getState().buildProgress).toBeCloseTo(0.4);
    env.engines[0].emit(3, 4);
    expect(store.getState().buildProgress).toBeCloseTo(0.75);
    env.engines[0].emit(0, 0);
    expect(store.getState().buildProgress).toBe(0);
  });

  it('keeps one timing record per load, steps in load order, and logs it as one line', async () => {
    const info = vi.spyOn(console, 'debug').mockImplementation(() => {});
    let progress: ((d: number, t: number) => void) | null = null;
    (env.traj.createTrajectoryEngine as ReturnType<typeof vi.fn>).mockImplementationOnce(() => ({
      ready: new Promise<void>((r) => setTimeout(() => { progress?.(1, 2); r(); }, 0)),
      onProgress: (f: (d: number, t: number) => void) => { progress = f; return () => { progress = null; }; },
      learning: learning(), resetLearning: vi.fn(), dispose: vi.fn(), query: vi.fn(), graph: () => null,
    }) as never);
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockImplementationOnce(
      async (s: MemoryCloudSnapshot, o?: { onStep?: (step: string) => void }) => {
        for (const step of ['vectorsStart', 'vectorsHeaders', 'vectorsBody', 'decoded', 'vectorsDelivered']) o?.onStep?.(step);
        return { snapshot: s, vectors: vectors(s.count) };
      },
    );
    const store = createMemoryCloudStore(env.deps);
    expect(store.getState().lastLoadTiming).toBeNull();
    await store.getState().loadSnapshot();
    const t = store.getState().lastLoadTiming!;
    expect(t.outcome).toBe('ready');
    expect(t.snapshotId).toBe('s1');
    expect(t.vectorsRequests).toBe(1);
    expect(t.vectorsReused).toBe(false);
    const order = ['snapshot', 'vectorsStart', 'vectorsHeaders', 'vectorsBody', 'decoded', 'vectorsDelivered',
      'engineCreated', 'firstProgress', 'ready'] as const;
    for (const step of order) expect(t.marks[step], step).toBeTypeOf('number');
    const times = order.map((s) => t.marks[s]!);
    expect([...times].sort((a, b) => a - b)).toEqual(times);
    // the trajectory import runs alongside the vectors fetch, so it is timed but not ordered
    expect(t.marks.trajectoryModule).toBeTypeOf('number');
    expect(info).toHaveBeenCalledTimes(1);
    expect(String(info.mock.calls[0][0])).toMatch(/^\[memoryCloud\] load s1 ready in \d+ ms; vectors 1 request\(s\); ms: snapshot=/);

    // a reload of the same snapshot reuses the vectors and records that
    await store.getState().loadSnapshot();
    expect(store.getState().lastLoadTiming!.vectorsReused).toBe(true);
    expect(store.getState().lastLoadTiming!.vectorsRequests).toBe(0);
    info.mockRestore();
  });

  it('starts the trajectory import alongside the vectors fetch, not after it', async () => {
    let releaseVectors!: () => void;
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockImplementationOnce(
      (s: MemoryCloudSnapshot) => new Promise((res) => { releaseVectors = () => res({ snapshot: s, vectors: vectors(s.count) }); }),
    );
    const store = createMemoryCloudStore(env.deps);
    const p = store.getState().loadSnapshot();
    await vi.waitFor(() => expect(env.deps.fetchVectors).toHaveBeenCalled());
    expect(env.deps.loadTrajectory).toHaveBeenCalledTimes(1);
    releaseVectors();
    await p;
    expect(store.getState().status).toBe('ready');
    expect(env.deps.loadTrajectory).toHaveBeenCalledTimes(1);
  });

  it('a failed trajectory import while the vectors fail surfaces the vectors error, unhandled-free', async () => {
    const info = vi.spyOn(console, 'debug').mockImplementation(() => {});
    (env.deps.loadTrajectory as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('chunk load failed'));
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new MemoryCloudApiError('invalid', 'bad blob'));
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().status).toBe('error');
    expect(store.getState().error).toMatch(/bad blob/);
    info.mockRestore();
  });

  it('records the outcome of a failed load', async () => {
    const info = vi.spyOn(console, 'debug').mockImplementation(() => {});
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new MemoryCloudApiError('forbidden', 'HTTP 403: no', 403));
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().lastLoadTiming!.outcome).toBe('forbidden');
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new MemoryCloudApiError('invalid', 'bad blob'));
    await store.getState().loadSnapshot();
    expect(store.getState().lastLoadTiming!.outcome).toBe('error');
    info.mockRestore();
  });

  it('reuses the engine for the same snapshot and disposes it on change', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    await store.getState().loadSnapshot();
    expect(env.engines.length).toBe(1);
    (env.deps.fetchSnapshot as ReturnType<typeof vi.fn>).mockResolvedValueOnce(snapshot('s2', 5));
    await store.getState().loadSnapshot();
    expect(env.engines[0].dispose).toHaveBeenCalledTimes(1);
    expect(env.engines.length).toBe(2);
    expect(env.engines[1].storageKey).toBe('vc-memory-learning:s2');
  });

  it('uses the snapshot returned with the vectors after a 409 refresh', async () => {
    (env.deps.fetchVectors as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => ({ snapshot: snapshot('s9', 2), vectors: vectors(2) }));
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().snapshot?.snapshotId).toBe('s9');
    expect(env.engines[0].storageKey).toBe('vc-memory-learning:s9');
  });

  it('surfaces load errors', async () => {
    (env.deps.fetchSnapshot as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new MemoryCloudApiError('http', 'HTTP 503: down', 503));
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().status).toBe('error');
    expect(store.getState().error).toContain('503');
  });

  it('runs a query: sidecar, local route, layout, recall history, playback from zero', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    store.getState().seek(5);
    await store.getState().runQuery('what did we decide', { k: 7, namespace: 'ns' });
    const s = store.getState();
    expect(env.deps.postQuery).toHaveBeenCalledWith({ text: 'what did we decide', k: 7, namespace: 'ns' }, expect.objectContaining({ expectDim: DIM }));
    const [q, opts] = env.engines[0].query.mock.calls[0];
    expect(q).toBeInstanceOf(Float32Array);
    expect(opts).toMatchObject({ k: 7, learn: true });
    expect(s.query.status).toBe('done');
    expect(s.query.run?.recall).toBe(0.5);
    expect(s.query.layout).toEqual(layoutFor('canopy'));
    expect(s.recallHistory).toEqual([0.5]);
    expect(s.playback.el).toBe(0);
    expect(s.playback.playing).toBe(true);
  });

  it('reloads the snapshot when the sidecar answers for a newer one', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    (env.deps.postQuery as ReturnType<typeof vi.fn>).mockResolvedValueOnce(response('s2'));
    (env.deps.fetchSnapshot as ReturnType<typeof vi.fn>).mockResolvedValueOnce(snapshot('s2'));
    await store.getState().runQuery('x');
    expect(store.getState().snapshot?.snapshotId).toBe('s2');
    expect(env.engines.length).toBe(2);
    expect(env.engines[1].query).toHaveBeenCalled();
    expect(store.getState().query.status).toBe('done');
  });

  it('a newer query aborts the older one', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    const signals: AbortSignal[] = [];
    (env.deps.postQuery as ReturnType<typeof vi.fn>).mockImplementation(async (_r: unknown, o: { signal: AbortSignal }) => {
      signals.push(o.signal);
      await new Promise((r) => setTimeout(r, 5));
      if (o.signal.aborted) throw new MemoryCloudApiError('abort', 'aborted');
      return response();
    });
    const a = store.getState().runQuery('first');
    const b = store.getState().runQuery('second');
    await Promise.all([a, b]);
    expect(signals[0].aborted).toBe(true);
    expect(store.getState().query.text).toBe('second');
    expect(store.getState().query.status).toBe('done');
  });

  it('switching view keeps the previous layout for the morph', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    await store.getState().runQuery('x');
    store.getState().setView('tree');
    const s = store.getState();
    expect(s.view).toBe('tree');
    expect(s.query.layout).toEqual(layoutFor('tree'));
    expect(s.query.prevLayout).toEqual(layoutFor('canopy'));
    expect(s.query.morphSeq).toBe(1);
  });

  it('mirrors learning settings onto the engine and resets it', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    store.getState().setLearning({ enabled: false, targetRecall: 0.8, learningRate: 0.5 });
    expect(env.engines[0].learning).toMatchObject({ enabled: false, targetRecall: 0.8, learningRate: 0.5 });
    await store.getState().runQuery('x');
    expect(env.engines[0].query.mock.calls[0][1]).toMatchObject({ learn: false });
    store.getState().resetLearning();
    expect(env.engines[0].resetLearning).toHaveBeenCalled();
  });

  it('playback: seek bumps the seek sequence, speed clamps, tick advances only while playing', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().seek(3);
    expect(store.getState().playback).toMatchObject({ el: 3, seekSeq: 1 });
    store.getState().setSpeed(100);
    expect(store.getState().playback.speed).toBe(4);
    store.getState().setSpeed(0);
    expect(store.getState().playback.speed).toBe(0.25);
    store.getState().reportPlayback(4.2, false);
    expect(store.getState().playback).toMatchObject({ el: 4.2, playing: false, seekSeq: 1 });
  });

  it('counts flashes by match kind', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().recordFlash('key');
    store.getState().recordFlash('none');
    store.getState().recordFlash('none');
    store.getState().recordFlash('namespace');
    expect(store.getState().flashes).toEqual({ key: 1, namespace: 1, none: 2 });
  });

  it('caps recall history at 30', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    for (let i = 0; i < 35; i++) await store.getState().runQuery(`q${i}`);
    expect(store.getState().recallHistory.length).toBe(30);
  });

  it('dispose releases the engine', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    store.getState().dispose();
    expect(env.engines[0].dispose).toHaveBeenCalled();
    expect(store.getState().engine).toBeNull();
  });
});

describe('sidecarAgreement', () => {
  it('compares the sidecar top-k with the local and exact top-k over the sample', () => {
    const a = sidecarAgreement(response('s1', [hit(0), hit(2), hit(null), hit(1)]).sidecar.results, run());
    expect(a).toEqual({ total: 4, inSample: 3, inLocal: 2, inExact: 2 });
  });
  it('handles a missing run', () => {
    expect(sidecarAgreement([hit(0)], null)).toEqual({ total: 1, inSample: 1, inLocal: 0, inExact: 0 });
  });
});

describe('describeAgreement', () => {
  it('states coverage over all hits and agreement over the sampled ones only', () => {
    const d = describeAgreement({ total: 10, inSample: 4, inLocal: 3, inExact: 4 });
    expect(d.coverage).toBe('4 of 10 sidecar hits are in the sample');
    expect(d.agreement).toBe('3 of 4 sampled agree with the local top-k');
  });

  it('never reports 0/0: with no sampled hit there is no agreement to measure', () => {
    // real queries mostly return ruvnet-kb rows the 6k sample does not hold
    const d = describeAgreement({ total: 10, inSample: 0, inLocal: 0, inExact: 0 });
    expect(d.coverage).toBe('0 of 10 sidecar hits are in the sample');
    expect(d.agreement).toBe('no sampled hit to compare with the local top-k');
    expect(d.agreement).not.toMatch(/0\s*\/\s*0|0 of 0/);
  });

  it('uses the singular for one hit', () => {
    expect(describeAgreement({ total: 1, inSample: 1, inLocal: 1, inExact: 1 }).coverage).toBe('1 of 1 sidecar hit is in the sample');
  });
});

describe('memoryCloudStore — backend availability', () => {
  let env: ReturnType<typeof makeDeps>;
  beforeEach(() => { env = makeDeps(); });
  afterEach(() => { vi.useRealTimers(); });

  it('treats 401/403 as a quiet forbidden state with no error text', async () => {
    env.deps.fetchSnapshot = vi.fn(async () => {
      throw new MemoryCloudApiError('forbidden', 'HTTP 403: power user required', 403);
    });
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().status).toBe('forbidden');
    expect(store.getState().error).toBeNull();
    expect(store.getState().retryAt).toBeNull();
  });

  it('a query refused with 403 drops to the forbidden state, not a query error', async () => {
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    env.deps.postQuery = vi.fn(async () => {
      throw new MemoryCloudApiError('forbidden', 'HTTP 403', 403);
    });
    await store.getState().runQuery('anything');
    expect(store.getState().status).toBe('forbidden');
    expect(store.getState().query.status).toBe('idle');
    expect(store.getState().query.error).toBeNull();
  });

  it('a 429 is a rate-limited query state with a retry time, not an error or a load failure', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(2_000_000);
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    env.deps.postQuery = vi.fn(async () => {
      throw new MemoryCloudApiError('rate_limited', 'HTTP 429: memory cloud query budget exhausted; retry in a minute', 429);
    });
    await store.getState().runQuery('anything');
    const q = store.getState().query;
    expect(q.status).toBe('rate_limited');
    expect(q.retryAt).toBe(2_000_000 + QUERY_RETRY_DEFAULT_MS);
    expect(q.error).toMatch(/rate limited.*retry/i);
    expect(store.getState().status).toBe('ready');

    env.deps.postQuery = vi.fn(async () => {
      throw new MemoryCloudApiError('rate_limited', 'HTTP 429', 429, { retryAfterMs: 9000 });
    });
    await store.getState().runQuery('again');
    expect(store.getState().query.retryAt).toBe(2_000_000 + 9000);

    env.deps.postQuery = vi.fn(async () => response());
    await store.getState().runQuery('later');
    expect(store.getState().query.status).toBe('done');
    expect(store.getState().query.retryAt).toBeNull();
  });

  it('on 503 waits Retry-After, then reloads once by itself', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    let calls = 0;
    env.deps.fetchSnapshot = vi.fn(async () => {
      calls++;
      if (calls === 1) throw new MemoryCloudApiError('unavailable', 'HTTP 503', 503, { retryAfterMs: 4000 });
      return snapshot('s1');
    });
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().status).toBe('unavailable');
    expect(store.getState().retryAt).toBe(1_004_000);
    await vi.advanceTimersByTimeAsync(3999);
    expect(calls).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    await vi.runAllTimersAsync();
    expect(calls).toBe(2);
    expect(store.getState().status).toBe('ready');
    expect(store.getState().retryAt).toBeNull();
  });

  it('without Retry-After backs off by the default, clamped to sane bounds', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    env.deps.fetchSnapshot = vi.fn(async () => {
      throw new MemoryCloudApiError('unavailable', 'HTTP 503', 503);
    });
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().retryAt).toBe(5000);
    env.deps.fetchSnapshot = vi.fn(async () => {
      throw new MemoryCloudApiError('unavailable', 'HTTP 503', 503, { retryAfterMs: 86_400_000 });
    });
    await store.getState().loadSnapshot();
    expect(store.getState().retryAt).toBe(60_000);
  });

  it('without Retry-After doubles the delay on each consecutive 503 and shows the reason', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    env.deps.fetchSnapshot = vi.fn(async () => {
      throw new MemoryCloudApiError('unavailable', 'HTTP 503: memory cloud not configured', 503);
    });
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    expect(store.getState().retryAt).toBe(5000);
    expect(store.getState().error).toBe('memory cloud not configured');
    await vi.advanceTimersByTimeAsync(5000);
    expect(store.getState().retryAt).toBe(5000 + 10_000);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(store.getState().retryAt).toBe(15_000 + 20_000);
  });

  it('dispose cancels a pending 503 retry', async () => {
    vi.useFakeTimers();
    env.deps.fetchSnapshot = vi.fn(async () => {
      throw new MemoryCloudApiError('unavailable', 'HTTP 503', 503, { retryAfterMs: 2000 });
    });
    const store = createMemoryCloudStore(env.deps);
    await store.getState().loadSnapshot();
    store.getState().dispose();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(env.deps.fetchSnapshot).toHaveBeenCalledTimes(1);
    expect(store.getState().retryAt).toBeNull();
  });
});

describe('memoryCloudStore — beat clock and Spotify', () => {
  const ID = '4uLU6hMCjMI75M1A2tKUQC';
  const T0 = 1_760_000_000_000;
  let env: ReturnType<typeof makeDeps>;
  beforeEach(() => { env = makeDeps(); });

  it('starts off, with a serialisable wire state', () => {
    const store = createMemoryCloudStore(env.deps);
    const b = store.getState().beat;
    expect(b.source).toBe('off');
    expect(JSON.parse(JSON.stringify(b))).toEqual(b);
    expect(Object.keys(b).sort()).toEqual(['bpm', 'confidence', 'phaseAt', 'source']);
  });

  it('a tap from off switches to tap tempo and locks phase on every tap', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().tapBeat(T0);
    expect(store.getState().beat).toMatchObject({ source: 'tap', phaseAt: T0, bpm: 120 });
    store.getState().tapBeat(T0 + 400);
    expect(store.getState().beat.phaseAt).toBe(T0 + 400);
    store.getState().tapBeat(T0 + 800);
    expect(store.getState().beat).toMatchObject({ source: 'tap', bpm: 150, phaseAt: T0 + 800 });
    expect(store.getState().beat.confidence).toBeGreaterThan(0);
  });

  it('taps under Spotify keep the Spotify source', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().setSpotifyLink(`https://open.spotify.com/track/${ID}`);
    for (let i = 0; i < 4; i++) store.getState().tapBeat(T0 + i * 500);
    expect(store.getState().beat).toMatchObject({ source: 'spotify', bpm: 120, phaseAt: T0 + 1500 });
  });

  it('a valid link selects Spotify; an invalid one is refused and leaves the source alone', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().setSpotifyLink('https://open.spotify.com.evil.com/track/' + ID);
    expect(store.getState().spotify.link).toBeNull();
    expect(store.getState().spotify.error).toMatch(/open\.spotify\.com/);
    expect(store.getState().beat.source).toBe('off');
    store.getState().setSpotifyLink(`https://open.spotify.com/album/${ID}?si=x`);
    expect(store.getState().spotify).toEqual({ link: { kind: 'album', id: ID, url: `https://open.spotify.com/album/${ID}` }, error: null });
    expect(store.getState().beat.source).toBe('spotify');
    store.getState().clearSpotify();
    expect(store.getState().spotify.link).toBeNull();
    expect(store.getState().beat.source).toBe('off');
  });

  it('switching sources keeps the tapped tempo for tap and Spotify, unlocks for file, keeps bpm for off', () => {
    const store = createMemoryCloudStore(env.deps);
    for (let i = 0; i < 4; i++) store.getState().tapBeat(T0 + i * 600);
    expect(store.getState().beat.bpm).toBe(100);
    store.getState().setBeatSource('spotify');
    expect(store.getState().beat).toMatchObject({ source: 'spotify', bpm: 100, phaseAt: T0 + 1800 });
    store.getState().setBeatSource('file');
    expect(store.getState().beat).toMatchObject({ source: 'file', phaseAt: 0 });
    store.getState().setBeatSource('tap');
    expect(store.getState().beat).toMatchObject({ source: 'tap', bpm: 100, phaseAt: T0 + 1800 });
    store.getState().setBeatSource('off');
    expect(store.getState().beat).toMatchObject({ source: 'off', bpm: 100 });
  });

  it('nudges bpm and phase and keeps them for the tap clock', () => {
    const store = createMemoryCloudStore(env.deps);
    for (let i = 0; i < 4; i++) store.getState().tapBeat(T0 + i * 500);
    store.getState().nudgeBeat({ bpm: 0.5, phaseMs: -20 });
    expect(store.getState().beat).toMatchObject({ bpm: 120.5, phaseAt: T0 + 1500 - 20 });
    store.getState().setBeatSource('off');
    store.getState().setBeatSource('tap');
    expect(store.getState().beat).toMatchObject({ bpm: 120.5, phaseAt: T0 + 1480 });
  });

  it('setBeat publishes a file lock from the director', () => {
    const store = createMemoryCloudStore(env.deps);
    store.getState().setBeatSource('file');
    store.getState().setBeat({ bpm: 96, phaseAt: T0 + 120, confidence: 0.8, source: 'file' });
    expect(store.getState().beat).toEqual({ bpm: 96, phaseAt: T0 + 120, confidence: 0.8, source: 'file' });
  });
});
