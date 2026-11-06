import { describe, it, expect } from 'vitest';
import { selectGemLod, createGemLodScratch, cameraMoved } from '../gemLod';

// 10 nodes on the x axis at x = 0, 10, …, 90; local i ↔ render index 9 − i
const positions = new Float32Array(30);
for (let r = 0; r < 10; r++) positions[r * 3] = r * 10;
const renderIndex = Int32Array.from({ length: 10 }, (_, i) => 9 - i);

describe('selectGemLod (ADR-2182 D2)', () => {
  it('draws gems only within gemLodDistance, by render index position', () => {
    const out = new Uint8Array(10);
    const n = selectGemLod({ positions, renderIndex, count: 10, camX: 0, camY: 0, camZ: 0, lodDistance: 25, budget: 100 }, out, createGemLodScratch());
    expect(n).toBe(3); // x = 0, 10, 20 → render 0,1,2 → local 9,8,7
    expect(Array.from(out)).toEqual([0, 0, 0, 0, 0, 0, 0, 1, 1, 1]);
  });

  it('caps nearest-first at the budget', () => {
    const out = new Uint8Array(10);
    const n = selectGemLod({ positions, renderIndex, count: 10, camX: 95, camY: 0, camZ: 0, lodDistance: 1000, budget: 2 }, out, createGemLodScratch());
    expect(n).toBe(2);
    expect(out[0]).toBe(1); // render 9 at x=90
    expect(out[1]).toBe(1); // render 8 at x=80
    expect(out.reduce((a, b) => a + b, 0)).toBe(2);
  });

  it('forced nodes (selected, hovered) are gems at any distance and outside the budget', () => {
    const out = new Uint8Array(10);
    const forced = new Uint8Array(10); forced[0] = 1;
    const n = selectGemLod({ positions, renderIndex, count: 10, camX: 0, camY: 0, camZ: 0, lodDistance: 5, budget: 1, forced }, out, createGemLodScratch());
    expect(n).toBe(2);
    expect(out[0]).toBe(1);
    expect(out[9]).toBe(1);
  });

  it('at overview distance there are no gems (LOD switch)', () => {
    const out = new Uint8Array(10).fill(1);
    expect(selectGemLod({ positions, renderIndex, count: 10, camX: 0, camY: 0, camZ: 900, lodDistance: 90, budget: 100 }, out, createGemLodScratch())).toBe(0);
    expect(Array.from(out)).toEqual(new Array(10).fill(0));
  });
});

describe('cameraMoved', () => {
  it('fires on first call and on moves past epsilon only', () => {
    const p = { x: 0, y: 0, z: 0, init: false };
    expect(cameraMoved(p, 0, 0, 0, 1)).toBe(true);
    expect(cameraMoved(p, 0.5, 0, 0, 1)).toBe(false);
    expect(cameraMoved(p, 2, 0, 0, 1)).toBe(true);
  });
});
