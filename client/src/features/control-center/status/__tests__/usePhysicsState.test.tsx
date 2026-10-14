import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

const get = vi.fn();
vi.mock('../../../../services/api/UnifiedApiClient', () => ({ unifiedApiClient: { get: (...a: unknown[]) => get(...a) } }));

import { usePhysicsState, PHYSICS_STATE_ENDPOINT } from '../usePhysicsState';

describe('usePhysicsState', () => {
  beforeEach(() => get.mockReset());
  afterEach(() => vi.useRealTimers());

  it('asks for a path relative to the API client base, which already adds "/api"', async () => {
    get.mockResolvedValue({ data: {} });
    renderHook(() => usePhysicsState(60_000));
    await waitFor(() => expect(get).toHaveBeenCalled());
    expect(get.mock.calls[0][0]).toBe('/graph/settlement');
    expect(PHYSICS_STATE_ENDPOINT.startsWith('/api')).toBe(false);
  });

  it('returns the parsed state on success and null when the request fails', async () => {
    get.mockResolvedValueOnce({
      data: { settlementState: { isSettled: false, stableFrameCount: 0, kineticEnergy: 2.5 }, telemetryLive: true, physicsRunning: true },
    });
    const { result } = renderHook(() => usePhysicsState(60_000));
    await waitFor(() => expect(result.current?.kineticEnergy).toBe(2.5));
    expect(result.current?.physicsRunning).toBe(true);
  });

  it('is null (shown as Unavailable) when the body is not a settlement document', async () => {
    get.mockResolvedValue({ data: { error: 'nope' } });
    const { result } = renderHook(() => usePhysicsState(60_000));
    await waitFor(() => expect(get).toHaveBeenCalled());
    await Promise.resolve();
    expect(result.current).toBeNull();
  });
});
