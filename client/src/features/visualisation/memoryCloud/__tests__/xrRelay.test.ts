import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createMemoryCloudStore, type MemoryCloudDeps } from '../memoryCloudStore';
import {
  createXrRelay,
  routeFrame,
  BEAT_HEARTBEAT_MS,
  MAX_ROUTE_PATH,
  RELAY_MIN_INTERVAL_MS,
  ROUTE_REPEAT_MS,
  type RelayFrame,
} from '../xrRelay';
import type { MemoryCloudSnapshot } from '../types';
import type { QueryRun, SearchTree } from '../../memoryTrajectory/types';

const T0 = 1_760_000_000_000;

const never = () => new Promise<never>(() => {});
const deps: MemoryCloudDeps = {
  fetchSnapshot: never,
  fetchVectors: never,
  postQuery: never,
  fetchHealth: never,
  loadTrajectory: never,
};

function snapshot(count = 4, id = 's1'): MemoryCloudSnapshot {
  return {
    version: 1, snapshotId: id, generatedAt: 1, dim: 4, count,
    positions: Array.from({ length: count * 3 }, (_, i) => i / 10),
    metadata: Array.from({ length: count }, (_, i) => ({ id: `mem-${i}`, key: `k${i}`, namespace: 'ns', sourceType: 'memory', updatedAt: i })),
    namespaces: ['ns'], sourceTypes: ['memory'], strata: [], excludedNamespaces: [], vectorsUrl: '',
  };
}

const runWithPath = (path: number[]) =>
  ({ result: { top: [], beam: [], hops: [], distanceEvals: 0, trace: [] },
     tree: { root: path[0] ?? 0, nodes: new Map(), list: [], path, maxDepth: 1, leaves: 1, keptTotal: 1 } as unknown as SearchTree,
     exactTop: [], recall: 1, breadth: 1, hintsUsed: [] }) as QueryRun;

