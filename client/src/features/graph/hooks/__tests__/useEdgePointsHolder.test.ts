import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useEdgePointsHolder } from '../useEdgePointsHolder';

describe('useEdgePointsHolder', () => {
  it('keeps the bulk array behind a stable ref and signals change with a number', () => {
    const { result } = renderHook(() => useEdgePointsHolder());
    const first = result.current;
    expect(first.ref.current).toEqual([]);
    expect(first.version).toBe(0);

    const big = new Array(6000).fill(1);
    act(() => first.set(big));
    expect(result.current.ref).toBe(first.ref);
    expect(result.current.ref.current).toBe(big);
    expect(result.current.version).toBe(1);
    expect(result.current.set).toBe(first.set);

    const bigger = new Array(12000).fill(2);
    act(() => result.current.set(bigger));
    expect(result.current.ref).toBe(first.ref);
    expect(result.current.ref.current).toBe(bigger);
    expect(result.current.version).toBe(2);
  });
});
