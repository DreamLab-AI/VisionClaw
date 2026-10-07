/**
 * Desktop → headset relay of the memory explorer's beat clock and route
 * (ADR-2134). The server forwards these two `/wss` text frames only to the
 * same authenticated user's other sessions, i.e. their Godot XR client:
 *
 *   { type: 'beatClock', bpm, phaseAt, confidence, source, sentAt }
 *   { type: 'memoryRoute', snapshotId, nodeIds, positions }
 *
 * - beatClock goes out when the clock changes and every 2 s while it is on
 *   (the headset drops a clock after ~6.5 s of silence). Nothing is sent while
 *   the source is off, apart from one `off` frame at the moment it turns off so
 *   the headset stops at once rather than timing out.
 * - memoryRoute goes out when a query completes (the route root → answer, as
 *   memory-entry ids plus their cloud positions) and as an empty route when the
 *   query is cleared; a live route repeats every 10 s for a headset that joins
 *   late.
 * - Both are throttled to ≤ 4 Hz with a trailing send, matching the server.
 * - `sentAt` lets the server rebase `phaseAt` onto its own clock, so a skewed
 *   desktop clock does not shift the headset's beat.
 *
 * Frames are flat (not the `{type, data}` envelope of `sendMessage`) and are
 * dropped, not queued, while the socket is down: a queued beat would be stale
 * by the time it was flushed, and the heartbeat re-sends the current state.
 */

import type { StoreApi } from 'zustand';
import type { BeatClockState } from './beatClock';
import type { MemoryCloudState } from './memoryCloudStore';

export const RELAY_MIN_INTERVAL_MS = 250;
export const BEAT_HEARTBEAT_MS = 2000;
export const ROUTE_REPEAT_MS = 10_000;
/** server cap (session_relay.rs MAX_ROUTE_NODES) */
export const MAX_ROUTE_NODES = 512;

export interface BeatClockFrame {
  type: 'beatClock';
  bpm: number;
  phaseAt: number;
  confidence: number;
  source: BeatClockState['source'];
  sentAt: number;
}

export interface MemoryRouteFrame {
  type: 'memoryRoute';
  snapshotId: string;
  nodeIds: string[];
  positions?: number[];
}

export type RelayFrame = BeatClockFrame | MemoryRouteFrame;

export interface XrRelayDeps {
  /** send one flat JSON frame; return false when the socket is not open */
  send: (frame: RelayFrame) => boolean;
  now?: () => number;
  setTimeout?: (fn: () => void, ms: number) => unknown;
  clearTimeout?: (h: unknown) => void;
  setInterval?: (fn: () => void, ms: number) => unknown;
  clearInterval?: (h: unknown) => void;
}

type Slice = Pick<MemoryCloudState, 'beat' | 'query' | 'snapshot'>;

