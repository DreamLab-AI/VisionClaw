import { describe, it, expect } from 'vitest';
import { stepAgentOffset, AGENT_NUDGE_SPEED, AGENT_RELAX_SPEED, AGENT_RELAX_SNAP } from '../agentNudge';

const base: [number, number, number] = [10, 0, 0];

describe('stepAgentOffset (desktop agent momentum nudge)', () => {
  it('a working agent converges on its target node, as before', () => {
    const off = new Float32Array(3);
    stepAgentOffset(off, 0, base, [110, 0, 0], 'working');
    expect(off[0]).toBeCloseTo(100 * AGENT_NUDGE_SPEED);
    for (let i = 0; i < 400; i++) stepAgentOffset(off, 0, base, [110, 0, 0], 'working');
    expect(base[0] + off[0]).toBeCloseTo(110, 1);
  });

  it('a finished agent never drifts outward: with no offset it stays exactly where the server has it (owner decision)', () => {
    const off = new Float32Array(3);
    for (let i = 0; i < 1000; i++) stepAgentOffset(off, 0, base, null, 'done');
    expect(Array.from(off)).toEqual([0, 0, 0]);
  });

  it('a done agent relaxes back to its server position (ADR-2135)', () => {
    const off = new Float32Array([50, -20, 8]);
    let prev = Infinity;
    for (let i = 0; i < 300; i++) {
      stepAgentOffset(off, 0, base, null, 'done');
      const mag = Math.hypot(off[0], off[1], off[2]);
      expect(mag).toBeLessThan(prev); // monotone: no overshoot, no oscillation
      prev = mag;
    }
    expect(prev).toBeLessThan(0.01);
    const one = new Float32Array([50, 0, 0]);
    stepAgentOffset(one, 0, base, null, 'done');
    expect(one[0]).toBeCloseTo(50 * (1 - AGENT_RELAX_SPEED));
  });

  it('a relaxing offset snaps to exactly zero below AGENT_RELAX_SNAP, so the tail ends', () => {
    const off = new Float32Array([AGENT_RELAX_SNAP * 0.9, 0, 0]);
    stepAgentOffset(off, 0, base, null, 'done');
    expect(Array.from(off)).toEqual([0, 0, 0]);
    const above = new Float32Array([AGENT_RELAX_SNAP * 2, 0, 0]);
    stepAgentOffset(above, 0, base, null, 'done');
    expect(above[0]).toBeGreaterThan(0);
  });

  it('an agent with no work entry relaxes too', () => {
    const a = new Float32Array([5, 0, 0]);
    stepAgentOffset(a, 0, base, null, null);
    expect(a[0]).toBeLessThan(5);
  });

  it('writes only its own slot', () => {
    const off = new Float32Array([1, 1, 1, 7, 7, 7]);
    stepAgentOffset(off, 3, base, [20, 0, 0], 'working');
    expect(Array.from(off.subarray(0, 3))).toEqual([1, 1, 1]);
  });
});
