import { describe, it, expect } from 'vitest';
import { stepAgentOffset, AGENT_NUDGE_SPEED, AGENT_DRIFT_SPEED, AGENT_RELAX_SPEED } from '../agentNudge';

const base: [number, number, number] = [10, 0, 0];
const centre: [number, number, number] = [0, 0, 0];

describe('stepAgentOffset (desktop agent momentum nudge)', () => {
  it('a working agent converges on its target node, as before', () => {
    const off = new Float32Array(3);
    stepAgentOffset(off, 0, base, [110, 0, 0], 'working', centre, false);
    expect(off[0]).toBeCloseTo(100 * AGENT_NUDGE_SPEED);
    for (let i = 0; i < 400; i++) stepAgentOffset(off, 0, base, [110, 0, 0], 'working', centre, true);
    expect(base[0] + off[0]).toBeCloseTo(110, 1);
  });

  it('merged layout: a done agent keeps its offset and drifts outward (unchanged behaviour)', () => {
    const off = new Float32Array([50, 0, 0]);
    stepAgentOffset(off, 0, base, null, 'done', centre, false);
    expect(off[0]).toBeCloseTo(50 + AGENT_DRIFT_SPEED);
  });

  it('separated layout: a done agent relaxes back to its server position (ADR-2135)', () => {
    const off = new Float32Array([50, -20, 8]);
    let prev = Infinity;
    for (let i = 0; i < 300; i++) {
      stepAgentOffset(off, 0, base, null, 'done', centre, true);
      const mag = Math.hypot(off[0], off[1], off[2]);
      expect(mag).toBeLessThan(prev); // monotone: no overshoot, no oscillation
      prev = mag;
    }
    expect(prev).toBeLessThan(0.01);
    const one = new Float32Array([50, 0, 0]);
    stepAgentOffset(one, 0, base, null, 'done', centre, true);
    expect(one[0]).toBeCloseTo(50 * (1 - AGENT_RELAX_SPEED));
  });

  it('separated layout: an agent with no work entry relaxes too; merged leaves it alone', () => {
    const a = new Float32Array([5, 0, 0]);
    stepAgentOffset(a, 0, base, null, null, centre, false);
    expect(a[0]).toBe(5);
    stepAgentOffset(a, 0, base, null, null, centre, true);
    expect(a[0]).toBeLessThan(5);
  });

  it('writes only its own slot', () => {
    const off = new Float32Array([1, 1, 1, 7, 7, 7]);
    stepAgentOffset(off, 3, base, [20, 0, 0], 'working', centre, false);
    expect(Array.from(off.subarray(0, 3))).toEqual([1, 1, 1]);
  });
});
