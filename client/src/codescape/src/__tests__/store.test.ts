import { describe, expect, it } from 'vitest';
import type { PresenceEvent, ServerEvent } from '../protocol';
import { createStore, initialState, PRESENCE_CAP, reduce, type CsState } from '../store';
import { node, smallSnapshot } from './fixtures';

const base = (): CsState => reduce(initialState({ demo: false, server: 'x' }), { type: 'snapshot', snapshot: smallSnapshot() });
const srv = (s: CsState, event: ServerEvent) => reduce(s, { type: 'server', event, nowMs: 1000 });
const presence = (actor: number, ids: number[], ts: number, kind: PresenceEvent['kind'] = 'query', text: string | null = null): ServerEvent => ({
  type: 'presence',
  actor,
  actor_label: `a${actor}`,
  actor_kind: 'agent',
  kind,
  node_ids: ids,
  paths: [],
  text,
  ts_ms: ts,
});

describe('store reducers', () => {
  it('loads a snapshot into a model', () => {
    const s = base();
    expect(s.model?.n).toBe(6);
    expect(s.modelVersion).toBe(1);
  });

  it('select pushes history; back pops it; fly bumps the camera cue', () => {
    let s = base();
    s = reduce(s, { type: 'select', id: 100 });
    s = reduce(s, { type: 'select', id: 101, fly: true });
    expect(s.selection).toBe(101);
    expect(s.history).toEqual([100]);
    expect(s.cameraCue.kind).toBe('focus');
    const n = s.cameraCue.n;
    s = reduce(s, { type: 'back' });
    expect(s.selection).toBe(100);
    expect(s.cameraCue.n).toBe(n + 1);
    s = reduce(s, { type: 'back' });
    expect(s.selection).toBeNull();
  });

  it('presence updates actors, keeps a bounded log, and turns say into a caption', () => {
    let s = base();
    s = srv(s, presence(7, [10, 100], 5));
    expect(s.actors[7]).toMatchObject({ label: 'a7', focus: 100, lastSeenMs: 5 });
    s = srv(s, presence(7, [], 6, 'say', 'hello there'));
    expect(s.actors[7].focus).toBe(100);
    expect(s.captions[7]).toMatchObject({ text: 'hello there', partial: false });
    for (let k = 0; k < PRESENCE_CAP + 10; k++) s = srv(s, presence(8, [101], 10 + k));
    expect(s.presence.length).toBe(PRESENCE_CAP);
    expect(s.presence[s.presence.length - 1].ts_ms).toBe(10 + PRESENCE_CAP + 9);
  });

  it('following an actor moves the selection with them', () => {
    let s = base();
    s = srv(s, presence(7, [100], 5));
    s = reduce(s, { type: 'follow', actor: 7 });
    expect(s.selection).toBe(100);
    s = srv(s, presence(7, [110], 6));
    expect(s.selection).toBe(110);
    s = reduce(s, { type: 'follow', actor: null });
    s = srv(s, presence(7, [101], 7));
    expect(s.selection).toBe(110);
  });

  it('a query with candidates waits for disambiguation; one without selects and highlights', () => {
    let s = base();
    s = srv(s, { type: 'query', command: 'focus n', node_ids: [], edge_pairs: [], path: [], candidates: [['n100', 100], ['n101', 101]], answer: '2 matches' });
    expect(s.selection).toBeNull();
    expect(s.query?.candidates.length).toBe(2);
    s = srv(s, { type: 'query', command: 'callers n101', node_ids: [101, 100], edge_pairs: [[100, 101]], path: [], candidates: [], answer: '' });
    expect(s.selection).toBe(101);
    expect(s.highlight?.ids).toEqual([101, 100]);
  });

  it('applies deltas, records when nodes appeared, and drops a removed selection', () => {
    let s = reduce(base(), { type: 'select', id: 110 });
    s = srv(s, { type: 'hello', repo: 'r', epoch: 1, revision: 1, nodes: 6, edges: 8, engine: 'cpu', version: '0' });
    s = srv(s, { type: 'delta', from_revision: 1, to_revision: 2, generated_at_ms: 500, changed_paths: [], added_nodes: [node(102, 'l2', 10)], changed_nodes: [], removed_nodes: [110], added_edges: [], removed_edges: [] });
    expect(s.model?.n).toBe(6);
    expect(s.model?.index.has(102)).toBe(true);
    expect(s.addedAt[102]).toBe(500);
    expect(s.selection).toBeNull();
    expect(s.hello?.revision).toBe(2);
    expect(s.hud.settled).toBe(false);
    s = srv(s, { type: 'settled', revision: 2 });
    expect(s.hud.settled).toBe(true);
  });

  it('floor, lag, caption, error events', () => {
    let s = base();
    s = srv(s, { type: 'floor', state: 'held', holder: 3, queue: [4] });
    expect(s.floor).toEqual({ state: 'held', holder: 3, queue: [4] });
    s = srv(s, { type: 'lag', behind: 4 });
    expect(s.hud.lag).toBe(4);
    s = srv(s, { type: 'caption', actor: 3, text: 'hi', partial: true, ts_ms: 1 });
    expect(s.captions[3]).toEqual({ actor: 3, text: 'hi', partial: true, ts_ms: 1 });
    s = srv(s, { type: 'error', message: 'bad' });
    expect(s.errors.at(-1)?.message).toBe('bad');
  });

  it('timeline going live clears the playhead; filters merge', () => {
    let s = base();
    s = reduce(s, { type: 'timeline', patch: { mode: 'paused', t: 123 } });
    expect(s.timeline).toMatchObject({ mode: 'paused', t: 123 });
    s = reduce(s, { type: 'timeline', patch: { mode: 'live' } });
    expect(s.timeline.t).toBeNull();
    s = reduce(s, { type: 'filter', patch: { hideTests: true } });
    s = reduce(s, { type: 'filter', patch: { community: 2 } });
    expect(s.filter).toEqual({ langs: [], community: 2, hideTests: true });
  });

  it('createStore notifies only on change', () => {
    const st = createStore(base());
    let calls = 0;
    st.subscribe(() => calls++);
    st.dispatch({ type: 'hover', id: null });
    expect(calls).toBe(0);
    st.dispatch({ type: 'hover', id: 100 });
    expect(calls).toBe(1);
    expect(st.getState().hover).toBe(100);
  });
});