/** Route frame for the current query, or an empty (clearing) route. */
export function routeFrame(s: Slice): MemoryRouteFrame | null {
  const snap = s.snapshot;
  if (!snap) return null;
  const path = s.query.status === 'done' ? s.query.run?.tree.path ?? [] : [];
  const rows = path.length > MAX_ROUTE_NODES ? path.slice(-MAX_ROUTE_NODES) : path;
  const nodeIds: string[] = [];
  const positions: number[] = [];
  for (const i of rows) {
    const meta = snap.metadata[i];
    if (!meta) continue;
    nodeIds.push(meta.id);
    const p = snap.positions;
    if (i * 3 + 2 < p.length) positions.push(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
  }
  const frame: MemoryRouteFrame = { type: 'memoryRoute', snapshotId: snap.snapshotId, nodeIds };
  if (positions.length === nodeIds.length * 3 && nodeIds.length > 0) frame.positions = positions;
  return frame;
}

function beatKey(b: BeatClockState): string {
  return `${b.source}|${b.bpm}|${b.phaseAt}|${b.confidence}`;
}

/** One throttled channel: at most one send per interval, newest frame wins. */
class Throttled<F extends RelayFrame> {
  private last = -Infinity;
  private pending: F | null = null;
  private timer: unknown = null;

  constructor(
    private readonly deps: Required<Pick<XrRelayDeps, 'send' | 'now' | 'setTimeout' | 'clearTimeout'>>,
  ) {}

  offer(frame: F): void {
    const now = this.deps.now();
    const since = now - this.last;
    if (since >= RELAY_MIN_INTERVAL_MS && this.timer === null) {
      this.last = now;
      this.deps.send(frame);
      return;
    }
    this.pending = frame;
    if (this.timer !== null) return;
    this.timer = this.deps.setTimeout(() => {
      this.timer = null;
      const f = this.pending;
      this.pending = null;
      if (f) {
        this.last = this.deps.now();
        this.deps.send(f);
      }
    }, Math.max(0, RELAY_MIN_INTERVAL_MS - since));
  }

  dispose(): void {
    if (this.timer !== null) this.deps.clearTimeout(this.timer);
    this.timer = null;
    this.pending = null;
  }
}

export interface XrRelay {
  dispose(): void;
}

/** Subscribe to a memory cloud store and relay its beat clock and route. */
export function createXrRelay(store: StoreApi<MemoryCloudState>, deps: XrRelayDeps): XrRelay {
  const d = {
    send: deps.send,
    now: deps.now ?? (() => Date.now()),
    setTimeout: deps.setTimeout ?? ((fn: () => void, ms: number) => setTimeout(fn, ms)),
    clearTimeout: deps.clearTimeout ?? ((h: unknown) => clearTimeout(h as ReturnType<typeof setTimeout>)),
    setInterval: deps.setInterval ?? ((fn: () => void, ms: number) => setInterval(fn, ms)),
    clearInterval: deps.clearInterval ?? ((h: unknown) => clearInterval(h as ReturnType<typeof setInterval>)),
  };
  const beatCh = new Throttled<BeatClockFrame>(d);
  const routeCh = new Throttled<MemoryRouteFrame>(d);

  const beatFrame = (b: BeatClockState): BeatClockFrame => ({
    type: 'beatClock',
    bpm: b.bpm,
    phaseAt: b.phaseAt,
    confidence: b.confidence,
    source: b.source,
    sentAt: d.now(),
  });

  let lastBeatKey = '';
  /** whether the headset currently believes a clock is on */
  let beatAnnounced = false;
  let lastRouteKey = '';
  let routeLive = false;
  let lastRouteSent = -Infinity;

  const onBeat = (b: BeatClockState) => {
    const key = beatKey(b);
    if (key === lastBeatKey) return;
    lastBeatKey = key;
    if (b.source === 'off') {
      if (beatAnnounced) beatCh.offer(beatFrame(b));
      beatAnnounced = false;
      return;
    }
    beatAnnounced = true;
    beatCh.offer(beatFrame(b));
  };

  const onRoute = (s: Slice) => {
    const f = routeFrame(s);
    if (!f) return;
    const key = `${f.snapshotId}|${f.nodeIds.join(',')}`;
    if (key === lastRouteKey) return;
    const clearing = f.nodeIds.length === 0;
    if (clearing && !routeLive) {
      lastRouteKey = key;
      return;
    }
    lastRouteKey = key;
    routeLive = !clearing;
    lastRouteSent = d.now();
    routeCh.offer(f);
  };

  const unsub = store.subscribe((s, prev) => {
    if (s.beat !== prev.beat) onBeat(s.beat);
    if (s.query !== prev.query || s.snapshot !== prev.snapshot) onRoute(s);
  });

  const heartbeat = d.setInterval(() => {
    const s = store.getState();
    if (s.beat.source !== 'off') {
      beatAnnounced = true;
      beatCh.offer(beatFrame(s.beat));
    }
    if (routeLive && d.now() - lastRouteSent >= ROUTE_REPEAT_MS) {
      const f = routeFrame(s);
      if (f && f.nodeIds.length > 0) {
        lastRouteSent = d.now();
        routeCh.offer(f);
      }
    }
  }, BEAT_HEARTBEAT_MS);

  // Adopt whatever is already live when the relay starts.
  onBeat(store.getState().beat);
  onRoute(store.getState());

  return {
    dispose() {
      unsub();
      d.clearInterval(heartbeat);
      beatCh.dispose();
      routeCh.dispose();
    },
  };
}
