import { describe, it, expect } from 'vitest';
import { spriteCapacityToMount } from '../OverviewNodes';
import { createOverviewNodeGeometry } from '../../../../rendering/materials/OverviewNodeMaterial';

// Regression for the live fault of 2026-11-07: 9,595 knowledge nodes against the
// initial 4,096-sprite geometry drew 0 sprites, so the gems never hid and the
// overview showed a solid white disc of lit gems over the edges.
//
// The grow decision must come from the geometry that is actually mounted. The
// old code compared against React's `capacity` state inside useFrame; useFrame
// runs every frame while setCapacity is batched until the next render, so the
// stale closure value re-queued the same grow and returned before the fill
// loop on every frame. The mesh reached the right size; instanceCount never
// left 0.
describe('spriteCapacityToMount', () => {
  it('grows to the next power of two when the population exceeds the mounted geometry', () => {
    expect(spriteCapacityToMount(9595, 4096)).toBe(16384);
  });

  it('does not grow once a geometry large enough is mounted, so the fill loop runs', () => {
    // This is the frame after the grow has rendered: 16,384 mounted, 9,595 nodes.
    // The old rule would still have compared against stale state here.
    expect(spriteCapacityToMount(9595, 16384)).toBeNull();
  });

  it('fits exactly at the boundary and grows one past it', () => {
    expect(spriteCapacityToMount(4096, 4096)).toBeNull();
    expect(spriteCapacityToMount(4097, 4096)).toBe(8192);
  });

  it('reads the mounted size the way the frame loop does: from the aIndex attribute', () => {
    const geo = createOverviewNodeGeometry(4096);
    const mounted = geo.getAttribute('aIndex').count;
    expect(mounted).toBe(4096);
    expect(spriteCapacityToMount(9595, mounted)).toBe(16384);
    const grown = createOverviewNodeGeometry(16384);
    expect(spriteCapacityToMount(9595, grown.getAttribute('aIndex').count)).toBeNull();
  });
});
