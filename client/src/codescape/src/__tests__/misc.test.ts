import { describe, expect, it } from 'vitest';
import { Decimator2, f32ToLeBytes, Framer, leBytesToF32, LinearResampler, lowpassTaps } from '../audio/pcm';
import { normalise, parseLocalCommand } from '../commands';
import { buildDemoWorld, demoQuery } from '../demo/demoGraph';
import { buildModel } from '../graph';
import { FramePipeline } from '../net/FramePipeline';
import { backoffDelay, isDemo, resolveEndpoints } from '../net/config';
import { applyPositionFrame, createNodeBuffers, encodeV5Frame, type PresenceEvent } from '../protocol';
import { actorFocusAt, heatAt, trailOf } from '../timeline';
import { smallSnapshot } from './fixtures';

describe('local command grammar', () => {
  it('normalises spoken input', () => {
    expect(normalise('Um, please ZOOM in!')).toBe('zoom in');
    expect(normalise('number two')).toBe('number 2');
  });
  it('parses view commands', () => {
    expect(parseLocalCommand('overview')).toEqual({ kind: 'overview' });
    expect(parseLocalCommand('number three')).toEqual({ kind: 'pick', n: 3 });
    expect(parseLocalCommand('2')).toEqual({ kind: 'pick', n: 2 });
    expect(parseLocalCommand('show only rust')).toEqual({ kind: 'show_only', lang: 'rust', community: null });
    expect(parseLocalCommand('show only community 4')).toEqual({ kind: 'show_only', lang: null, community: 4 });
    expect(parseLocalCommand('isolate')).toEqual({ kind: 'isolate', community: null });
    expect(parseLocalCommand('rewind 2 minutes')).toEqual({ kind: 'rewind', seconds: 120 });
    expect(parseLocalCommand('rewind thirty seconds')).toEqual({ kind: 'rewind', seconds: 30 });
    expect(parseLocalCommand('follow claude')).toEqual({ kind: 'follow', actor: 'claude' });
    expect(parseLocalCommand('hide tests')).toEqual({ kind: 'hide_tests' });
    expect(parseLocalCommand('focus parse_command')).toBeNull();
    expect(parseLocalCommand('callers of apply')).toBeNull();
    expect(parseLocalCommand('show only klingon')).toBeNull();
  });
});

describe('endpoints and backoff', () => {
  it('uses the same origin by default and ?server= when given', () => {
    const same = resolveEndpoints({ protocol: 'https:', host: 'cs.example:4100', search: '' });
    expect(same).toMatchObject({ http: '', positions: 'wss://cs.example:4100/ws/positions', events: 'wss://cs.example:4100/ws/events', voice: 'wss://cs.example:4100/ws/voice' });
    const o = resolveEndpoints({ protocol: 'http:', host: 'localhost:5173', search: '?server=http://10.0.0.2:4100' });
    expect(o).toMatchObject({ http: 'http://10.0.0.2:4100', positions: 'ws://10.0.0.2:4100/ws/positions', label: 'http://10.0.0.2:4100' });
    expect(resolveEndpoints({ protocol: 'http:', host: 'h', search: '?server=box:4100' }).events).toBe('ws://box:4100/ws/events');
    expect(isDemo('?demo=1')).toBe(true);
    expect(isDemo('?demo=0')).toBe(false);
    expect(isDemo('')).toBe(false);
  });
  it('backs off exponentially with jitter and a cap', () => {
    expect(backoffDelay(0, 0.5)).toBe(500);
    expect(backoffDelay(3, 0.5)).toBe(4000);
    expect(backoffDelay(30, 0.5)).toBe(15_000);
    expect(backoffDelay(0, 0)).toBe(400);
    expect(backoffDelay(0, 0.999)).toBeLessThanOrEqual(600);
  });
});

