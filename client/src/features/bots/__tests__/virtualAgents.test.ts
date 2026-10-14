import { describe, it, expect } from 'vitest';
import {
  VIRTUAL_AGENT_TTL_MS,
  isVirtualAgentId,
  virtualAgentHoverOffset,
  VIRTUAL_ROLES,
  livePresence,
  resolveVirtualAgentPosition,
  VIRTUAL_HOVER_RADIUS,
  VIRTUAL_HOVER_LIFT,
} from '../virtualAgents';

const FLAG = 0x80000000;

describe('virtual agents (ADR-2142)', () => {
  it('recognises the reserved demo ids with or without the agent flag, and nothing else', () => {
    for (const s of [0xd001, 0xd003, 0xd006, 0xd0ff]) {
      expect(isVirtualAgentId(s)).toBe(true);
      expect(isVirtualAgentId((FLAG | s) >>> 0)).toBe(true);
    }
    for (const id of [0, 1, 0xd000, 0xd100, 0x80000001, 12345]) expect(isVirtualAgentId(id)).toBe(false);
  });

  it('has the six roles, one per reserved id, in the server order', () => {
    expect(VIRTUAL_ROLES.map((r) => r.suffix)).toEqual([0xd001, 0xd002, 0xd003, 0xd004, 0xd005, 0xd006]);
    expect(VIRTUAL_ROLES.map((r) => r.label)).toEqual(['Architect', 'Analyst', 'Coder', 'Reviewer', 'Tester', 'Optimizer']);
  });

  it('hovers each agent at a stable, distinct point around its target, never on it', () => {
    const seen = new Set<string>();
    for (const r of VIRTUAL_ROLES) {
      const a = virtualAgentHoverOffset((FLAG | r.suffix) >>> 0, 20, 8);
      const b = virtualAgentHoverOffset((FLAG | r.suffix) >>> 0, 20, 8);
      expect(a).toEqual(b); // stable: no jitter between frames
      expect(Math.hypot(a.x, a.z)).toBeCloseTo(20, 5);
      expect(a.y).toBe(8);
      seen.add(`${a.x.toFixed(2)},${a.z.toFixed(2)}`);
    }
    expect(seen.size).toBe(6); // six agents on one node do not stack
  });

  it('keeps an agent present for 30 s after its last action and not a moment longer', () => {
    const seenAt = new Map<number, number>([[0x8000d001, 1000]]);
    expect(livePresence(0x8000d001, seenAt, 1000)).toBe(true);
    expect(livePresence(0x8000d001, seenAt, 1000 + VIRTUAL_AGENT_TTL_MS - 1)).toBe(true);
    expect(livePresence(0x8000d001, seenAt, 1000 + VIRTUAL_AGENT_TTL_MS)).toBe(false);
    expect(livePresence(0x8000d002, seenAt, 1000)).toBe(false); // never seen
  });

  it('is longer than the server keep-alive and the longest rest, so a resting agent stays', () => {
    expect(VIRTUAL_AGENT_TTL_MS).toBe(30_000);
    expect(VIRTUAL_AGENT_TTL_MS).toBeGreaterThan(26_000); // longest rest
  });

  describe('resolveVirtualAgentPosition', () => {
    const id = 0x8000d002;
    const node = (_: number, out: { x: number; y: number; z: number }) => { out.x = 100; out.y = 50; out.z = -30; return true; };
    const grab = () => { const o = { x: 0, y: 0, z: 0 }; return { o, set: (x: number, y: number, z: number) => { o.x = x; o.y = y; o.z = z; } }; };

    it('puts a present agent beside its target, at the hover radius and lift', () => {
      const out = grab();
      const ok = resolveVirtualAgentPosition(id, out, { targets: new Map([[id, 7]]), seenAt: new Map([[id, 1000]]) }, node, 2000);
      expect(ok).toBe(true);
      expect(Math.hypot(out.o.x - 100, out.o.z + 30)).toBeCloseTo(VIRTUAL_HOVER_RADIUS, 5);
      expect(out.o.y).toBe(50 + VIRTUAL_HOVER_LIFT);
    });

    it('follows its target as the layout moves it', () => {
      const out = grab();
      const st = { targets: new Map([[id, 7]]), seenAt: new Map([[id, 1000]]) };
      resolveVirtualAgentPosition(id, out, st, node, 1500);
      const first = { ...out.o };
      resolveVirtualAgentPosition(id, out, st, (_n, o) => { o.x = 200; o.y = 50; o.z = -30; return true; }, 1600);
      expect(out.o.x - first.x).toBeCloseTo(100, 5);
    });

    it('is absent once expired, never seen, without a target, or when the target is off screen', () => {
      const out = grab();
      const fresh = { targets: new Map([[id, 7]]), seenAt: new Map([[id, 1000]]) };
      expect(resolveVirtualAgentPosition(id, out, fresh, node, 1000 + VIRTUAL_AGENT_TTL_MS)).toBe(false);
      expect(resolveVirtualAgentPosition(0x8000d003, out, fresh, node, 1500)).toBe(false);
      expect(resolveVirtualAgentPosition(id, out, { targets: new Map(), seenAt: new Map([[id, 1000]]) }, node, 1500)).toBe(false);
      expect(resolveVirtualAgentPosition(id, out, fresh, () => false, 1500)).toBe(false);
      expect(out.o).toEqual({ x: 0, y: 0, z: 0 }); // never touched on failure
    });
  });
});
