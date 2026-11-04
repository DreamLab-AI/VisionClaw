import { describe, expect, it } from 'vitest';
import { BACKDROP_RENDER_ORDER, boundsSphere, framedPlacement, placementFor } from '../fit';

const ISS = { min: [-64, -36, -26] as [number, number, number], max: [64, 35, 43] as [number, number, number] };
const GRAPH = { centre: [10, 5, -3] as [number, number, number], radius: 80 };

describe('fit rules', () => {
  it('lands the bounds centre on the target centre, uniformly scaled', () => {
    const p = placementFor('entered', ISS, GRAPH);
    const s = boundsSphere(ISS);
    expect(p.scale * s.radius).toBeCloseTo(GRAPH.radius * 3, 6);
    for (let k = 0; k < 3; k++) expect(p.position[k] + s.centre[k] * p.scale).toBeCloseTo(GRAPH.centre[k], 6);
  });

  it('keeps a framed volume clear of the graph sphere', () => {
    const p = framedPlacement(ISS, GRAPH);
    const s = boundsSphere(ISS);
    const centre = [0, 1, 2].map((k) => p.position[k] + s.centre[k] * p.scale);
    const gap = Math.hypot(centre[0] - GRAPH.centre[0], centre[1] - GRAPH.centre[1], centre[2] - GRAPH.centre[2]);
    expect(gap).toBeGreaterThan(GRAPH.radius + p.scale * s.radius);
  });

  it('draws an entered cloud below every graph layer, a framed one at the default order', () => {
    expect(placementFor('entered', ISS, GRAPH).renderOrder).toBe(BACKDROP_RENDER_ORDER);
    expect(BACKDROP_RENDER_ORDER).toBeLessThan(0);
    expect(placementFor('framed', ISS, GRAPH).renderOrder).toBe(0);
  });

  it('falls back to a default graph before one exists', () => {
    const p = placementFor('framed', ISS, null);
    expect(Number.isFinite(p.scale) && p.scale > 0).toBe(true);
  });
});