describe('PCM pipeline', () => {
  const sine = (hz: number, rate: number, n: number) => Float32Array.from({ length: n }, (_, i) => Math.sin((2 * Math.PI * hz * i) / rate));
  const rms = (x: Float32Array) => Math.sqrt(x.reduce((a, v) => a + v * v, 0) / x.length);

  it('low-pass taps have unity DC gain', () => {
    expect(lowpassTaps(31, 0.22).reduce((a, b) => a + b, 0)).toBeCloseTo(1, 6);
  });
  it('decimates 48k → 24k seamlessly across odd and even chunks', () => {
    const d = new Decimator2();
    const total = [480, 127, 129, 128, 4].reduce((acc, n) => acc + d.process(new Float32Array(n).fill(1)).length, 0);
    expect(total).toBe(Math.floor((480 + 127 + 129 + 128 + 4 + 1) / 2));
    const steady = d.process(new Float32Array(64).fill(1));
    steady.forEach((v) => expect(v).toBeCloseTo(1, 4));
  });
  it('passes speech-band tones and rejects content above the new Nyquist', () => {
    const d1 = new Decimator2();
    const pass = d1.process(sine(1000, 48_000, 4800)).subarray(100);
    expect(rms(pass)).toBeCloseTo(Math.SQRT1_2, 1);
    const d2 = new Decimator2();
    const stop = d2.process(sine(18_000, 48_000, 4800)).subarray(100);
    expect(rms(stop)).toBeLessThan(0.05);
  });
  it('linear resampler produces the right count across chunks', () => {
    const r = new LinearResampler(44_100, 24_000);
    let out = 0;
    for (let k = 0; k < 100; k++) out += r.process(new Float32Array(441).fill(0.5)).length;
    expect(Math.abs(out - 24_000)).toBeLessThanOrEqual(2);
  });
  it('frames samples into fixed 20 ms chunks and round-trips LE bytes', () => {
    const f = new Framer(480);
    const frames: Float32Array[] = [];
    f.push(new Float32Array(1000), (x) => frames.push(x));
    expect(frames.map((x) => x.length)).toEqual([480, 480]);
    f.flush((x) => frames.push(x));
    expect(frames[2].length).toBe(40);
    const bytes = f32ToLeBytes(Float32Array.from([1.5, -2]));
    expect(Array.from(new Uint8Array(bytes))).toEqual([0, 0, 0xc0, 0x3f, 0, 0, 0, 0xc0]);
    expect(Array.from(leBytesToF32(bytes))).toEqual([1.5, -2]);
  });
});

describe('timeline replay', () => {
  const ev = (actor: number, ids: number[], ts: number, kind: PresenceEvent['kind'] = 'update'): PresenceEvent => ({ actor, actor_label: 'a', actor_kind: 'agent', kind, node_ids: ids, paths: [], text: null, ts_ms: ts });
  const log = [ev(1, [10], 0), ev(2, [20], 30_000, 'query'), ev(1, [11, 12], 60_000)];
  it('heat decays with a 60 s half-life and ignores the future', () => {
    const h = heatAt(log, 60_000);
    expect(h.get(10)).toBeCloseTo(0.5, 6);
    expect(h.get(12)).toBeCloseTo(1, 6);
    expect(h.get(20)).toBeCloseTo(0.6 * Math.pow(0.5, 0.5), 6);
    expect(heatAt(log, 10_000).has(11)).toBe(false);
  });
  it('actor focus and trails at a time', () => {
    expect(actorFocusAt(log, 45_000).get(1)).toBe(10);
    expect(actorFocusAt(log, 60_000).get(1)).toBe(12);
    expect(trailOf(log, 1, 60_000, 5).map((x) => x.id)).toEqual([10, 11, 12]);
    expect(trailOf(log, 1, 60_000, 2).map((x) => x.id)).toEqual([11, 12]);
  });
});

describe('frame pipeline (main-thread fallback) and demo', () => {
  it('carries positions across a model change and seeds new nodes at their parent', () => {
    const applied: number[] = [];
    const p = new FramePipeline((r) => applied.push(r.seq));
    expect(p.shared).toBe(false);
    const m1 = buildModel(smallSnapshot());
    p.setModel(m1, 100);
    p.push(encodeV5Frame(3, [{ id: 10, position: [50, 60, 70] }, { id: 100, position: [1, 2, 3] }]));
    expect(applied).toEqual([3]);
    const snap2 = smallSnapshot();
    snap2.nodes.push({ ...snap2.nodes[3], id: 102, key: 'k102', parent: 10 });
    const m2 = buildModel(snap2);
    p.setModel(m2, 100);
    const b = p.buffers;
    const i100 = m2.index.get(100)!;
    expect([b.positions[i100 * 3], b.positions[i100 * 3 + 1], b.positions[i100 * 3 + 2]]).toEqual([1, 2, 3]);
    const i102 = m2.index.get(102)!;
    expect(Math.abs(b.positions[i102 * 3] - 50)).toBeLessThan(2);
    expect(Math.abs(b.positions[i102 * 3 + 2] - 70)).toBeLessThan(2);
    p.dispose();
  });

  it('builds a ~5k-node demo graph with valid parents that decodes through the real wire', () => {
    const w = buildDemoWorld(5000);
    const m = buildModel(w.snapshot);
    expect(m.n).toBe(5000);
    expect([...m.tier].filter((t) => t === 0).length).toBeGreaterThan(5);
    for (const n of w.snapshot.nodes) if (n.parent) expect(m.index.has(n.parent)).toBe(true);
    expect(m.calls.length).toBeGreaterThan(1000);
    const frame = encodeV5Frame(1, w.snapshot.nodes.map((n, i) => ({ id: n.id, position: [w.base[i * 3], w.base[i * 3 + 1], w.base[i * 3 + 2]] as [number, number, number] })));
    const b = createNodeBuffers(m.n, false);
    expect(applyPositionFrame(frame, b, m.index)!.applied).toBe(5000);
    const q = demoQuery(m, `explain ${m.snapshot.nodes[4000].label}`);
    expect(q.node_ids.length + q.candidates.length).toBeGreaterThan(0);
    expect(demoQuery(m, 'path from crates to scripts').path.length).toBeGreaterThan(1);
  });
});
