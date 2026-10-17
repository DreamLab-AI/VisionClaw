import { describe, expect, it } from 'vitest';
import { CONTEXT_ALPHA, contextStyle, regionZoom, selectTier, springFactor, streamedWeight, TIER_DISTANCE, tierWeights, zoomFromDistance } from '../scene/lod';

describe('tier selection by distance', () => {
  const R = 500;
  it('maps far/mid/near to l0/l1/l2', () => {
    expect(selectTier(R * 3, R)).toBe('l0');
    expect(selectTier(R * 1, R)).toBe('l1');
    expect(selectTier(R * 0.2, R)).toBe('l2');
  });
  it('switches exactly at the boundaries without history', () => {
    expect(selectTier(R * TIER_DISTANCE.l0 + 1, R)).toBe('l0');
    expect(selectTier(R * TIER_DISTANCE.l0 - 1, R)).toBe('l1');
    expect(selectTier(R * TIER_DISTANCE.l1 - 1, R)).toBe('l2');
  });
  it('holds the previous tier inside the hysteresis band', () => {
    const justInside = R * TIER_DISTANCE.l0 * 0.97;
    expect(selectTier(justInside, R, 'l0')).toBe('l0');
    expect(selectTier(justInside, R, 'l1')).toBe('l1');
    expect(selectTier(R * TIER_DISTANCE.l0 * 0.8, R, 'l0')).toBe('l1');
    expect(selectTier(R * TIER_DISTANCE.l1 * 1.03, R, 'l2')).toBe('l2');
    expect(selectTier(R * TIER_DISTANCE.l1 * 1.2, R, 'l2')).toBe('l1');
    expect(selectTier(R * TIER_DISTANCE.l0 * 1.03, R, 'l1')).toBe('l1');
    expect(selectTier(R * TIER_DISTANCE.l1 * 0.97, R, 'l1')).toBe('l1');
  });
  it('zoomFromDistance is monotonic 2 → 0 as the camera retreats', () => {
    let prev = Infinity;
    for (let x = 0.1; x < 4; x += 0.05) {
      const z = zoomFromDistance(R * x, R);
      expect(z).toBeLessThanOrEqual(prev + 1e-9);
      expect(z).toBeGreaterThanOrEqual(0);
      expect(z).toBeLessThanOrEqual(2);
      prev = z;
    }
    expect(zoomFromDistance(R * 0.1, R)).toBe(2);
    expect(zoomFromDistance(R * 5, R)).toBe(0);
  });
});

describe('semantic zoom cross-fade', () => {
  it('tier weights fade the aggregate out as members fade in', () => {
    expect(tierWeights(0)).toEqual([1, 0, 0]);
    expect(tierWeights(0.5)).toEqual([0.5, 0.5, 0]);
    expect(tierWeights(1)).toEqual([0, 1, 0]);
    expect(tierWeights(1.5)).toEqual([0, 1, 0.5]);
    expect(tierWeights(2)).toEqual([0, 1, 1]);
    expect(tierWeights(-1)).toEqual([1, 0, 0]);
  });
  it('region zoom rises with projected size and saturates', () => {
    expect(regionZoom(10)).toBe(0);
    expect(regionZoom(200)).toBe(1);
    expect(regionZoom(5000)).toBe(2);
    expect(regionZoom(90)).toBeGreaterThan(0);
    expect(regionZoom(90)).toBeLessThan(1);
  });
  it('spring factor is frame-rate independent', () => {
    const one = springFactor(1 / 30, 0.1);
    const two = 1 - (1 - springFactor(1 / 60, 0.1)) ** 2;
    expect(one).toBeCloseTo(two, 10);
    expect(springFactor(0.1, 0)).toBe(1);
  });
});

describe('zoom clamped to the streamed View', () => {
  it('never opens a region past what the server streams', () => {
    // Streaming L0 only: aggregates stay whole however close the camera is; files collapse.
    expect(streamedWeight(0, 2, 0, false)).toBe(1);
    expect(streamedWeight(1, 2, 0, false)).toBe(0);
    // Streaming L1: files open with zoom; symbols only from the focused file.
    expect(streamedWeight(0, 1, 1, false)).toBe(0);
    expect(streamedWeight(1, 0.5, 1, false)).toBe(0.5);
    expect(streamedWeight(2, 2, 1, false)).toBe(0);
    expect(streamedWeight(2, 2, 1, true)).toBe(1);
    // Streaming L2: plain cross-fade.
    expect(streamedWeight(2, 1.5, 2, false)).toBe(0.5);
    expect(streamedWeight(0, 0, 2, false)).toBe(1);
  });
});

describe('focus+context alpha', () => {
  it('is full without a focus', () => {
    expect(contextStyle(false, undefined, 2)).toEqual({ alpha: 1, desaturate: false });
  });
  it('keeps the k-hop neighbourhood full and dims the rest to 15% desaturated', () => {
    expect(contextStyle(true, 0, 2)).toEqual({ alpha: 1, desaturate: false });
    expect(contextStyle(true, 2, 2)).toEqual({ alpha: 1, desaturate: false });
    expect(contextStyle(true, 3, 2)).toEqual({ alpha: CONTEXT_ALPHA, desaturate: true });
    expect(contextStyle(true, undefined, 2)).toEqual({ alpha: 0.15, desaturate: true });
  });
});
