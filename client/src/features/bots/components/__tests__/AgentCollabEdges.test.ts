import { describe, it, expect } from 'vitest';
import {
  computeAvgTokenRate,
  computeEdgeOpacity,
  computeEdgeColor,
  isEdgeActive,
  shouldEdgePulse,
  computePulse,
  growEdgeCapacity,
  EDGE_INITIAL_CAPACITY,
} from '../AgentCollabEdges';

describe('AgentCollabEdges — computeAvgTokenRate', () => {
  it('averages two rates', () => {
    expect(computeAvgTokenRate(10, 30)).toBe(20);
  });

  it('treats missing/undefined rates as 0', () => {
    expect(computeAvgTokenRate(undefined, 40)).toBe(20);
    expect(computeAvgTokenRate(undefined, undefined)).toBe(0);
    expect(computeAvgTokenRate(0, 0)).toBe(0);
  });
});

describe('AgentCollabEdges — computeEdgeOpacity', () => {
  it('idle base is 0.3, active base is 0.8 (no token boost below threshold)', () => {
    expect(computeEdgeOpacity(false, 0)).toBe(0.3);
    expect(computeEdgeOpacity(true, 0)).toBe(0.8);
    // avg 10 is NOT > 10 → no boost
    expect(computeEdgeOpacity(false, 10)).toBe(0.3);
  });

  it('adds a token-rate boost above 10, capped at +0.4', () => {
    // avg 20 → boost min(20/50,0.4)=0.4 → 0.3+0.4=0.7
    expect(computeEdgeOpacity(false, 20)).toBeCloseTo(0.7, 6);
    // avg 15 → boost 15/50=0.3 → 0.3+0.3=0.6
    expect(computeEdgeOpacity(false, 15)).toBeCloseTo(0.6, 6);
  });

  it('caps total opacity at 1', () => {
    // active 0.8 + max boost 0.4 = 1.2 → clamped to 1
    expect(computeEdgeOpacity(true, 100)).toBe(1);
  });
});

describe('AgentCollabEdges — computeEdgeColor', () => {
  it('idle edges use the base colour', () => {
    expect(computeEdgeColor(false, 999, '#123456')).toBe('#123456');
  });

  it('active edges bucket by token rate', () => {
    expect(computeEdgeColor(true, 25, '#123456')).toBe('#E67E22'); // > 20
    expect(computeEdgeColor(true, 15, '#123456')).toBe('#3498DB'); // > 10
    expect(computeEdgeColor(true, 5, '#123456')).toBe('#2980B9');  // low
  });

  it('bucket boundaries are strict >', () => {
    expect(computeEdgeColor(true, 20, '#123456')).toBe('#3498DB'); // 20 not > 20
    expect(computeEdgeColor(true, 10, '#123456')).toBe('#2980B9'); // 10 not > 10
  });
});

describe('AgentCollabEdges — isEdgeActive', () => {
  it('is active within the 5 s window, inactive at/after it', () => {
    const now = 100_000;
    expect(isEdgeActive(now - 4999, now)).toBe(true);
    expect(isEdgeActive(now - 5000, now)).toBe(false);
    expect(isEdgeActive(now - 9000, now)).toBe(false);
    expect(isEdgeActive(now, now)).toBe(true);
  });
});

describe('AgentCollabEdges — shouldEdgePulse', () => {
  it('pulses on high token rate OR high message count', () => {
    expect(shouldEdgePulse(41, 0)).toBe(true);   // rate > 40
    expect(shouldEdgePulse(0, 201)).toBe(true);  // count > 200
    expect(shouldEdgePulse(40, 200)).toBe(false); // both at boundary, not >
    expect(shouldEdgePulse(5, 5)).toBe(false);
  });
});

describe('AgentCollabEdges — computePulse', () => {
  it('returns 1 when not pulsing, regardless of time', () => {
    expect(computePulse(false, 0)).toBe(1);
    expect(computePulse(false, 12.34)).toBe(1);
  });

  it('oscillates in [0.7, 1.3] around 1 when pulsing', () => {
    // sin(t*5)*0.3+1 — peak at sin=1, trough at sin=-1
    const peak = computePulse(true, Math.PI / 10); // t*5 = π/2 → sin=1
    const trough = computePulse(true, (3 * Math.PI) / 10); // t*5 = 3π/2 → sin=-1
    expect(peak).toBeCloseTo(1.3, 6);
    expect(trough).toBeCloseTo(0.7, 6);
    expect(computePulse(true, 0)).toBeCloseTo(1, 6); // sin(0)=0
  });
});

describe('AgentCollabEdges — growEdgeCapacity', () => {
  it('returns current when already sufficient', () => {
    expect(growEdgeCapacity(256, 100)).toBe(256);
    expect(growEdgeCapacity(256, 256)).toBe(256);
  });

  it('doubles until it covers the needed count', () => {
    expect(growEdgeCapacity(256, 257)).toBe(512);
    expect(growEdgeCapacity(256, 1000)).toBe(1024);
    expect(growEdgeCapacity(256, 2048)).toBe(2048);
    expect(growEdgeCapacity(256, 2049)).toBe(4096);
  });

  it('never returns below 1 for a degenerate current', () => {
    expect(growEdgeCapacity(0, 3)).toBe(4);
    expect(growEdgeCapacity(0, 0)).toBe(1);
  });

  it('initial capacity comfortably covers a typical swarm (342 edges → 512)', () => {
    expect(growEdgeCapacity(EDGE_INITIAL_CAPACITY, 342)).toBe(512);
  });
});
