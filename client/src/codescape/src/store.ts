/**
 * Application state: one immutable object, a pure reducer, and a tiny
 * external store read through `useSyncExternalStore`. Per-frame data
 * (positions, heat) never goes through here; it lives in the shared buffers.
 */
import { useSyncExternalStore } from 'react';
import { applyDelta, buildModel, type GraphModel } from './graph';
import {
  captionOf,
  deltaOf,
  floorOf,
  presenceOf,
  queryOf,
  type CaptionEvent,
  type FloorState,
  type HelloEvent,
  type Lang,
  type PresenceEvent,
  type QueryResult,
  type ServerEvent,
  type Tier,
} from './protocol';

export type ConnStatus = 'idle' | 'connecting' | 'open' | 'closed' | 'error';

export interface Connection {
  status: ConnStatus;
  url: string;
  detail?: string;
  attempts: number;
}

export interface Actor {
  id: number;
  label: string;
  kind: string;
  /** Wire id of the node the actor last touched or focused. */
  focus: number | null;
  lastSeenMs: number;
}

export interface Filter {
  /** Visible languages; empty = all. */
  langs: Lang[];
  /** Isolated community, or null for all. */
  community: number | null;
  hideTests: boolean;
}

export type TimelineMode = 'live' | 'paused' | 'replay';

export interface Timeline {
  mode: TimelineMode;
  /** Playhead (unix ms) when not live. */
  t: number | null;
  /** Replay speed multiplier. */
  speed: number;
}

export interface Hud {
  fps: number;
  framesApplied: number;
  lastSeq: number;
  /** Frames received but not yet applied, plus the server's `lag` report. */
  lag: number;
  unknownRecords: number;
  settled: boolean;
  progress: { phase: string; done: number; total: number | null } | null;
}

export interface Highlight {
  command: string;
  ids: number[];
  path: number[];
  edgePairs: [number, number][];
}

export interface CsState {
  demo: boolean;
  server: string;
  events: Connection;
  positions: Connection;
  hello: HelloEvent | null;
  model: GraphModel | null;
  modelVersion: number;
  snapshotError: string | null;
  /** Selected wire id (focus), null for none. */
  selection: number | null;
  hover: number | null;
  history: number[];
  /** k for the focus neighbourhood. */
  focusHops: number;
  highlight: Highlight | null;
  filter: Filter;
  tierMode: 'auto' | Tier;
  currentTier: Tier;
  actors: Record<number, Actor>;
  presence: PresenceEvent[];
  captions: Record<number, CaptionEvent>;
  floor: FloorState;
  timeline: Timeline;
  /** Node wire id → unix ms it first appeared in a delta this session (timeline topology). */
  addedAt: Record<number, number>;
  query: QueryResult | null;
  follow: number | null;
  hud: Hud;
  errors: { ts: number; message: string }[];
  /** Incremented to ask the camera to fly to the selection / overview. */
  cameraCue: { kind: 'focus' | 'overview' | 'zoom_in' | 'zoom_out'; n: number };
}

export const PRESENCE_CAP = 4000;
export const ERROR_CAP = 20;

export function initialState(opts: { demo: boolean; server: string }): CsState {
  const conn = (url: string): Connection => ({ status: 'idle', url, attempts: 0 });
  return {
    demo: opts.demo,
    server: opts.server,
    events: conn(''),
    positions: conn(''),
    hello: null,
    model: null,
    modelVersion: 0,
    snapshotError: null,
    selection: null,
    hover: null,
    history: [],
    focusHops: 2,
    highlight: null,
    filter: { langs: [], community: null, hideTests: false },
    tierMode: 'auto',
    currentTier: 'l0',
    actors: {},
    presence: [],
    captions: {},
    floor: { state: 'free' },
    timeline: { mode: 'live', t: null, speed: 4 },
    addedAt: {},
    query: null,
    follow: null,
    hud: { fps: 0, framesApplied: 0, lastSeq: 0, lag: 0, unknownRecords: 0, settled: false, progress: null },
    errors: [],
    cameraCue: { kind: 'overview', n: 0 },
  };
}

