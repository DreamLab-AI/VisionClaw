/**
 * Real swarm agents ride the ADR-2142 demo pathway (owner rule, 2026-11-01): agents are not graph nodes, every
 * agent reaches the desktop only as 0x23 frames, the client registry is the one source of embodiment, and the
 * marker pass is the only agent position writer. These tests decode the SHARED fixture (written from the server's
 * transform) with the real `decodeAgentActions` and drive the real registry and marker pass.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { useAgentTargetStore } from '../../../store/agentTargetStore';
import { useTransientBeamStore } from '../../../store/transientBeamStore';
import { AgentMarkerPass, agentMarkers, agentMarkerPosition } from '../agentMarkerPass';
import { agentHoverOffset, VIRTUAL_HOVER_RADIUS, VIRTUAL_HOVER_LIFT, VIRTUAL_ROLES, virtualWireId } from '../virtualAgents';
import { separatedFrame } from '../../graph/triLayout';
import { AGENT_EDGE_SECONDS } from '../../graph/utils/agentNudge';
import { trailSamplePoint } from '../components/AgentTrail';
import { nameplateAnchor, nameplateText } from '../components/AgentNameplate';
import { edgeEndpoints } from '../components/AgentCollabEdges';
import { pairMidpoint, groupChainLabels } from '../chain/ChainPaymentLabels';
import { AgentActionType, type AgentActionEvent } from '../../../services/binaryProtocol/frameTypes';
import { decodeFrame, loadRealAgentFrames, parsedPayloads } from './realAgentFrames';

const NODE = { x: 100, y: 0, z: 0 };
const EDGE = { centre: [0, 0, 0] as [number, number, number], radius: 200 };
const T0 = 1_000_000;

const store = () => useAgentTargetStore.getState();
let wall = T0;
let run = 0; // the frame clock (performance.now), ms

/** One frame of the marker pass, as GraphManager runs it. Every node (id 7) sits at NODE. */
function frame(pass: AgentMarkerPass = agentMarkers, edge = EDGE) {
  pass.step({
    registry: store(),
    resolveNode: (_id, out) => { out.x = NODE.x; out.y = NODE.y; out.z = NODE.z; return true; },
    drift: null,
    triFrame: separatedFrame(),
    offsetsEnabled: true,
    edge,
    nowMs: run,
    wallMs: wall,
  });
}
const advance = (ms: number) => { wall += ms; run += ms; vi.setSystemTime(wall); };
const push = (events: AgentActionEvent[]) => store().pushActions(events);
const posOf = (id: number) => { const o = { x: NaN, y: NaN, z: NaN }; const ok = agentMarkerPosition(id, { set: (x, y, z) => { o.x = x; o.y = y; o.z = z; } }); return ok ? o : null; };
const dist = (p: { x: number; y: number; z: number }) => Math.hypot(p.x - EDGE.centre[0], p.y - EDGE.centre[1], p.z - EDGE.centre[2]);
const idOf = () => decodeFrame('spawn')[0].sourceAgentId;
/** Where an agent hovers: beside its work node, or around the scene centre when it has none (the fixture's frames name none). */
const anchorOf = (id: number) => (store().targets.get(id) ? NODE : { x: EDGE.centre[0], y: EDGE.centre[1], z: EDGE.centre[2] });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  wall = T0; run = 0; vi.setSystemTime(wall);
  store().clear();
  agentMarkers.clear();
  useTransientBeamStore.getState().clear();
});
afterEach(() => { vi.useRealTimers(); });

describe('the shared fixture decodes with the real decoder', () => {
  it.each(['spawn', 'keepalive', 'done', 'retired'] as const)('%s: decoded events equal the fixture fields and payload', (name) => {
    const frame = loadRealAgentFrames().find(f => f.name === name)!;
    const events = decodeFrame(name);
    expect(events.length).toBe(frame.events.length);
    events.forEach((e, i) => {
      const want = frame.events[i];
      expect([e.sourceAgentId, e.targetNodeId, e.actionType, e.timestamp, e.durationMs])
        .toEqual([want.sourceAgentId, want.targetNodeId, want.actionType, want.timestamp, want.durationMs]);
    });
    expect(parsedPayloads(events)).toEqual(frame.events.map(e => e.payload));
  });

  it('real agents sit in the flagged band 0x82000000..0x83FFFFFF, state frames carry no action', () => {
    const e = decodeFrame('spawn')[0];
    expect(e.sourceAgentId).toBeGreaterThanOrEqual(0x82000000);
    expect(e.sourceAgentId).toBeLessThanOrEqual(0x83ffffff);
    expect(e.actionType).toBe(0);
    expect(e.durationMs).toBe(0);
  });
});

