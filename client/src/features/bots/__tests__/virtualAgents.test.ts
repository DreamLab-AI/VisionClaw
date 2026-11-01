import { describe, it, expect } from 'vitest';
import {
  VIRTUAL_AGENT_TTL_MS,
  isVirtualAgentId,
  agentHoverOffset,
  agentWireId,
  agentMarkerColour,
  virtualWireId,
  VIRTUAL_ROLES,
  livePresence,
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

  it('hovers each agent at a stable point on a ring around its anchor, from its id hash', () => {
    const a = agentHoverOffset(0x82123456, 20, 8);
    expect(a).toEqual(agentHoverOffset(0x82123456, 20, 8));
    expect(Math.hypot(a.x, a.z)).toBeCloseTo(20, 5);
    expect(a.y).toBe(8);
    // the flag does not matter: the 26-bit id is hashed
    expect(agentHoverOffset(0x02123456, 20, 8)).toEqual(a);
  });

  it('normalises any id form to the flagged unsigned registry key', () => {
    expect(agentWireId(0xd001)).toBe(0x8000d001);
    expect(agentWireId('13')).toBe(0x8000000d);
    expect(agentWireId(0x8200_0007)).toBe(0x82000007);
    expect(agentWireId(0x0200_0007)).toBe(0x82000007);
    expect(agentWireId(0x82000007)).toBeGreaterThan(0);
  });

  it('colours: a demo role keeps its own, a known type takes the type colour, the rest a stable fallback', () => {
    expect(agentMarkerColour(virtualWireId(0xd003))).toBe(VIRTUAL_ROLES[2].colour);
    expect(agentMarkerColour(0x82000001, 'coder', () => '#123456')).toBe('#123456');
    expect(agentMarkerColour(0x82000001)).toBe(agentMarkerColour(0x82000001));
    expect(agentMarkerColour(0x82000001)).toMatch(/^#[0-9a-f]{6}$/);
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
});
