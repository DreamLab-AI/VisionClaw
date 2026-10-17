/**
 * Wires the store to the server (or the demo feed): snapshot loading, the
 * positions pipeline with acks, the events socket, View/Focus requests, and
 * the command path.
 */
import { HELP_TEXT, parseLocalCommand, type LocalCommand } from './commands';
import { buildDemoWorld, DemoFeed, demoQuery } from './demo/demoGraph';
import { Api } from './net/api';
import { resolveEndpoints, backoffDelay, isDemo, newSessionKey, withQuery, type Endpoints } from './net/config';
import { EventsSocket } from './net/EventsSocket';
import { FramePipeline, type Applied } from './net/FramePipeline';
import { PositionsSocket } from './net/PositionsSocket';
import type { ClientEvent, ServerEvent, Tier } from './protocol';
import { createStore, initialState, installStore, type Store } from './store';

export interface LiveStats {
  framesApplied: number;
  framesReceived: number;
  lastSeq: number;
  unknown: number;
  lastFrameAt: number;
}

export class Session {
  readonly store: Store;
  readonly endpoints: Endpoints;
  readonly api: Api;
  readonly pipeline: FramePipeline;
  readonly demo: boolean;
  readonly live: LiveStats = { framesApplied: 0, framesReceived: 0, lastSeq: 0, unknown: 0, lastFrameAt: 0 };
  /** Scene radius estimate, refreshed by the scene from the live positions. */
  sceneRadius = 400;
  private positions: PositionsSocket | null = null;
  private events: EventsSocket | null = null;
  private feed: DemoFeed | null = null;
  private timers: ReturnType<typeof setInterval>[] = [];
  private unsub: (() => void) | null = null;
  private snapshotAttempts = 0;
  private lastResync = 0;
  private lastView = '';
  readonly name: string;
  /** Links this page's sockets server-side (`?session=`). */
  readonly sessionKey = newSessionKey();
  private readonly repo: string | null;

  constructor(loc: Location = window.location) {
    this.endpoints = resolveEndpoints(loc);
    this.demo = isDemo(loc.search);
    this.store = createStore(initialState({ demo: this.demo, server: this.endpoints.label }));
    installStore(this.store);
    this.api = new Api(this.endpoints.http);
    const params = new URLSearchParams(loc.search);
    this.name = params.get('name') ?? 'desktop';
    this.repo = params.get('repo');
    this.pipeline = new FramePipeline(
      (r) => this.onApplied(r),
      (reason, b) => this.store.dispatch({ type: 'error', message: `frame rejected (${reason}, 0x${b.toString(16)})`, nowMs: Date.now() }),
    );
  }

  start(): void {
    let lastModel = 0;
    let lastSel: number | null = null;
    let lastTier: Tier | null = null;
    this.unsub = this.store.subscribe(() => {
      const s = this.store.getState();
      if (s.modelVersion !== lastModel && s.model) {
        lastModel = s.modelVersion;
        this.pipeline.setModel(s.model, this.sceneRadius);
      }
      const selChanged = s.selection !== lastSel;
      if (selChanged) {
        lastSel = s.selection;
        this.send({ type: 'focus', node_id: s.selection ?? 0 });
      }
      const tier = s.tierMode === 'auto' ? s.currentTier : s.tierMode;
      if (tier !== lastTier || selChanged) {
        lastTier = tier;
        this.sendView();
      }
    });
    this.timers.push(setInterval(() => this.publishHud(), 500));
    if (this.demo) this.startDemo();
    else this.startLive();
  }

  stop(): void {
    this.unsub?.();
    this.timers.forEach((t) => clearInterval(t));
    this.positions?.stop();
    this.events?.stop();
    this.feed?.stop();
    this.timers = [];
    this.lastView = '';
    this.positions = null;
    this.events = null;
    this.feed = null;
  }

  /** Stop and release the decoder worker (page teardown). */
  dispose(): void {
    this.stop();
    this.pipeline.dispose();
  }

  // ------------------------------------------------------------ wiring

  /** Socket URL with the session, repo, name and (for positions) initial tier. */
  socketUrl(kind: 'events' | 'positions' | 'voice'): string {
    const s = this.store.getState();
    const tier = s.tierMode === 'auto' ? s.currentTier : s.tierMode;
    return withQuery(this.endpoints[kind], {
      session: this.sessionKey,
      repo: this.repo,
      name: kind === 'positions' ? null : this.name,
      tier: kind === 'positions' ? tier : null,
    });
  }