describe('a real task-1 agent is a registry agent with a marker', () => {
  it('appears in the registry on its spawn frame, with its name and working state', () => {
    push(decodeFrame('spawn'));
    const id = idOf();
    expect(store().seenAt.has(id)).toBe(true);
    expect(store().agents.get(id)).toMatchObject({ state: 'working', name: 'agent task-1' });
  });

  it('gets a marker position on the next pass, on the hover ring (no target: around the scene centre)', () => {
    push(decodeFrame('spawn'));
    frame();
    const p = posOf(idOf())!;
    expect(p).not.toBeNull();
    const target = store().targets.get(idOf());
    const anchor = target ? NODE : { x: EDGE.centre[0], y: EDGE.centre[1], z: EDGE.centre[2] };
    expect(Math.hypot(p.x - anchor.x, p.z - anchor.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
    expect(p.y).toBeCloseTo(anchor.y + VIRTUAL_HOVER_LIFT, 5);
  });

  it('keeps its id and its marker across the keep-alive frame', () => {
    push(decodeFrame('spawn'));
    frame();
    const id = idOf();
    const first = posOf(id)!;
    const sizeBefore = store().agents.size;
    advance(5_000);
    push(decodeFrame('keepalive'));
    frame();
    expect(store().agents.size).toBe(sizeBefore);
    expect(decodeFrame('keepalive')[0].sourceAgentId).toBe(id);
    expect(agentMarkers.positions.size).toBe(1);
    const second = posOf(id)!;
    // the keep-alive may name the last work node; the marker is then beside it, still on its ring
    const target = store().targets.get(id);
    if (!target) expect(second).toEqual(first);
    else expect(Math.hypot(second.x - NODE.x, second.z - NODE.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
  });

  it('every wire id with live evidence is an agent, whatever its range', () => {
    const ev = (src: number): AgentActionEvent => ({ sourceAgentId: src, targetNodeId: 7, actionType: AgentActionType.Query, timestamp: 0, durationMs: 1000 });
    const ids = [0x82000123, 0x83ffffff, 0x800003e8, 0x8000d001, 0x80123456];
    push(ids.map(ev));
    frame();
    expect(agentMarkers.positions.size).toBe(ids.length);
    for (const id of ids) expect(posOf(id)).not.toBeNull();
  });
});

describe('done runs to the scene edge over 30 s, then holds', () => {
  it('runs outward on the frame clock, never passes the edge, and is bit-constant once there', () => {
    push(decodeFrame('spawn'));
    advance(5_000); push(decodeFrame('keepalive'));
    frame();
    const id = idOf();
    const doneFrame = decodeFrame('done');
    advance(1_000);
    push(doneFrame);
    frame();
    let prev = dist(posOf(id)!);
    expect(prev).toBeLessThan(EDGE.radius - 20); // starts well inside
    let arrived = -1;
    let last = posOf(id)!;
    for (let s = 0.1; s <= 36; s += 0.1) {
      advance(100);
      if (Math.round(s * 10) % 50 === 0) push(doneFrame); // the server resends state every 5 s
      frame();
      const p = posOf(id)!;
      expect(p).not.toBeNull(); // presence never lapses while the keep-alive arrives
      const d = dist(p);
      expect(d).toBeGreaterThanOrEqual(prev - 1e-4);
      expect(d).toBeLessThanOrEqual(EDGE.radius + 1e-4);
      prev = d;
      if (arrived < 0 && Math.abs(d - EDGE.radius) < 2e-4) arrived = s;
      if (arrived >= 0 && s > arrived + 0.5) expect(p).toEqual(last); // holds, bit for bit
      last = p;
    }
    expect(arrived).toBeGreaterThan(AGENT_EDGE_SECONDS - 1.2);
    expect(arrived).toBeLessThan(AGENT_EDGE_SECONDS + 0.3);
  });

  it('a working agent stays beside its target however long it works (R3: no nudge into the node)', () => {
    push(decodeFrame('spawn'));
    push(decodeFrame('keepalive'));
    const id = idOf();
    frame();
    const first = posOf(id)!;
    for (let i = 0; i < 600; i++) { advance(50); if (i % 100 === 0) push(decodeFrame('keepalive')); frame(); }
    expect(posOf(id)).toEqual(first);
  });

  it('going back to work after done brings the marker home (no stranding at the edge)', () => {
    push(decodeFrame('keepalive'));
    frame();
    const id = idOf();
    const home = posOf(id)!;
    push(decodeFrame('done'));
    for (let i = 0; i < 100; i++) { advance(100); frame(); }
    expect(dist(posOf(id)!)).toBeGreaterThan(dist(home) + 5);
    push(decodeFrame('keepalive')); // working again
    for (let i = 0; i < 400; i++) { advance(50); frame(); }
    const back = posOf(id)!;
    expect(Math.hypot(back.x - home.x, back.y - home.y, back.z - home.z)).toBeLessThan(1e-2);
  });

  it('a done agent whose base is the scene centre still runs out (ring offset, not a zero-distance dead end)', () => {
    push(decodeFrame('spawn'));
    store().targets.delete(idOf()); // no work node on record
    store().agents.set(idOf(), { state: 'done', name: 'agent task-1' });
    frame();
    const d0 = dist(posOf(idOf())!);
    expect(d0).toBeGreaterThan(0);
    for (let i = 0; i < 100; i++) { advance(100); store().seenAt.set(idOf(), wall); frame(); }
    expect(dist(posOf(idOf())!)).toBeGreaterThan(d0 + 5);
  });
});

describe('retired removes the agent at once', () => {
  it('leaves the registry, the marker map and the beam resolver', () => {
    push(decodeFrame('spawn'));
    push(decodeFrame('keepalive'));
    frame();
    const id = idOf();
    expect(posOf(id)).not.toBeNull();
    push(decodeFrame('retired'));
    for (const m of [store().seenAt, store().targets, store().agents]) expect(m.has(id)).toBe(false);
    frame();
    expect(agentMarkers.positions.has(id)).toBe(false);
    expect(posOf(id)).toBeNull();
  });

  it('retire(id) is the same removal, and a later pass leaves no offset behind for a reused id', () => {
    push(decodeFrame('keepalive'));
    push(decodeFrame('done'));
    for (let i = 0; i < 50; i++) { advance(100); frame(); }
    store().retire(idOf());
    frame();
    expect(posOf(idOf())).toBeNull();
    push(decodeFrame('keepalive')); // a fresh agent under the same id starts from its own hover point
    frame();
    const p = posOf(idOf())!;
    const a = anchorOf(idOf());
    expect(Math.hypot(p.x - a.x, p.z - a.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
  });
});

describe('a state frame is presence plus state only', () => {
  it('draws no beam, adds no work record and no drift credit', async () => {
    const { getAgentWork, resetAgentWorkTargets } = await import('../agentWorkTargets');
    resetAgentWorkTargets();
    for (const name of ['spawn', 'keepalive', 'done', 'retired'] as const) {
      useTransientBeamStore.getState().pushBeams(decodeFrame(name));
    }
    expect(useTransientBeamStore.getState().beams).toHaveLength(0);
    expect(getAgentWork(String(idOf()))).toBeNull();
  });

  it('an ordinary action frame from the same agent still beams and clears done', () => {
    push(decodeFrame('done'));
    const id = idOf();
    expect(store().agents.get(id)?.state).toBe('done');
    const action: AgentActionEvent = { sourceAgentId: id, targetNodeId: 9, actionType: AgentActionType.Update, timestamp: 1, durationMs: 1500 };
    useTransientBeamStore.getState().pushBeams([action]);
    push([action]);
    expect(useTransientBeamStore.getState().beams).toHaveLength(1);
    expect(store().agents.get(id)?.state).toBe('working');
    expect(store().targets.get(id)).toBe(9);
  });
});

describe('the label, trail, collaboration edge and payment label all read the marker map', () => {
  it('equal the marker position of the same agent, joined by wire id', () => {
    push(decodeFrame('keepalive'));
    frame();
    const id = idOf();
    const p = posOf(id)!;
    const other = 0x82000042;
    push([{ sourceAgentId: other, targetNodeId: 7, actionType: AgentActionType.Query, timestamp: 0, durationMs: 1000 }]);
    frame();
    const q = posOf(other)!;
    const v = () => ({ x: NaN, y: NaN, z: NaN, set(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; } });

    const trail = v(); expect(trailSamplePoint(id, trail)).toBe(true);
    expect([trail.x, trail.y, trail.z]).toEqual([p.x, p.y, p.z]);

    const label = v(); expect(nameplateAnchor(id, label)).toBe(true);
    expect([label.x, label.y, label.z]).toEqual([p.x, p.y, p.z]);

    // /bots/data ids and edge ends are the wire id masked to 26 bits; the join normalises them
    const a = v(), b = v();
    expect(edgeEndpoints({ srcWire: id & 0x03ffffff, tgtWire: other & 0x03ffffff }, a, b)).toBe(true);
    expect([a.x, a.y, a.z]).toEqual([p.x, p.y, p.z]);
    expect([b.x, b.y, b.z]).toEqual([q.x, q.y, q.z]);

    const groups = groupChainLabels([{
      id: 'e1', source: 'task-1', target: 'task-2', type: 'chain_payment', dataVolume: 0, messageCount: 0, lastMessageTime: 0,
      sourceWireId: id & 0x03ffffff, targetWireId: other & 0x03ffffff,
      chainPayment: { txid: 'ab'.repeat(32), amountSats: 5, settled: true } as never,
    }]);
    const mid = v();
    expect(pairMidpoint(groups[0], mid)).toBe(true);
    expect(mid.x).toBeCloseTo((p.x + q.x) / 2, 6);
    expect(mid.y).toBeCloseTo((p.y + q.y) / 2, 6);
    expect(mid.z).toBeCloseTo((p.z + q.z) / 2, 6);
  });

  it('none of them has a position once the agent is retired', () => {
    push(decodeFrame('keepalive'));
    frame();
    push(decodeFrame('retired'));
    frame();
    const o = { set() {} };
    expect(trailSamplePoint(idOf(), o)).toBe(false);
    expect(nameplateAnchor(idOf(), o)).toBe(false);
    expect(edgeEndpoints({ srcWire: idOf() & 0x03ffffff, tgtWire: 1 }, o, o)).toBe(false);
  });

  it('the nameplate takes the registry name, or the /bots/data record name when there is one', () => {
    expect(nameplateText(0x82000001, 'agent task-1', undefined)).toBe('agent task-1');
    expect(nameplateText(0x82000001, 'agent task-1', { name: 'Coder 7' } as never)).toBe('Coder 7');
    expect(nameplateText(0x82000001, undefined, undefined)).toMatch(/0x82000001|82000001/i);
  });
});

describe('a demo agent still works, through the same path', () => {
  it('a reserved id keeps its role, hovers beside its target, and leaves on clearVirtual', () => {
    const demo = virtualWireId(0xd003);
    push([{ sourceAgentId: demo, targetNodeId: 7, actionType: AgentActionType.Query, timestamp: 0, durationMs: 2000 }]);
    frame();
    const p = posOf(demo)!;
    expect(Math.hypot(p.x - NODE.x, p.z - NODE.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
    store().clearVirtual();
    frame();
    expect(posOf(demo)).toBeNull();
  });

  it('a real agent survives clearVirtual', () => {
    push(decodeFrame('keepalive'));
    store().clearVirtual();
    frame();
    expect(posOf(idOf())).not.toBeNull();
  });
});

describe('the hover ring is a function of the id hash', () => {
  it('keeps the six demo agents at least 25 degrees apart, and is stable', () => {
    const angles = VIRTUAL_ROLES.map(r => {
      const o = agentHoverOffset(virtualWireId(r.suffix), 20, 8);
      expect(o).toEqual(agentHoverOffset(virtualWireId(r.suffix), 20, 8));
      expect(Math.hypot(o.x, o.z)).toBeCloseTo(20, 5);
      return Math.atan2(o.z, o.x);
    }).sort((a, b) => a - b);
    for (let i = 0; i < angles.length; i++) {
      const next = i + 1 < angles.length ? angles[i + 1] : angles[0] + 2 * Math.PI;
      expect(next - angles[i]).toBeGreaterThan((25 * Math.PI) / 180);
    }
  });

  it('spreads real ids (25 hash bits) too: no two of 12 consecutive hashes stack', () => {
    const seen = new Set<string>();
    for (let k = 0; k < 12; k++) {
      const o = agentHoverOffset((0x82000000 | (0x155aa5 + k)) >>> 0, 20, 8);
      seen.add(`${o.x.toFixed(2)},${o.z.toFixed(2)}`);
    }
    expect(seen.size).toBe(12);
  });
});

describe('unmapped agentbox ids hover beside their target, never start from a knowledge node (R4)', () => {
  it('the dev driver id 1000+a (agent flag) resolves to its marker, not to node 1000', () => {
    const id = (0x80000000 | 1003) >>> 0;
    push([{ sourceAgentId: id, targetNodeId: 7, actionType: AgentActionType.Query, timestamp: 0, durationMs: 1000 }]);
    frame();
    const p = posOf(id)!;
    expect(Math.hypot(p.x - NODE.x, p.z - NODE.z)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
  });
});