export type Action =
  | { type: 'connection'; channel: 'events' | 'positions'; patch: Partial<Connection> }
  | { type: 'snapshot'; snapshot: GraphModel['snapshot'] }
  | { type: 'snapshot_error'; message: string }
  | { type: 'server'; event: ServerEvent; nowMs: number }
  | { type: 'select'; id: number | null; fly?: boolean }
  | { type: 'hover'; id: number | null }
  | { type: 'back' }
  | { type: 'filter'; patch: Partial<Filter> }
  | { type: 'toggle_hide_tests' }
  | { type: 'tier_mode'; mode: 'auto' | Tier }
  | { type: 'current_tier'; tier: Tier }
  | { type: 'timeline'; patch: Partial<Timeline> }
  | { type: 'hud'; patch: Partial<Hud> }
  | { type: 'follow'; actor: number | null }
  | { type: 'clear_query' }
  | { type: 'camera'; kind: CsState['cameraCue']['kind'] }
  | { type: 'error'; message: string; nowMs: number };

function pushError(s: CsState, message: string, ts: number): CsState['errors'] {
  return [...s.errors, { ts, message }].slice(-ERROR_CAP);
}

function recordPresence(s: CsState, p: PresenceEvent): CsState {
  const prev = s.actors[p.actor];
  const touched = p.node_ids.length ? p.node_ids[p.node_ids.length - 1] : (prev?.focus ?? null);
  const actor: Actor = {
    id: p.actor,
    label: p.actor_label,
    kind: p.actor_kind,
    focus: touched,
    lastSeenMs: Math.max(prev?.lastSeenMs ?? 0, p.ts_ms),
  };
  const presence = s.presence.length >= PRESENCE_CAP ? [...s.presence.slice(s.presence.length - PRESENCE_CAP + 1), p] : [...s.presence, p];
  const next: CsState = { ...s, actors: { ...s.actors, [p.actor]: actor }, presence };
  if (p.kind === 'say' && p.text) {
    next.captions = { ...s.captions, [p.actor]: { actor: p.actor, text: p.text, partial: false, ts_ms: p.ts_ms } };
  }
  // Following an actor moves our selection with them.
  if (s.follow === p.actor && touched !== null && touched !== s.selection) {
    next.selection = touched;
    next.cameraCue = { kind: 'focus', n: s.cameraCue.n + 1 };
  }
  return next;
}

