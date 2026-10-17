import { describe, expect, it } from 'vitest';
import { labelScore, selectLabels, topK, type LabelCandidate } from '../scene/labelSelect';

const opt = { max: 300, width: 1000, height: 800, cell: 10 };
const c = (index: number, x: number, y: number, score: number, extra: Partial<LabelCandidate> = {}): LabelCandidate => ({ index, x, y, w: 60, h: 16, score, ...extra });

describe('label selection', () => {
  it('scores by importance over distance squared', () => {
    expect(labelScore(1, 10)).toBeGreaterThan(labelScore(1, 20));
    expect(labelScore(1, 10) / labelScore(1, 20)).toBeCloseTo(4, 6);
    expect(labelScore(0.9, 10)).toBeGreaterThan(labelScore(0.1, 10));
  });

  it('rejects overlapping boxes, keeping the higher score', () => {
    const out = selectLabels([c(1, 100, 100, 1), c(2, 110, 102, 5), c(3, 400, 100, 0.5)], opt);
    expect(out).toEqual([2, 3]);
  });

  it('keeps boxes that are clear of each other on the grid', () => {
    const out = selectLabels([c(1, 100, 100, 1), c(2, 100, 140, 1), c(3, 300, 100, 1)], opt);
    expect(out.sort()).toEqual([1, 2, 3]);
  });

  it('lets pinned labels (focus/hover) win even over higher scores', () => {
    const out = selectLabels([c(1, 100, 100, 100), c(2, 105, 100, 0, { pinned: true })], opt);
    expect(out).toEqual([2]);
  });

  it('places pinned labels even when they collide with each other', () => {
    const out = selectLabels([c(1, 100, 100, 0, { pinned: true }), c(2, 100, 100, 0, { pinned: true })], opt);
    expect(out.sort()).toEqual([1, 2]);
  });

  it('caps at max and drops off-screen candidates', () => {
    const many: LabelCandidate[] = [];
    for (let k = 0; k < 2000; k++) many.push({ index: k, x: (k % 40) * 25 + 10, y: Math.floor(k / 40) * 20 + 20, w: 8, h: 8, score: Math.random() });
    expect(selectLabels(many, { ...opt, max: 300 }).length).toBe(300);
    expect(selectLabels([c(1, -500, 100, 1), c(2, 100, 5000, 1)], opt)).toEqual([]);
  });

  it('topK keeps the k best without a full sort', () => {
    const items = Array.from({ length: 1000 }, (_, i) => ({ score: (i * 7919) % 1000 }));
    const best = topK(items, 5).map((x) => x.score);
    expect(best).toEqual([999, 998, 997, 996, 995]);
    expect(topK(items, 0)).toEqual([]);
  });
});
