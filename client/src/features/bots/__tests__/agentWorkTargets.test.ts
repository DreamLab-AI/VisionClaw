import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { useTransientBeamStore } from '../../../store/transientBeamStore';
import { getAgentWork, resetAgentWorkTargets } from '../agentWorkTargets';
import type { AgentActionEvent } from '../../../services/binaryProtocol/frameTypes';

function event(agent: number, target: number): AgentActionEvent {
  return {
    sourceAgentId: agent,
    targetNodeId: target,
    actionType: 0 as AgentActionEvent['actionType'],
    timestamp: 0,
    durationMs: 3000,
  };
}

describe('agentWorkTargets', () => {
  beforeEach(() => {
    resetAgentWorkTargets();
    useTransientBeamStore.getState().clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns null for an agent that has never beamed', () => {
    expect(getAgentWork('42')).toBeNull();
  });

  it('sees beams that were pushed BEFORE the first lookup subscribed', () => {
    // Regression: zustand subscribe only fires on later changes, so the tracker
    // must drain the current store state when it first attaches.
    useTransientBeamStore.getState().pushBeams([event(7, 100)]);
    expect(getAgentWork('7')).toEqual({ targetNodeId: '100', state: 'working' });
  });

  it('follows the newest beam per agent after subscribing', () => {
    getAgentWork('7'); // attaches the subscription with an empty store
    useTransientBeamStore.getState().pushBeams([event(7, 100)]);
    useTransientBeamStore.getState().pushBeams([event(7, 200), event(8, 300)]);
    expect(getAgentWork('7')?.targetNodeId).toBe('200');
    expect(getAgentWork('8')?.targetNodeId).toBe('300');
  });

  it('flips to done once the idle threshold elapses', () => {
    const now = vi.spyOn(performance, 'now');
    now.mockReturnValue(1_000);
    useTransientBeamStore.getState().pushBeams([event(7, 100)]);
    expect(getAgentWork('7')?.state).toBe('working');
    now.mockReturnValue(1_000 + 15_001);
    expect(getAgentWork('7')?.state).toBe('done');
    // The target is retained through the idle phase so the sprite still knows
    // where to drift away from.
    expect(getAgentWork('7')?.targetNodeId).toBe('100');
  });
});
