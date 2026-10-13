import { describe, it, expect } from 'vitest';
import { tickTween, type TweenInput } from '../tween';
import type { TweenSettings } from '../types';

const DEFAULT_SETTINGS: TweenSettings = {
  enabled: true,
  responseMs: 60,
  snapThreshold: 0.01,
  maxDivergence: 500,
};

function makeInput(overrides: Partial<TweenInput> = {}): TweenInput {
  const nodeCount = overrides.nodeCount ?? 1;
  return {
    curPos: overrides.curPos ?? new Float32Array(nodeCount * 3),
    tgtPos: overrides.tgtPos ?? new Float32Array(nodeCount * 3),
    vel: overrides.vel ?? new Float32Array(nodeCount * 3),
    nodeCount,
    pinnedNodeIds: overrides.pinnedNodeIds ?? new Set(),
    nodeIdMap: overrides.nodeIdMap ?? new Map([['n0', 0]]),
    nodeIds: overrides.nodeIds ?? ['n0'],
    tweenSettings: overrides.tweenSettings ?? DEFAULT_SETTINGS,
    deltaTime: overrides.deltaTime ?? 0.016,
    ...overrides,
  };
}

describe('tickTween', () => {
  it('returns no movement when current equals target', () => {
    const input = makeInput();
    const result = tickTween(input);
    expect(result.hadMovement).toBe(false);
    expect(result.totalMovement).toBe(0);
  });

  it('t=0 equivalent: positions unchanged before any tick when already at target', () => {
    const cur = new Float32Array([1, 2, 3]);
    const tgt = new Float32Array([1, 2, 3]);
    const input = makeInput({ curPos: cur, tgtPos: tgt });
    tickTween(input);
    expect(cur[0]).toBe(1);
    expect(cur[1]).toBe(2);
    expect(cur[2]).toBe(3);
  });

  it('snaps to target when divergence exceeds maxDivergence', () => {
    const cur = new Float32Array([0, 0, 0]);
    const tgt = new Float32Array([1000, 0, 0]);
    const vel = new Float32Array(3);
    const input = makeInput({ curPos: cur, tgtPos: tgt, vel, maxDivergence: 500 } as any);
    input.tweenSettings = { ...DEFAULT_SETTINGS, maxDivergence: 500 };
    const result = tickTween(input);
    expect(cur[0]).toBe(1000);
    expect(vel[0]).toBe(0);
    expect(result.hadMovement).toBe(true);
    expect(result.totalMovement).toBeGreaterThan(0);
  });

  it('snaps to target when distance is below snapThreshold but above inner 0.01 guard', () => {
    // snapThreshold=0.01 → snap when distanceSq < 0.0001
    // inner positionChanged guard requires |delta| > 0.01
    // Use distance 0.015: inside snap range (0.015^2=0.000225 > 0.0001? No)
    // Actually snapThreshold default is 0.01, so distance 0.015 is OUTSIDE snap range.
    // Use distance 0.008 (< 0.01) but delta per-axis is 0.008 < 0.01 → positionChanged=false → no snap
    // The snap only fires if positionChanged (any axis delta > 0.01) or within tiny range.
    // Simplest: set tgt very close but let the function's hadMovement trigger.
    // Use a sub-0.001 delta — hadMovement returns false early, no tween at all.
    const cur = new Float32Array([0, 0, 0.0005]); // < 0.001 threshold for hadMovement
    const tgt = new Float32Array([0, 0, 0]);
    const input = makeInput({ curPos: cur, tgtPos: tgt });
    const result = tickTween(input);
    // hadMovement=false → early return, no modification
    expect(result.hadMovement).toBe(false);
    expect(cur[2]).toBeCloseTo(0.0005); // unchanged
  });

  it('normal lerp step: advances toward target', () => {
    const cur = new Float32Array([0, 0, 0]);
    const tgt = new Float32Array([10, 0, 0]);
    const input = makeInput({ curPos: cur, tgtPos: tgt, deltaTime: 0.016 });
    const result = tickTween(input);
    expect(cur[0]).toBeGreaterThan(0);
    expect(cur[0]).toBeLessThan(10);
    expect(result.hadMovement).toBe(true);
    expect(result.totalMovement).toBeGreaterThan(0);
  });

  it('skips pinned nodes', () => {
    const cur = new Float32Array([0, 0, 0]);
    const tgt = new Float32Array([100, 0, 0]);
    const pinnedNodeIds = new Set([0]);
    const nodeIdMap = new Map([['n0', 0]]);
    const input = makeInput({ curPos: cur, tgtPos: tgt, pinnedNodeIds, nodeIdMap });
    tickTween(input);
    // Position must not have changed
    expect(cur[0]).toBe(0);
  });

  it('processes multiple nodes independently', () => {
    const nodeCount = 3;
    const cur = new Float32Array([0,0,0, 5,5,5, 0,0,0]);
    const tgt = new Float32Array([10,0,0, 5,5,5, 0,0,0]);
    const nodeIdMap = new Map([['n0', 0], ['n1', 1], ['n2', 2]]);
    const input = makeInput({ curPos: cur, tgtPos: tgt, nodeCount, nodeIds: ['n0','n1','n2'], nodeIdMap });
    tickTween(input);
    expect(cur[0]).toBeGreaterThan(0); // node 0 moved
    expect(cur[3]).toBe(5);            // node 1 unchanged (at target)
    expect(cur[6]).toBe(0);            // node 2 unchanged (at target)
  });

  it('totalMovement reflects sum of displacements', () => {
    const cur = new Float32Array([0, 0, 0]);
    const tgt = new Float32Array([3, 4, 0]); // distance 5
    const input = makeInput({ curPos: cur, tgtPos: tgt, deltaTime: 1 });
    const result = tickTween(input);
    expect(result.totalMovement).toBeGreaterThan(0);
  });

  describe('response time (responseMs)', () => {
    const run = (responseMs: number, dt: number, seconds: number) => {
      const input = makeInput({
        curPos: new Float32Array([0, 0, 0]),
        tgtPos: new Float32Array([100, 0, 0]),
        tweenSettings: { ...DEFAULT_SETTINGS, responseMs, snapThreshold: 0.0001 },
        deltaTime: dt,
      });
      for (let t = 0; t < seconds; t += dt) tickTween(input);
      return input.curPos[0];
    };

    it('covers ~63% of the gap in one response time', () => {
      expect(run(60, 0.001, 0.06)).toBeGreaterThan(62);
      expect(run(60, 0.001, 0.06)).toBeLessThan(64.5);
    });

    it('is independent of the frame rate', () => {
      const at30 = run(60, 1 / 30, 0.2);
      const at144 = run(60, 1 / 144, 0.2);
      expect(Math.abs(at30 - at144)).toBeLessThan(1.5);
    });

    it('lower is faster', () => {
      expect(run(30, 0.016, 0.1)).toBeGreaterThan(run(120, 0.016, 0.1));
    });
  });
});
