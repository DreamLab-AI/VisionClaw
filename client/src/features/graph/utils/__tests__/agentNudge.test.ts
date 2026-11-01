import { describe, it, expect } from 'vitest';
import {
  stepAgentOffset, AGENT_NUDGE_SPEED, AGENT_RELAX_SPEED, AGENT_RELAX_SNAP, AGENT_DRIFT_SPEED,
  AGENT_EDGE_SNAP, type SceneEdge,
} from '../agentNudge';

const base: [number, number, number] = [10, 0, 0];
const edge: SceneEdge = { centre: [0, 0, 0], radius: 10.5 };
const dist = (off: Float32Array, e: SceneEdge = edge) =>
  Math.hypot(base[0] + off[0] - e.centre[0], base[1] + off[1] - e.centre[1], base[2] + off[2] - e.centre[2]);

describe('stepAgentOffset (desktop agent momentum nudge)', () => {
  it('a working agent converges on its target node, as before', () => {
    const off = new Float32Array(3);
    stepAgentOffset(off, 0, base, [110, 0, 0], 'working', edge);
    expect(off[0]).toBeCloseTo(100 * AGENT_NUDGE_SPEED);
    for (let i = 0; i < 400; i++) stepAgentOffset(off, 0, base, [110, 0, 0], 'working', edge);
    expect(base[0] + off[0]).toBeCloseTo(110, 1);
  });

  it('a done agent drifts outward at the long-standing speed', () => {
    const off = new Float32Array([0, 0, 0]);
    stepAgentOffset(off, 0, base, null, 'done', edge);
    expect(off[0]).toBeCloseTo(AGENT_DRIFT_SPEED);
  });

  it('a done agent drifts to the scene edge, never past it, and then holds still bit-for-bit', () => {
    const off = new Float32Array(3);
    let prev = dist(off);
    let settled = -1;
    let last = Array.from(off);
    for (let i = 0; i < 2000; i++) {
      stepAgentOffset(off, 0, base, null, 'done', edge);
      const d = dist(off);
      expect(d).toBeGreaterThanOrEqual(prev - 1e-6); // only outward
      expect(d).toBeLessThanOrEqual(edge.radius + 1e-4); // never past the edge
      prev = d;
      const cur = Array.from(off);
      if (cur.every((v, k) => v === last[k])) { if (settled < 0) settled = i; } else settled = -1;
      last = cur;
    }
    expect(settled).toBeGreaterThan(0);
    expect(Math.abs(prev - edge.radius)).toBeLessThan(AGENT_EDGE_SNAP * 2);
  });

  it('a done agent beyond the edge is put straight back onto it, and never further out', () => {
    const off = new Float32Array([5, 0, 0]); // 15 from the centre, edge at 10.5
    stepAgentOffset(off, 0, base, null, 'done', edge);
    expect(dist(off)).toBeCloseTo(edge.radius, 4);
    const held = Array.from(off);
    stepAgentOffset(off, 0, base, null, 'done', edge);
    expect(Array.from(off)).toEqual(held);
  });

  it('a done agent with no known scene edge holds still', () => {
    const off = new Float32Array([1, 2, 3]);
    stepAgentOffset(off, 0, base, null, 'done', null);
    expect(Array.from(off)).toEqual([1, 2, 3]);
  });

  it('an agent with no work on record relaxes back to its server position (ADR-2135), monotonically', () => {
    const off = new Float32Array([50, -20, 8]);
    let prev = Infinity;
    for (let i = 0; i < 300; i++) {
      stepAgentOffset(off, 0, base, null, null, edge);
      const mag = Math.hypot(off[0], off[1], off[2]);
      expect(mag).toBeLessThan(prev);
      prev = mag;
    }
    expect(prev).toBeLessThan(0.01);
    const one = new Float32Array([50, 0, 0]);
    stepAgentOffset(one, 0, base, null, null, edge);
    expect(one[0]).toBeCloseTo(50 * (1 - AGENT_RELAX_SPEED));
  });

  it('a relaxing offset snaps to exactly zero below AGENT_RELAX_SNAP, so the tail ends', () => {
    const off = new Float32Array([AGENT_RELAX_SNAP * 0.9, 0, 0]);
    stepAgentOffset(off, 0, base, null, null, edge);
    expect(Array.from(off)).toEqual([0, 0, 0]);
  });

  it('writes only its own slot', () => {
    const off = new Float32Array([1, 1, 1, 7, 7, 7]);
    stepAgentOffset(off, 3, base, [20, 0, 0], 'working', edge);
    expect(Array.from(off.subarray(0, 3))).toEqual([1, 1, 1]);
  });
});
