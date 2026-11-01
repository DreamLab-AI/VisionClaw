/**
 * The marker pass (ADR-2142 / ADR-2135): the finished-agent run to the scene edge, drift, offsets and hiding.
 * These pins moved here from `drawnPositions.test.ts` when the agent pass left the drawn-positions array
 * (arrival at 60 and 120 fps, never past the edge, bit-constant once there, D5 no teleport).
 */
import { describe, it, expect } from 'vitest';
import { AgentMarkerPass, type MarkerPassInput, type MarkerRegistry } from '../agentMarkerPass';
import { AgentDriftFeed } from '../agentDriftFeed';
import { separatedFrame } from '../../graph/triLayout';
import { VIRTUAL_HOVER_LIFT, VIRTUAL_HOVER_RADIUS, VIRTUAL_AGENT_TTL_MS } from '../virtualAgents';

const ID = 0x82000001;
const NODE = { x: 10, y: 5, z: -4 };
const EDGE = { centre: [0, 0, 0] as [number, number, number], radius: 60 };

function registry(state: 'working' | 'done', target: number | undefined = 5, seen = 0): MarkerRegistry & { agents: Map<number, { state: 'working' | 'done' }> } {
  return {
    targets: new Map(target === undefined ? [] : [[ID, target]]),
    seenAt: new Map([[ID, seen]]),
    agents: new Map([[ID, { state }]]),
  };
}
function input(reg: MarkerRegistry, over: Partial<MarkerPassInput> = {}): MarkerPassInput {
  return {
    registry: reg,
    resolveNode: (_id, out) => { out.set(NODE.x, NODE.y, NODE.z); return true; },
    drift: null, triFrame: separatedFrame(), offsetsEnabled: true, edge: EDGE, nowMs: 0, wallMs: 0, ...over,
  };
}
const pos = (p: AgentMarkerPass) => ({ ...p.positions.get(ID)! });
const dist = (v: { x: number; y: number; z: number }) => Math.hypot(v.x, v.y, v.z);

describe('a finished agent reaches the scene edge in about 30 s and stops there (owner decision)', () => {
  for (const fps of [60, 120]) {
    it(`at ${fps} fps: arrives on time, never passes the edge, then is bit-constant`, () => {
      const p = new AgentMarkerPass();
      const reg = registry('done');
      let prev = 0;
      let arrivedAt = -1;
      let last = { x: NaN, y: NaN, z: NaN };
      for (let f = 0; f <= fps * 36; f++) {
        const t = f / fps;
        reg.seenAt.set(ID, t * 1000); // keep-alive
        p.step(input(reg, { nowMs: 1000 + t * 1000, wallMs: t * 1000 }));
        const v = pos(p);
        const d = dist(v);
        expect(d).toBeGreaterThanOrEqual(prev - 1e-4);
        expect(d).toBeLessThanOrEqual(EDGE.radius + 1e-4);
        if (f === fps * 20) expect(EDGE.radius - d).toBeGreaterThan(1);
        prev = d;
        if (arrivedAt < 0 && Math.abs(d - EDGE.radius) < 2e-4) arrivedAt = t;
        if (arrivedAt >= 0 && t > 30.2) expect(v).toEqual(last);
        last = v;
      }
      expect(arrivedAt).toBeGreaterThan(29.0);
      expect(arrivedAt).toBeLessThan(30.2);
    });
  }

  it('with the real drift feed it still never passes the edge', () => {
    let now = 100;
    const feed = new AgentDriftFeed({ lookupNode: () => ({ metadata: { type: 'page' } }), agentKeys: () => [String(ID)], nowS: () => now });
    feed.ingestBeams([{ id: 1, sourceAgentId: ID, targetNodeId: 5 }]);
    const p = new AgentMarkerPass();
    const reg = registry('done');
    for (let f = 0; f < 60 * 60; f++) {
      now += 1 / 60;
      reg.seenAt.set(ID, now * 1000);
      p.step(input(reg, { drift: feed, nowMs: now * 1000, wallMs: now * 1000 }));
      expect(dist(pos(p))).toBeLessThanOrEqual(EDGE.radius + 1e-3);
    }
  });

  it('with no known edge a finished agent stays at its hover point', () => {
    const p = new AgentMarkerPass();
    const reg = registry('done');
    p.step(input(reg, { edge: null }));
    const first = pos(p);
    for (let i = 1; i < 200; i++) p.step(input(reg, { edge: null, nowMs: i * 100 }));
    expect(pos(p)).toEqual(first);
  });
});

