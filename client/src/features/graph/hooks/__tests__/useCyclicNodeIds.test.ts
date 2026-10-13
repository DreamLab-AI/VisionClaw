import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

// The failure case uses a plain rejecting function rather than a vi.fn(): with a
// vi.fn() returning a rejected promise this test failed with the rejection's
// error even though the hook catches it; a plain function does not.
const h = vi.hoisted(() => ({ fail: false, failCalls: 0 }));
const getCycles = vi.hoisted(() => vi.fn());

vi.mock('../../../../api/analyticsApi', () => ({
  analyticsAPI: {
    getCycles: () => {
      if (h.fail) {
        h.failCalls += 1;
        return Promise.reject(new Error('boom'));
      }
      return getCycles();
    },
  },
}));

import { useCyclicNodeIds } from '../useCyclicNodeIds';

describe('useCyclicNodeIds', () => {
  beforeEach(() => {
    getCycles.mockReset();
    h.fail = false;
    h.failCalls = 0;
  });

  it('does not fetch while disabled', () => {
    const { result } = renderHook(() => useCyclicNodeIds(false, 3, 2));
    expect(result.current).toBeNull();
    expect(getCycles).not.toHaveBeenCalled();
  });

  it('exposes the server report as string ids', async () => {
    getCycles.mockResolvedValue({ cyclicNodeIds: [4, 7] });
    const { result } = renderHook(() => useCyclicNodeIds(true, 10, 12));
    await waitFor(() => expect(result.current).not.toBeNull());
    expect([...result.current!].sort()).toEqual(['4', '7']);
  });

  it('refetches when the graph shape changes', async () => {
    getCycles.mockResolvedValue({ cyclicNodeIds: [] });
    const { rerender } = renderHook(({ e }) => useCyclicNodeIds(true, 10, e), {
      initialProps: { e: 12 },
    });
    await waitFor(() => expect(getCycles).toHaveBeenCalledTimes(1));
    rerender({ e: 13 });
    await waitFor(() => expect(getCycles).toHaveBeenCalledTimes(2));
  });

  it('falls back to null when the request fails', async () => {
    h.fail = true;
    const { result } = renderHook(() => useCyclicNodeIds(true, 1, 1));
    await waitFor(() => expect(h.failCalls).toBe(1));
    expect(result.current).toBeNull();
  });
});
