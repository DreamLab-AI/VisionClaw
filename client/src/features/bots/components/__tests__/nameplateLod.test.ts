// W3D — agent nameplate LOD. Unit-tests the pure distance→tier classifier that
// declutters dense swarms: full 3-line nameplate up close, name-only mid-range,
// hidden far, with ±10% directional hysteresis and a near-field density cap.
// The function is imported from nameplateTier but exercised in
// isolation — no R3F canvas is mounted.

import { describe, it, expect } from 'vitest';
import { computeNameplateTier, type NameplateTierOpts, type NameplateTier } from '../../nameplateTier';

// Defaults mirror AgentNameplate: D1 = 40, D2 = 40 × 2.25 = 90, h = 0.1.
// ⇒ D1 band [36, 44), name band [44, 81), D2 band [81, 99).
const D1 = 40;
const D2 = 90;
const opts = (over: Partial<NameplateTierOpts> = {}): NameplateTierOpts => ({
  fullDistance: D1,
  nameDistance: D2,
  hysteresis: 0.1,
  ...over,
});

describe('computeNameplateTier — unambiguous distance zones', () => {
  it('is full well inside D1 (any prevTier)', () => {
    expect(computeNameplateTier(10, 'hidden', opts())).toBe('full');
    expect(computeNameplateTier(10, 'name', opts())).toBe('full');
    expect(computeNameplateTier(35.9, 'name', opts())).toBe('full');
  });

  it('is name in the clear mid band', () => {
    expect(computeNameplateTier(60, 'full', opts())).toBe('name');
    expect(computeNameplateTier(60, 'hidden', opts())).toBe('name');
    expect(computeNameplateTier(44, 'full', opts())).toBe('name');   // exactly d1Hi
    expect(computeNameplateTier(80.9, 'hidden', opts())).toBe('name');
  });

  it('is hidden well beyond D2 (any prevTier)', () => {
    expect(computeNameplateTier(100, 'name', opts())).toBe('hidden'); // clear of d2Hi≈99
    expect(computeNameplateTier(200, 'full', opts())).toBe('hidden');
  });
});

describe('computeNameplateTier — hysteresis holds tier on the boundary', () => {
  it('holds full vs name inside the D1 dead-band [36,44)', () => {
    // Same distance, opposite history → opposite tier: the dead-band never flips.
    expect(computeNameplateTier(40, 'full', opts())).toBe('full');
    expect(computeNameplateTier(40, 'name', opts())).toBe('name');
    expect(computeNameplateTier(36, 'full', opts())).toBe('full');
    expect(computeNameplateTier(43.9, 'name', opts())).toBe('name');
  });

  it('only promotes name→full once distance drops below d1·(1−h)=36', () => {
    expect(computeNameplateTier(36.5, 'name', opts())).toBe('name'); // still inside band
    expect(computeNameplateTier(35.9, 'name', opts())).toBe('full'); // crossed d1Lo
  });

  it('only demotes full→name once distance exceeds d1·(1+h)=44', () => {
    expect(computeNameplateTier(43.5, 'full', opts())).toBe('full'); // still inside band
    expect(computeNameplateTier(44.1, 'full', opts())).toBe('name'); // crossed d1Hi
  });

  it('holds name vs hidden inside the D2 dead-band [81,99)', () => {
    expect(computeNameplateTier(90, 'name', opts())).toBe('name');
    expect(computeNameplateTier(90, 'hidden', opts())).toBe('hidden');
  });

  it('only promotes hidden→name below d2·(1−h)=81 and demotes name→hidden above d2·(1+h)=99', () => {
    expect(computeNameplateTier(82, 'hidden', opts())).toBe('hidden'); // still inside band
    expect(computeNameplateTier(80.9, 'hidden', opts())).toBe('name'); // crossed d2Lo
    expect(computeNameplateTier(98, 'name', opts())).toBe('name');     // still inside band
    expect(computeNameplateTier(99.1, 'name', opts())).toBe('hidden'); // crossed d2Hi
  });

  it('does not flicker across a boundary-hugging camera jitter sweep', () => {
    // Camera parked at the D1 boundary, dithering ±5u without ever clearing a
    // dead-band edge, must never change the displayed tier.
    let tier: NameplateTier = 'name';
    for (const d of [40, 42, 38, 43, 37, 41, 39, 40]) {
      tier = computeNameplateTier(d, tier, opts());
      expect(tier).toBe('name');
    }
  });
});

describe('computeNameplateTier — priority overrides', () => {
  it('forceFull pins to full at any distance (queen / hovered / selected)', () => {
    expect(computeNameplateTier(500, 'hidden', opts({ forceFull: true }))).toBe('full');
    expect(computeNameplateTier(90, 'name', opts({ forceFull: true }))).toBe('full');
  });

  it('capName caps a would-be full at name (near-field density guard)', () => {
    expect(computeNameplateTier(10, 'full', opts({ capName: true }))).toBe('name');
  });

  it('capName never overrides forceFull (queen/hovered stay full when crowded)', () => {
    expect(computeNameplateTier(10, 'full', opts({ capName: true, forceFull: true }))).toBe('full');
  });

  it('capName leaves already-reduced tiers untouched', () => {
    expect(computeNameplateTier(60, 'name', opts({ capName: true }))).toBe('name');
    expect(computeNameplateTier(200, 'hidden', opts({ capName: true }))).toBe('hidden');
  });
});