  private startDemo(): void {
    const world = buildDemoWorld(5000);
    this.sceneRadius = world.radius;
    this.store.dispatch({ type: 'connection', channel: 'events', patch: { status: 'open', url: 'demo' } });
    this.store.dispatch({ type: 'connection', channel: 'positions', patch: { status: 'open', url: 'demo' } });
    this.store.dispatch({ type: 'snapshot', snapshot: world.snapshot });
    this.feed = new DemoFeed(
      world,
      () => this.store.getState().model,
      (frame) => {
        this.live.framesReceived++;
        this.pipeline.push(frame);
      },
      (e) => this.onServerEvent(e),
    );
    this.feed.start();
  }

  private startLive(): void {
    const evUrl = this.socketUrl('events');
    this.events = new EventsSocket(
      evUrl,
      (status, attempts, detail) => this.store.dispatch({ type: 'connection', channel: 'events', patch: { status, attempts, detail, url: evUrl } }),
      () => this.onEventsOpen(),
    );
    this.events.onAny((e) => this.onServerEvent(e));
    this.store.dispatch({ type: 'connection', channel: 'positions', patch: { status: 'idle', url: this.endpoints.positions, detail: 'waiting for snapshot' } });
    this.loadSnapshot();
    this.events.start();
  }

  /** Every (re)connect of /ws/events re-identifies and restates this client's View and Focus. */
  private onEventsOpen(): ClientEvent[] {
    const s = this.store.getState();
    const view = this.currentView();
    this.lastView = `${view.tier}:${view.focus_files.join(',')}`;
    return [{ type: 'hello', name: this.name, kind: 'human' }, view, { type: 'focus', node_id: s.selection ?? 0 }];
  }

  /**
   * (Re)open /ws/positions. Called only once a snapshot has been adopted, so the
   * join keyframe always lands on a populated id→index map; a resync restarts it
   * to get a fresh keyframe.
   */
  private startPositions(): void {
    this.positions?.stop();
    const url = this.socketUrl('positions');
    this.positions = new PositionsSocket(url, this.pipeline, {
      onStatus: (status, attempts, detail) => this.store.dispatch({ type: 'connection', channel: 'positions', patch: { status, attempts, detail, url } }),
      onApplied: () => undefined,
    });
    this.positions.start();
  }

  private loadSnapshot(): void {
    this.api
      .snapshot('l2')
      .then((snap) => {
        this.snapshotAttempts = 0;
        // The store subscription adopts the model into the pipeline synchronously.
        this.store.dispatch({ type: 'snapshot', snapshot: snap });
        if (!this.demo) this.startPositions();
      })
      .catch((err: unknown) => {
        this.store.dispatch({ type: 'snapshot_error', message: String(err instanceof Error ? err.message : err) });
        setTimeout(() => this.loadSnapshot(), backoffDelay(this.snapshotAttempts++));
      });
  }

  private onServerEvent(e: ServerEvent): void {
    const prevEpoch = this.store.getState().hello?.epoch;
    this.store.dispatch({ type: 'server', event: e, nowMs: Date.now() });
    // A new interner epoch invalidates every id: refetch the snapshot.
    if (e.type === 'hello' && prevEpoch !== undefined && prevEpoch !== e.epoch && !this.demo) this.loadSnapshot();
  }

  private onApplied(r: Applied): void {
    this.live.framesApplied++;
    this.live.lastSeq = r.seq;
    this.live.unknown = r.unknown;
    this.live.lastFrameAt = performance.now();
    this.positions?.ack(r);
    // Records for ids we do not know mean our snapshot is behind (missed delta): resync, at most every 5 s.
    if (r.unknown > 0 && !this.demo && performance.now() - this.lastResync > 5000) {
      this.lastResync = performance.now();
      this.loadSnapshot();
    }
  }

  private publishHud(): void {
    const received = this.positions ? this.positions.framesReceived : this.live.framesReceived;
    this.store.dispatch({
      type: 'hud',
      patch: {
        framesApplied: this.live.framesApplied,
        lastSeq: this.live.lastSeq,
        unknownRecords: this.live.unknown,
        lag: Math.max(this.pipeline.inFlight, received - this.live.framesApplied, this.store.getState().hud.lag > 0 ? this.store.getState().hud.lag : 0),
      },
    });
  }

  // ------------------------------------------------------------ outbound

  send(e: ClientEvent): void {
    if (this.demo) return;
    this.events?.send(e);
  }

