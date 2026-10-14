import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

const get = vi.fn();
const post = vi.fn();
vi.mock('../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: { get: (...a: unknown[]) => get(...a), post: (...a: unknown[]) => post(...a) },
}));

import { useAgentDemo, DEMO_ENDPOINT } from '../agentDemo';
import { useAgentTargetStore } from '../../../store/agentTargetStore';
import { AgentActionType } from '../../../services/binaryProtocol/frameTypes';

const status = (running: boolean) => ({ data: { running, agentIds: running ? [0x8000d001] : [] } });

describe('useAgentDemo (the one shared demo control)', () => {
  beforeEach(() => { get.mockReset(); post.mockReset(); useAgentTargetStore.getState().clear(); });
  afterEach(() => vi.useRealTimers());

  it('talks to the API client with a path relative to its /api base', () => {
    expect(DEMO_ENDPOINT).toBe('/agents/demo');
  });

  it('shows the server state, so a demo started from the headset reads as running here', async () => {
    get.mockResolvedValue(status(true));
    const { result } = renderHook(() => useAgentDemo(60_000));
    await waitFor(() => expect(result.current.running).toBe(true));
  });

  it('toggle posts the opposite of the current state and adopts the answer', async () => {
    get.mockResolvedValue(status(false));
    post.mockResolvedValue(status(true));
    const { result } = renderHook(() => useAgentDemo(60_000));
    await waitFor(() => expect(get).toHaveBeenCalled());
    await act(async () => { await result.current.toggle(); });
    expect(post).toHaveBeenCalledWith(DEMO_ENDPOINT, { running: true });
    expect(result.current.running).toBe(true);
    await act(async () => { post.mockResolvedValue(status(false)); await result.current.toggle(); });
    expect(post).toHaveBeenLastCalledWith(DEMO_ENDPOINT, { running: false });
    expect(result.current.running).toBe(false);
  });

  it('surfaces the server refusal instead of pretending it started', async () => {
    get.mockResolvedValue(status(false));
    post.mockRejectedValue(new Error('the graph has no nodes the demo agents can work on'));
    const { result } = renderHook(() => useAgentDemo(60_000));
    await waitFor(() => expect(get).toHaveBeenCalled());
    await act(async () => { await result.current.toggle(); });
    expect(result.current.running).toBe(false);
    expect(result.current.error).toContain('no nodes');
  });

  it('clears the on-screen demo agents the moment it sees the demo stop', async () => {
    vi.useFakeTimers();
    get.mockResolvedValue(status(true));
    useAgentTargetStore.getState().pushActions([{ sourceAgentId: 0x8000d001, targetNodeId: 5, actionType: AgentActionType.Query, timestamp: 0, durationMs: 2000, payload: new Uint8Array() }]);
    const { result } = renderHook(() => useAgentDemo(1_000));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(result.current.running).toBe(true);
    get.mockResolvedValue(status(false)); // stopped from the other client
    await act(async () => { await vi.advanceTimersByTimeAsync(1_100); });
    expect(result.current.running).toBe(false);
    expect(useAgentTargetStore.getState().targets.size).toBe(0);
  });

  it('does not touch real agents when the demo stops', async () => {
    vi.useFakeTimers();
    get.mockResolvedValue(status(true));
    useAgentTargetStore.getState().pushActions([
      { sourceAgentId: 0x80000010, targetNodeId: 9, actionType: AgentActionType.Query, timestamp: 0, durationMs: 2000, payload: new Uint8Array() },
    ]);
    renderHook(() => useAgentDemo(1_000));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    get.mockResolvedValue(status(false));
    await act(async () => { await vi.advanceTimersByTimeAsync(1_100); });
    expect(useAgentTargetStore.getState().targets.has(0x80000010)).toBe(true);
  });
});
