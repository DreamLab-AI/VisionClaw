import { describe, it, expect, beforeEach } from 'vitest';
import { fadeAlpha, fadeAlphaWith, numberOps, tslScalarOps, edgeWidthPx, selectDrawnEdges, WEIGHT_AT_MAX_WIDTH } from '../edgeStyle';
import {
  publishOverviewEdgeReport, getOverviewEdgeReport, resetOverviewEdgeReport, subscribeOverviewEdgeReport,
  overviewEdgeNoticeText,
} from '../overviewEdgeStore';
import { resolveOverviewEdgeCapacity } from '../../state/overviewEdgeCapacity';
import { DEFAULT_GRAPH_VIEW } from '../../../../api/settings/defaults';

const E = DEFAULT_GRAPH_VIEW.overviewEdges;

describe('fadeAlpha (ADR-2181 D2)', () => {
  it('is 1 up to fadeNearPx, minOpacity from fadeFarPx, linear between', () => {
    expect(fadeAlpha(0, E.fadeNearPx, E.fadeFarPx, E.minOpacity)).toBe(1);
    expect(fadeAlpha(50, 50, 150, 0.25)).toBe(1);
    expect(fadeAlpha(100, 50, 150, 0.25)).toBeCloseTo(0.625, 9);
    expect(fadeAlpha(150, 50, 150, 0.25)).toBe(0.25);
    expect(fadeAlpha(1e6, 50, 150, 0.25)).toBe(0.25);
  });
  it('degenerate range is a step; NaN length is opaque; min clamps', () => {
    expect(fadeAlpha(60, 50, 50, 0.3)).toBe(0.3);
    expect(fadeAlpha(NaN, 50, 150, 0.3)).toBe(1);
    expect(fadeAlpha(500, 50, 150, -2)).toBe(0);
  });
});

describe('edgeWidthPx', () => {
  it('maps weight to widthPx..maxWidthPx by sqrt', () => {
    expect(edgeWidthPx(1, 1, 3)).toBe(1);
    expect(edgeWidthPx(0, 1, 3)).toBe(1);
    expect(edgeWidthPx(NaN, 1, 3)).toBe(1);
    expect(edgeWidthPx(WEIGHT_AT_MAX_WIDTH, 1, 3)).toBe(3);
    expect(edgeWidthPx(100, 1, 3)).toBe(3);
    const mid = edgeWidthPx(2.25, 1, 3); // sqrt = 1.5 → halfway
    expect(mid).toBeCloseTo(2, 9);
  });
  it('a max below the min collapses to the min', () => {
    expect(edgeWidthPx(10, 2, 1)).toBe(2);
  });
});

describe('selectDrawnEdges (ADR-2181 D4: highest weight first)', () => {
  it('draws everything, in plan order, when it fits', () => {
    expect(Array.from(selectDrawnEdges([1, 5, 2], 3, 4))).toEqual([0, 1, 2]);
  });
  it('keeps the heaviest when the budget is short, stable on ties, in plan order', () => {
    const w = [1, 9, 3, 9, 0.5, 3];
    expect(Array.from(selectDrawnEdges(w, 6, 3))).toEqual([1, 2, 3]);
    expect(Array.from(selectDrawnEdges(w, 6, 0))).toEqual([]);
  });
  it('a capacity from resolveOverviewEdgeCapacity with a numeric budget truncates and reports', () => {
    const weights = Float32Array.from({ length: 10 }, (_, i) => i);
    const cap = resolveOverviewEdgeCapacity(4, 10);
    expect(cap).toEqual({ capacity: 4, drawn: 4, truncated: true });
    expect(Array.from(selectDrawnEdges(weights, 10, cap.capacity))).toEqual([6, 7, 8, 9]);
    expect(resolveOverviewEdgeCapacity('auto', 147_048)).toEqual({ capacity: 262_144, drawn: 147_048, truncated: false });
  });
});

describe('overview edge report store (no silent cap)', () => {
  beforeEach(() => resetOverviewEdgeReport());
  it('publishes changes once and the notice names N of M', () => {
    let calls = 0;
    const off = subscribeOverviewEdgeReport(() => { calls++; });
    publishOverviewEdgeReport({ capacity: 4, drawn: 4, total: 10, truncated: true });
    publishOverviewEdgeReport({ capacity: 4, drawn: 4, total: 10, truncated: true });
    expect(calls).toBe(1);
    expect(getOverviewEdgeReport()).toEqual({ capacity: 4, drawn: 4, total: 10, truncated: true });
    expect(overviewEdgeNoticeText(getOverviewEdgeReport())).toBe('4 of 10 edges drawn (highest weight first)');
    publishOverviewEdgeReport({ capacity: 16, drawn: 10, total: 10, truncated: false });
    expect(overviewEdgeNoticeText(getOverviewEdgeReport())).toBeNull();
    off();
  });
});

describe('one length fade for the CPU and the shader (VC-31 parallel fade)', () => {
  // A stand-in for the three/tsl module over plain numbers: if tslScalarOps
  // wires any op wrongly, the shader's fade parts from the CPU's here.
  class N {
    constructor(readonly v: number) {}
    add(b: N) { return new N(this.v + b.v); }
    sub(b: N) { return new N(this.v - b.v); }
    mul(b: N) { return new N(this.v * b.v); }
    div(b: N) { return new N(this.v / b.v); }
    greaterThan(b: N) { return this.v > b.v; }
  }
  const T = {
    float: (v: number) => new N(v),
    max: (a: unknown, b: unknown) => new N(Math.max((a as N).v, (b as N).v)),
    clamp: (a: unknown, lo: number, hi: number) => new N(Math.min(hi, Math.max(lo, (a as N).v))),
    select: (c: unknown, a: unknown, b: unknown) => (c ? a : b) as N,
  };
  const ops = tslScalarOps(T as unknown as Parameters<typeof tslScalarOps>[0]);

  it('the TSL ops give the CPU result across the range, the degenerate step and the clamp', () => {
    const cases: [number, number, number, number][] = [
      [0, 50, 150, 0.25], [50, 50, 150, 0.25], [75, 50, 150, 0.25], [100, 50, 150, 0.25], [150, 50, 150, 0.25],
      [1e6, 50, 150, 0.25], [60, 50, 50, 0.3], [500, 50, 150, -2], [500, 50, 150, 3], [NaN, 50, 150, 0.3],
    ];
    for (const [len, near, far, min] of cases) {
      const tsl = fadeAlphaWith(ops, new N(len), new N(near), new N(far), new N(min)) as unknown as N;
      expect(tsl.v).toBeCloseTo(fadeAlpha(len, near, far, min), 12);
      expect(fadeAlphaWith(numberOps, len, near, far, min)).toBe(fadeAlpha(len, near, far, min));
    }
  });
});
