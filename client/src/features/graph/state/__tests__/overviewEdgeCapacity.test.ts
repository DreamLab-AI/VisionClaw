import { describe, it, expect } from 'vitest';
import { normaliseEdgeCapacity, resolveOverviewEdgeCapacity } from '../overviewEdgeCapacity';

describe('overview edge capacity (ADR-2181 D4)', () => {
  it("'auto' rounds the edge count up to a power of two and never truncates", () => {
    expect(resolveOverviewEdgeCapacity('auto', 147_048)).toEqual({ capacity: 262_144, drawn: 147_048, truncated: false });
    expect(resolveOverviewEdgeCapacity('auto', 0).capacity).toBe(1);
  });

  it('a numeric budget below the count truncates visibly', () => {
    expect(resolveOverviewEdgeCapacity(65_536, 147_048)).toEqual({ capacity: 65_536, drawn: 65_536, truncated: true });
    // a budget larger than needed does not over-allocate
    expect(resolveOverviewEdgeCapacity(1_000_000, 100).capacity).toBe(128);
  });

  it('accepts numeric strings from a settings control; anything else is auto', () => {
    expect(normaliseEdgeCapacity('131072')).toBe(131_072);
    expect(normaliseEdgeCapacity('auto')).toBe('auto');
    expect(normaliseEdgeCapacity('')).toBe('auto');
    expect(normaliseEdgeCapacity(-5)).toBe('auto');
    expect(normaliseEdgeCapacity(undefined)).toBe('auto');
  });
});