describe('xrRelay', () => {
  let sent: RelayFrame[];
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    sent = [];
  });
  afterEach(() => vi.useRealTimers());

  const make = () => {
    const store = createMemoryCloudStore(deps);
    const relay = createXrRelay(store, { send: (f) => { sent.push(f); return true; } });
    return { store, relay };
  };
  const beats = () => sent.filter((f) => f.type === 'beatClock');
  const routes = () => sent.filter((f) => f.type === 'memoryRoute');

  it('sends nothing while the beat source is off', () => {
    const { relay } = make();
    vi.advanceTimersByTime(10 * BEAT_HEARTBEAT_MS);
    expect(beats()).toHaveLength(0);
    relay.dispose();
  });

  it('relays a tap clock with sentAt, heartbeats every 2 s, and sends one off frame', () => {
    const { store, relay } = make();
    for (let i = 0; i < 4; i++) {
      store.getState().tapBeat(T0 + i * 500);
      vi.advanceTimersByTime(500);
    }
    const first = beats();
    expect(first.length).toBeGreaterThan(0);
    const last = first[first.length - 1] as Extract<RelayFrame, { type: 'beatClock' }>;
    expect(last).toMatchObject({ type: 'beatClock', bpm: 120, source: 'tap', phaseAt: T0 + 1500 });
    expect(typeof last.sentAt).toBe('number');
    const n = beats().length;
    vi.advanceTimersByTime(3 * BEAT_HEARTBEAT_MS);
    expect(beats().length - n).toBe(3); // heartbeat
    store.getState().setBeatSource('off');
    vi.advanceTimersByTime(RELAY_MIN_INTERVAL_MS);
    const off = beats()[beats().length - 1];
    expect(off).toMatchObject({ source: 'off' });
    const m = beats().length;
    vi.advanceTimersByTime(10 * BEAT_HEARTBEAT_MS);
    expect(beats().length).toBe(m); // silent while off
    relay.dispose();
  });

  it('throttles to at most 4 Hz and delivers the final state', () => {
    const { store, relay } = make();
    store.getState().setBeatSource('tap');
    store.getState().tapBeat(T0);
    // 40 nudges in one second: far more than 4 Hz
    for (let i = 0; i < 40; i++) {
      store.getState().nudgeBeat({ bpm: 0.5 });
      vi.advanceTimersByTime(25);
    }
    vi.advanceTimersByTime(RELAY_MIN_INTERVAL_MS);
    const b = beats();
    expect(b.length).toBeLessThanOrEqual(6);
    expect((b[b.length - 1] as { bpm: number }).bpm).toBe(store.getState().beat.bpm);
    relay.dispose();
  });

  it('relays the route as snapshot rows with sidecar and query, repeats it, then clears it', () => {
    const { store, relay } = make();
    store.setState({ snapshot: snapshot() });
    expect(routes()).toHaveLength(0); // no route yet, nothing to clear
    const response = {
      snapshotId: 's1', embedModel: 'bge', query: { text: 'q', vector: [] },
      sidecar: { tookMs: 1, results: [
        { id: 'a', key: 'a', namespace: 'ns', sourceType: 'm', score: 1, snippet: '', sampleIndex: 3 },
        { id: 'b', key: 'b', namespace: 'ns', sourceType: 'm', score: 1, snippet: '', sampleIndex: null },
      ] },
    };
    store.setState((s) => ({ query: { ...s.query, text: 'how does auth work', status: 'done', run: runWithPath([0, 2, 3]), response, seq: 1 } }));
    vi.advanceTimersByTime(RELAY_MIN_INTERVAL_MS);
    expect(routes()[0]).toEqual({
      type: 'memoryRoute', snapshotId: 's1', seq: 1, sentAt: T0,
      path: [0, 2, 3], sidecar: [3], query: 'how does auth work',
    });
    vi.advanceTimersByTime(ROUTE_REPEAT_MS + BEAT_HEARTBEAT_MS);
    expect(routes().length).toBe(2); // late-joiner repeat
    const rep = routes()[1] as Extract<RelayFrame, { type: 'memoryRoute' }>;
    expect(rep.seq).toBe(2);
    expect(rep.sentAt).toBeGreaterThan(T0); // fresh stamp: the headset accepts it as newer
    store.getState().clearQuery();
    vi.advanceTimersByTime(RELAY_MIN_INTERVAL_MS);
    expect(routes()[routes().length - 1]).toMatchObject({ type: 'memoryRoute', snapshotId: 's1', path: [], sidecar: [], query: '', seq: 3 });
    const n = routes().length;
    vi.advanceTimersByTime(3 * ROUTE_REPEAT_MS);
    expect(routes().length).toBe(n); // no repeats once cleared
    relay.dispose();
  });

  it('keeps the answer end of an over-long route and drops out-of-range rows', () => {
    const snap = snapshot(200);
    const path = Array.from({ length: 100 }, (_, i) => i);
    const beat = { bpm: 120, phaseAt: 0, confidence: 0, source: 'off' as const };
    const f = routeFrame({ snapshot: snap, beat, query: { status: 'done', text: 'é'.repeat(300), run: runWithPath(path) } as never })!;
    expect(f.path).toHaveLength(MAX_ROUTE_PATH);
    expect(f.path[f.path.length - 1]).toBe(99);
    expect(Array.from(f.query)).toHaveLength(120);
    const g = routeFrame({ snapshot: snapshot(4), beat, query: { status: 'done', text: '', run: runWithPath([0, 9, 3]) } as never })!;
    expect(g.path).toEqual([0, 3]);
  });

  it('stops everything on dispose', () => {
    const { store, relay } = make();
    store.getState().setBeatSource('tap');
    store.getState().tapBeat(T0);
    relay.dispose();
    const n = sent.length;
    store.getState().tapBeat(T0 + 500);
    vi.advanceTimersByTime(10 * BEAT_HEARTBEAT_MS);
    expect(sent.length).toBe(n);
  });
});