export function reduce(s: CsState, a: Action): CsState {
  switch (a.type) {
    case 'connection': {
      const cur = s[a.channel];
      return { ...s, [a.channel]: { ...cur, ...a.patch } };
    }
    case 'snapshot': {
      const model = buildModel(a.snapshot);
      const keep = s.selection !== null && model.index.has(s.selection) ? s.selection : null;
      return { ...s, model, modelVersion: s.modelVersion + 1, snapshotError: null, selection: keep };
    }
    case 'snapshot_error':
      return { ...s, snapshotError: a.message };
    case 'server': {
      const e = a.event;
      switch (e.type) {
        case 'hello':
          return { ...s, hello: e };
        case 'delta': {
          if (!s.model) return s;
          const d = deltaOf(e);
          const snap = applyDelta(s.model.snapshot, d);
          const model = buildModel(snap);
          const addedAt = { ...s.addedAt };
          for (const nd of d.added_nodes) if (addedAt[nd.id] === undefined) addedAt[nd.id] = d.generated_at_ms || a.nowMs;
          const selection = s.selection !== null && model.index.has(s.selection) ? s.selection : null;
          const hello = s.hello ? { ...s.hello, revision: d.to_revision, nodes: snap.nodes.length, edges: snap.edges.length } : s.hello;
          return { ...s, model, modelVersion: s.modelVersion + 1, addedAt, selection, hello, hud: { ...s.hud, settled: false } };
        }
        case 'settled':
          return { ...s, hud: { ...s.hud, settled: true } };
        case 'presence':
          return recordPresence(s, presenceOf(e));
        case 'caption': {
          const c = captionOf(e);
          return { ...s, captions: { ...s.captions, [c.actor]: c } };
        }
        case 'floor':
          return { ...s, floor: floorOf(e) };
        case 'query': {
          const q = queryOf(e);
          const highlight: Highlight | null =
            q.node_ids.length || q.path.length ? { command: q.command, ids: q.node_ids, path: q.path, edgePairs: q.edge_pairs } : s.highlight;
          let next: CsState = { ...s, query: q, highlight };
          const first = q.path[0] ?? q.node_ids[0];
          if (first !== undefined && q.candidates.length === 0 && s.model?.index.has(first)) {
            next = reduce(next, { type: 'select', id: first, fly: true });
          }
          return next;
        }
        case 'lag':
          return { ...s, hud: { ...s.hud, lag: e.behind } };
        case 'progress':
          return { ...s, hud: { ...s.hud, progress: { phase: e.phase, done: e.done, total: e.total } } };
        case 'error':
          return { ...s, errors: pushError(s, e.message, a.nowMs) };
      }
      return s;
    }
    case 'select': {
      if (a.id === s.selection) {
        return a.fly ? { ...s, cameraCue: { kind: 'focus', n: s.cameraCue.n + 1 } } : s;
      }
      const history = s.selection !== null ? [...s.history, s.selection].slice(-50) : s.history;
      return {
        ...s,
        selection: a.id,
        history,
        highlight: a.id === null ? null : s.highlight,
        cameraCue: a.fly && a.id !== null ? { kind: 'focus', n: s.cameraCue.n + 1 } : s.cameraCue,
      };
    }
    case 'hover':
      return a.id === s.hover ? s : { ...s, hover: a.id };
    case 'back': {
      if (!s.history.length) return { ...s, selection: null, highlight: null };
      const history = s.history.slice(0, -1);
      return { ...s, selection: s.history[s.history.length - 1], history, cameraCue: { kind: 'focus', n: s.cameraCue.n + 1 } };
    }
    case 'toggle_hide_tests':
      return { ...s, filter: { ...s.filter, hideTests: !s.filter.hideTests } };

    case 'filter':
      return { ...s, filter: { ...s.filter, ...a.patch } };
    case 'tier_mode':
      return { ...s, tierMode: a.mode };
    case 'current_tier':
      return a.tier === s.currentTier ? s : { ...s, currentTier: a.tier };
    case 'timeline': {
      const tl = { ...s.timeline, ...a.patch };
      if (tl.mode === 'live') tl.t = null;
      return { ...s, timeline: tl };
    }
    case 'hud':
      return { ...s, hud: { ...s.hud, ...a.patch } };
    case 'follow': {
      const next = { ...s, follow: a.actor };
      const f = a.actor !== null ? s.actors[a.actor]?.focus : null;
      if (f !== null && f !== undefined) return reduce(next, { type: 'select', id: f, fly: true });
      return next;
    }
    case 'clear_query':
      return { ...s, query: null };
    case 'camera':
      return { ...s, cameraCue: { kind: a.kind, n: s.cameraCue.n + 1 } };
    case 'error':
      return { ...s, errors: pushError(s, a.message, a.nowMs) };
  }
}

export interface Store {
  getState(): CsState;
  dispatch(a: Action): void;
  subscribe(fn: () => void): () => void;
}

export function createStore(init: CsState): Store {
  let state = init;
  const subs = new Set<() => void>();
  return {
    getState: () => state,
    dispatch(a) {
      const next = reduce(state, a);
      if (next !== state) {
        state = next;
        subs.forEach((f) => f());
      }
    },
    subscribe(fn) {
      subs.add(fn);
      return () => subs.delete(fn);
    },
  };
}

let current: Store | null = null;

export function installStore(s: Store): void {
  current = s;
}

export function getStore(): Store {
  if (!current) throw new Error('Codescape store not installed');
  return current;
}

/** Select a slice of state; re-renders only when the selected value changes (Object.is). */
export function useCs<T>(selector: (s: CsState) => T): T {
  const store = getStore();
  return useSyncExternalStore(
    store.subscribe,
    () => selector(store.getState()),
    () => selector(store.getState()),
  );
}

export const dispatch = (a: Action): void => getStore().dispatch(a);
