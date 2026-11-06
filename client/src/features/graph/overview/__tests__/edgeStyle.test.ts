import { describe, it, expect, beforeEach } from 'vitest';
import { fadeAlpha, edgeWidthPx, selectDrawnEdges, WEIGHT_AT_MAX_WIDTH } from '../edgeStyle';
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
