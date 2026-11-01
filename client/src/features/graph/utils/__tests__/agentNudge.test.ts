import { describe, it, expect } from 'vitest';
import {
  stepAgentOffset, newAgentFinish, AGENT_NUDGE_SPEED, AGENT_RELAX_SPEED, AGENT_RELAX_SNAP,
  AGENT_EDGE_SECONDS, AGENT_EDGE_SNAP, type SceneEdge,
} from '../agentNudge';

const base: [number, number, number] = [10, 0, 0];
const edge: SceneEdge = { centre: [0, 0, 0], radius: 100 };
const dist = (off: Float32Array, e: SceneEdge = edge) =>
  Math.hypot(base[0] + off[0] - e.centre[0], base[1] + off[1] - e.centre[1], base[2] + off[2] - e.centre[2]);
const ms = (s: number) => s * 1000;

/** Run a done agent for `seconds` at `fps`; returns the distance from the centre at each frame. */
function runDone(fps: number, seconds: number, e: SceneEdge = edge, off = new Float32Array(3)) {
  const finish = newAgentFinish();
  const out: number[] = [];
  const frames = Math.round(seconds * fps);
  for (let f = 0; f <= frames; f++) {
    stepAgentOffset(off, 0, base, null, 'done', e, finish, ms(f / fps));
    out.push(dist(off, e));
  }
  return { off, out, finish };
}

describe('stepAgentOffset (desktop agent momentum nudge)', () => {
  it('a working agent converges on its target node, as before', () => {
    const off = new Float32Array(3);
    stepAgentOffset(off, 0, base, [110, 0, 0], 'working', edge, newAgentFinish(), 0);
    expect(off[0]).toBeCloseTo(100 * AGENT_NUDGE_SPEED);
    for (let i = 0; i < 400; i++) stepAgentOffset(off, 0, base, [110, 0, 0], 'working', edge, newAgentFinish(), 0);
    expect(base[0] + off[0]).toBeCloseTo(110, 1);
  });

  for (const fps of [60, 120]) {
    it(`a done agent reaches the edge in ${AGENT_EDGE_SECONDS} s at ${fps} fps, easing out, never past it`, () => {
      const { out, off } = runDone(fps, AGENT_EDGE_SECONDS + 2);
      const at = (sec: number) => out[Math.round(sec * fps)];
      expect(out[0]).toBeCloseTo(10, 4); // starts where it finished
      for (let i = 1; i < out.length; i++) {
        expect(out[i]).toBeGreaterThanOrEqual(out[i - 1] - 1e-4); // only outward
        expect(out[i]).toBeLessThanOrEqual(edge.radius + 1e-4); // never past the edge
      }
      // easing out: it covers more in the first half than in the second, and it is still short at 20 s
      expect(at(15) - at(0)).toBeGreaterThan(at(30) - at(15));
      expect(edge.radius - at(20)).toBeGreaterThan(1);
      // arrived on time, within a frame, and holds bit-for-bit afterwards
      expect(Math.abs(at(AGENT_EDGE_SECONDS) - edge.radius)).toBeLessThan(AGENT_EDGE_SNAP * 2);
      const held = Array.from(off);
      const finish = newAgentFinish();
      finish.since = 0;
      finish.r0 = 10;
      stepAgentOffset(off, 0, base, null, 'done', edge, finish, ms(AGENT_EDGE_SECONDS + 5));
      expect(Array.from(off)).toEqual(held);
    });
  }

  it('the 30 s are measured from the finish, not from the start of the session', () => {
    const off = new Float32Array(3);
    const finish = newAgentFinish();
    stepAgentOffset(off, 0, base, [60, 0, 0], 'working', edge, finish, 5000);
    expect(finish.since).toBe(-1);
    stepAgentOffset(off, 0, base, null, 'done', edge, finish, 5016);
    expect(finish.since).toBe(5016);
    const start = dist(off);
    stepAgentOffset(off, 0, base, null, 'done', edge, finish, 5016 + 15000);
    const half = dist(off);
    expect(half).toBeGreaterThan(start + (edge.radius - start) * 0.8); // easeOutCubic(0.5) = 0.875
    expect(half).toBeLessThan(edge.radius);
    // working again resets the clock for its next finish
    stepAgentOffset(off, 0, base, [60, 0, 0], 'working', edge, finish, 21000);
    expect(finish.since).toBe(-1);
  });

  it('a done agent beyond the edge is put straight back onto it, and never further out', () => {
    const tight: SceneEdge = { centre: [0, 0, 0], radius: 10.5 };
    const off = new Float32Array([5, 0, 0]); // 15 from the centre, edge at 10.5
    const finish = newAgentFinish();
    stepAgentOffset(off, 0, base, null, 'done', tight, finish, 0);
    expect(dist(off, tight)).toBeCloseTo(tight.radius, 4);
    const held = Array.from(off);
    for (let f = 1; f < 5; f++) stepAgentOffset(off, 0, base, null, 'done', tight, finish, ms(f));
    expect(dist(off, tight)).toBeLessThanOrEqual(tight.radius + 1e-4);
    expect(Array.from(off)).toEqual(held);
  });

  it('a done agent with no known scene edge holds still', () => {
    const off = new Float32Array([1, 2, 3]);
    stepAgentOffset(off, 0, base, null, 'done', null, newAgentFinish(), 0);
    expect(Array.from(off)).toEqual([1, 2, 3]);
  });

  it('an agent with no work on record relaxes back to its server position (ADR-2135), monotonically', () => {
    const off = new Float32Array([50, -20, 8]);
    let prev = Infinity;
    for (let i = 0; i < 300; i++) {
      stepAgentOffset(off, 0, base, null, null, edge, newAgentFinish(), 0);
      const mag = Math.hypot(off[0], off[1], off[2]);
      expect(mag).toBeLessThan(prev);
      prev = mag;
    }
    expect(prev).toBeLessThan(0.01);
    const one = new Float32Array([50, 0, 0]);
    stepAgentOffset(one, 0, base, null, null, edge, newAgentFinish(), 0);
    expect(one[0]).toBeCloseTo(50 * (1 - AGENT_RELAX_SPEED));
  });

  it('a relaxing offset snaps to exactly zero below AGENT_RELAX_SNAP, so the tail ends', () => {
    const off = new Float32Array([AGENT_RELAX_SNAP * 0.9, 0, 0]);
    stepAgentOffset(off, 0, base, null, null, edge, newAgentFinish(), 0);
    expect(Array.from(off)).toEqual([0, 0, 0]);
  });

  it('writes only its own slot', () => {
    const off = new Float32Array([1, 1, 1, 7, 7, 7]);
    stepAgentOffset(off, 3, base, [20, 0, 0], 'working', edge, newAgentFinish(), 0);
    expect(Array.from(off.subarray(0, 3))).toEqual([1, 1, 1]);
  });
});
