import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

const fetchMock = vi.hoisted(() => vi.fn());
vi.mock('../communityLabels', async (orig) => ({ ...(await orig<typeof import('../communityLabels')>()), fetchCommunityLabels: fetchMock }));

import { useCommunityLabels } from '../useCommunityLabels';

const body = (generation: number) => ({ generation, communities: [] });

describe('useCommunityLabels', () => {
  beforeEach(() => { vi.useFakeTimers(); fetchMock.mockReset(); });
  afterEach(() => { vi.useRealTimers(); });

  it('applies a slow response even though the component re-renders with the same signature meanwhile', async () => {
    let resolve!: (v: unknown) => void;
    fetchMock.mockReturnValueOnce(new Promise((r) => { resolve = r; }));
    const { result, rerender } = renderHook(({ sig }) => useCommunityLabels(true, sig), { initialProps: { sig: '1:4' } });
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    for (let i = 0; i < 5; i++) rerender({ sig: '1:4' }); // each ~0.5 s tick re-renders
    await act(async () => { resolve(body(2)); await Promise.resolve(); });
    expect(result.current?.generation).toBe(2);
  });

  it('refetches when the partition signature changes, but not more than once per minimum interval', async () => {
    fetchMock.mockResolvedValue(body(1));
    const { rerender } = renderHook(({ sig }) => useCommunityLabels(true, sig), { initialProps: { sig: '1:4' } });
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    rerender({ sig: '1:5' });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(fetchMock).toHaveBeenCalledTimes(1); // throttled
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('keeps the newer generation when an older response arrives late', async () => {
    fetchMock.mockResolvedValueOnce(body(5)).mockResolvedValueOnce(body(4));
    const { result, rerender } = renderHook(({ sig }) => useCommunityLabels(true, sig), { initialProps: { sig: 'a' } });
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    rerender({ sig: 'b' });
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(result.current?.generation).toBe(5);
  });

  it('does nothing while disabled or before any partition exists', async () => {
    renderHook(() => useCommunityLabels(false, '1:4'));
    renderHook(() => useCommunityLabels(true, ''));
    await act(async () => { await vi.advanceTimersByTimeAsync(120000); });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
