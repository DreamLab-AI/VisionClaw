import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { useAgentTargetStore } from '../agentTargetStore';
import { AgentActionType } from '../../services/binaryProtocol/frameTypes';

const ev = (src: number, target: number) => ({
  sourceAgentId: src,
  targetNodeId: target,
  actionType: AgentActionType.Query,
  timestamp: 0,
  durationMs: 2000,
  payload: new Uint8Array(),
});

describe('agentTargetStore presence (ADR-2142)', () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(10_000); useAgentTargetStore.getState().clear(); });
  afterEach(() => vi.useRealTimers());

  it('records when each agent was last heard from, and refreshes it on every action', () => {
    const s = useAgentTargetStore.getState();
    s.pushActions([ev(0x8000d001, 42)]);
    expect(useAgentTargetStore.getState().seenAt.get(0x8000d001)).toBe(10_000);
    vi.setSystemTime(15_000);
    s.pushActions([ev(0x8000d001, 43)]);
    expect(useAgentTargetStore.getState().seenAt.get(0x8000d001)).toBe(15_000);
    expect(useAgentTargetStore.getState().targets.get(0x8000d001)).toBe(43);
  });

  it('removing an agent forgets both its target and its presence', () => {
    const s = useAgentTargetStore.getState();
    s.pushActions([ev(7, 1)]);
    s.remove(7);
    expect(useAgentTargetStore.getState().targets.has(7)).toBe(false);
    expect(useAgentTargetStore.getState().seenAt.has(7)).toBe(false);
  });

  it('clearVirtual drops the six reserved agents and leaves real agents alone', () => {
    const s = useAgentTargetStore.getState();
    s.pushActions([ev(0x8000d001, 1), ev(0x8000d006, 2), ev(0x80000010, 3)]);
    s.clearVirtual();
    const st = useAgentTargetStore.getState();
    expect([...st.targets.keys()]).toEqual([0x80000010]);
    expect([...st.seenAt.keys()]).toEqual([0x80000010]);
  });
});