describe('offsets', () => {
  it('a working agent sits on its hover ring: beside its node, VIRTUAL_HOVER_LIFT above it', () => {
    const p = new AgentMarkerPass();
    p.step(input(registry('working')));
    const v = pos(p);
    expect(Math.hypot(v.x - NODE.x, v.z - NODE.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
    expect(v.y).toBeCloseTo(NODE.y + VIRTUAL_HOVER_LIFT, 5);
  });

  it('offsets off: the marker stays at its hover point; turning them on after a long gap restarts the run (D5, no jump)', () => {
    const p = new AgentMarkerPass();
    const reg = registry('done');
    p.step(input(reg, { offsetsEnabled: false, nowMs: 0, wallMs: 0 }));
    const home = pos(p);
    for (let i = 1; i <= 400; i++) { reg.seenAt.set(ID, i * 100); p.step(input(reg, { offsetsEnabled: false, nowMs: i * 100, wallMs: i * 100 })); }
    expect(pos(p)).toEqual(home);
    reg.seenAt.set(ID, 40_100);
    p.step(input(reg, { nowMs: 40_100, wallMs: 40_100 }));
    expect(Math.hypot(pos(p).x - home.x, pos(p).y - home.y, pos(p).z - home.z)).toBeLessThan(1e-3);
  });

  it('steps the drift feed once per frame, and only when an agent is drawn', () => {
    let steps = 0;
    const drift = { step: () => { steps++; }, offset: () => [0, 0, 0] as const };
    const p = new AgentMarkerPass();
    p.step(input(registry('working'), { drift }));
    p.step(input(registry('working'), { drift }));
    expect(steps).toBe(2);
    p.step(input({ targets: new Map(), seenAt: new Map(), agents: new Map() }, { drift }));
    expect(steps).toBe(2);
  });

  it('drift is added to the hover point under the agent wire id string', () => {
    const keys: string[] = [];
    const drift = { step() {}, offset: (k: string) => { keys.push(k); return [1, 2, 3] as const; } };
    const p = new AgentMarkerPass();
    p.step(input(registry('working'), { drift }));
    expect(keys).toEqual([String(ID)]);
    const v = pos(p);
    expect(v.y).toBeCloseTo(NODE.y + VIRTUAL_HOVER_LIFT + 2, 5);
  });
});

describe('presence and hiding', () => {
  it('an agent past the 30 s window, or whose work node is off screen, has no marker, and its offset does not outlive it', () => {
    const p = new AgentMarkerPass();
    const reg = registry('done');
    for (let i = 0; i < 100; i++) p.step(input(reg, { nowMs: i * 100, wallMs: 100 }));
    expect(p.positions.has(ID)).toBe(true);
    p.step(input(reg, { wallMs: VIRTUAL_AGENT_TTL_MS + 1 }));
    expect(p.positions.has(ID)).toBe(false);
    p.step(input(registry('working'), { resolveNode: () => false }));
    expect(p.positions.has(ID)).toBe(false);
    // a new occupant of the id starts from its hover point, not from the old run
    p.step(input(registry('working'), { nowMs: 1e6, wallMs: 0 }));
    expect(Math.hypot(pos(p).x - NODE.x, pos(p).z - NODE.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
  });

  it('agents do not share an offset: two done agents run independently', () => {
    const p = new AgentMarkerPass();
    const A = ID, B = ID + 1;
    const reg: MarkerRegistry = {
      targets: new Map([[A, 5], [B, 5]]),
      seenAt: new Map([[A, 0], [B, 0]]),
      agents: new Map([[A, { state: 'done' as const }], [B, { state: 'working' as const }]]),
    };
    for (let i = 0; i < 100; i++) p.step(input(reg, { nowMs: i * 100 }));
    const a = p.positions.get(A)!, b = p.positions.get(B)!;
    expect(dist(a)).toBeGreaterThan(dist(b) + 5);
  });
});