  /** The View this client wants: the streamed tier plus the focused file's symbols. */
  currentView(): Extract<ClientEvent, { type: 'view' }> {
    const s = this.store.getState();
    const tier: Tier = s.tierMode === 'auto' ? s.currentTier : s.tierMode;
    const focusFiles: number[] = [];
    if (s.model && s.selection !== null) {
      const i = s.model.index.get(s.selection);
      if (i !== undefined) {
        let j = i;
        while (j >= 0 && s.model.tier[j] > 1) j = s.model.parent[j];
        if (j >= 0 && s.model.tier[j] === 1) focusFiles.push(s.model.ids[j]);
      }
    }
    return { type: 'view', tier, focus_files: focusFiles };
  }

  private sendView(): void {
    const v = this.currentView();
    const key = `${v.tier}:${v.focus_files.join(',')}`;
    if (key === this.lastView) return;
    this.lastView = key;
    this.send(v);
  }

  /** Run a typed command: view commands apply locally; everything is forwarded. */
  command(text: string): void {
    const trimmed = text.trim();
    if (!trimmed) return;
    const local = parseLocalCommand(trimmed);
    if (local) {
      this.applyLocal(local);
      if (local.kind === 'pick' || local.kind === 'help' || local.kind === 'clear') return;
    }
    if (this.demo) {
      if (local) return;
      const m = this.store.getState().model;
      if (m) this.onServerEvent({ type: 'query', ...demoQuery(m, trimmed) });
      return;
    }
    if (this.events && this.store.getState().events.status === 'open') {
      this.send({ type: 'command', text: trimmed });
    } else if (!local) {
      this.api
        .query(trimmed)
        .then((q) => this.onServerEvent({ type: 'query', ...q }))
        .catch((err: unknown) => this.store.dispatch({ type: 'error', message: `query failed: ${String(err)}`, nowMs: Date.now() }));
    }
  }

  applyLocal(c: LocalCommand): void {
    const d = this.store.dispatch;
    const s = this.store.getState();
    switch (c.kind) {
      case 'overview':
        d({ type: 'select', id: null });
        d({ type: 'camera', kind: 'overview' });
        return;
      case 'zoom_in':
        d({ type: 'camera', kind: 'zoom_in' });
        return;
      case 'zoom_out':
        d({ type: 'camera', kind: 'zoom_out' });
        return;
      case 'back':
        d({ type: 'back' });
        return;
      case 'show_all':
        d({ type: 'filter', patch: { langs: [], community: null, hideTests: false } });
        return;
      case 'hide_tests':
        d({ type: 'filter', patch: { hideTests: true } });
        return;
      case 'show_only':
        d({ type: 'filter', patch: c.lang ? { langs: [c.lang], community: null } : { community: c.community, langs: [] } });
        return;
      case 'isolate': {
        let comm = c.community;
        if (comm === null && s.model && s.selection !== null) {
          const i = s.model.index.get(s.selection);
          if (i !== undefined) comm = s.model.community[i];
        }
        d({ type: 'filter', patch: { community: comm } });
        return;
      }
      case 'rewind': {
        const base = s.timeline.t ?? Date.now();
        d({ type: 'timeline', patch: { mode: 'paused', t: base - c.seconds * 1000 } });
        return;
      }
      case 'replay': {
        const first = s.presence[0]?.ts_ms;
        d({ type: 'timeline', patch: { mode: 'replay', t: s.timeline.t ?? first ?? Date.now() - 60_000 } });
        return;
      }
      case 'pause':
        d({ type: 'timeline', patch: { mode: 'paused', t: s.timeline.t ?? Date.now() } });
        return;
      case 'live':
        d({ type: 'timeline', patch: { mode: 'live' } });
        return;
      case 'follow': {
        const want = c.actor.toLowerCase();
        const a = Object.values(s.actors).find((x) => x.label.toLowerCase().includes(want));
        if (a) d({ type: 'follow', actor: a.id });
        else d({ type: 'error', message: `no actor matching “${c.actor}”`, nowMs: Date.now() });
        return;
      }
      case 'stop_following':
        d({ type: 'follow', actor: null });
        return;
      case 'pick': {
        const cand = s.query?.candidates[c.n - 1];
        if (cand) {
          d({ type: 'select', id: cand[1], fly: true });
          d({ type: 'clear_query' });
        } else d({ type: 'error', message: `no candidate number ${c.n}`, nowMs: Date.now() });
        return;
      }
      case 'clear':
        d({ type: 'select', id: null });
        d({ type: 'clear_query' });
        return;
      case 'help':
        this.onServerEvent({ type: 'query', command: 'help', node_ids: [], edge_pairs: [], path: [], candidates: [], answer: HELP_TEXT });
        return;
    }
  }
}
