import { describe, it, expect } from 'vitest';
import {
  selectLabels, rankByPriority, labelPriorities, createLabelSelection, CollisionGrid, glyphCapacityFor,
  TIER_SELECTION, TIER_OVERVIEW, TIER_DYNAMIC, type LabelFrame,
} from '../labelBudget';
import { HIGHLIGHT_SELECTED, HIGHLIGHT_SEARCH_HIT, HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_FOCUS_NEIGHBOUR } from '../../state/highlightMask';
import { DEFAULT_GRAPH_VIEW } from '../../../../api/settings/defaults';

function frame(n: number, opts: Partial<LabelFrame> & { priority: number[] }): LabelFrame {
  const priority = opts.priority;
  // spread on a grid so nothing collides unless a test puts nodes together
  const sx = opts.sx ?? Array.from({ length: n }, (_, i) => (i % 20) * 100 + 5);
  const sy = opts.sy ?? Array.from({ length: n }, (_, i) => Math.floor(i / 20) * 40 + 5);
  return {
    count: n, priority, order: rankByPriority(priority, n),
    bits: opts.bits ?? new Uint8Array(n), sx, sy,
    onScreen: opts.onScreen ?? new Uint8Array(n).fill(1),
    viewWidth: 2000, viewHeight: 2000, cellW: 100, cellH: 20,
  };
}

const run = (f: LabelFrame, b = { budget: 3, dynamicBudget: 2, selectedBudget: 2 }) => {
  const out = createLabelSelection(b.budget + b.dynamicBudget + b.selectedBudget + 2);
  selectLabels(f, b, new CollisionGrid(), new Uint8Array(f.count), out);
  return Array.from({ length: out.count }, (_, k) => [out.nodes[k], out.tiers[k]]);
};

describe('label budget (ADR-2186)', () => {
  it('defaults are the three tiers 40 / 40 / 100', () => {
    expect(DEFAULT_GRAPH_VIEW.labels).toMatchObject({ budget: 40, dynamicBudget: 40, selectedBudget: 100 });
    expect(glyphCapacityFor(DEFAULT_GRAPH_VIEW.labels)).toBeLessThan(32768);
  });

  it('ranks by priority, not position, and respects the budgets', () => {
    const p = Array.from({ length: 10 }, (_, i) => i); // node 9 most important
    const sel = run(frame(10, { priority: p }));
    expect(sel).toEqual([[9, TIER_OVERVIEW], [8, TIER_OVERVIEW], [7, TIER_OVERVIEW], [6, TIER_DYNAMIC], [5, TIER_DYNAMIC]]);
  });

  it('overview tier is global: off-screen top nodes leave room for the dynamic tier', () => {
    const p = Array.from({ length: 10 }, (_, i) => i);
    const onScreen = new Uint8Array(10).fill(1); onScreen[9] = 0; onScreen[8] = 0;
    const sel = run(frame(10, { priority: p, onScreen }));
    expect(sel).toEqual([[7, TIER_OVERVIEW], [6, TIER_DYNAMIC], [5, TIER_DYNAMIC]]);
  });

  it('selected nodes and search hits are always labelled; greyed nodes never are', () => {
    const p = Array.from({ length: 10 }, (_, i) => i);
    const bits = new Uint8Array(10);
    bits[0] = HIGHLIGHT_SELECTED;
    bits[1] = HIGHLIGHT_SEARCH_HIT;
    bits[9] = HIGHLIGHT_FILTERED_OUT;
    bits[8] = HIGHLIGHT_SEARCH_HIT | HIGHLIGHT_FILTERED_OUT; // greyed search hit: no label
    const sel = run(frame(10, { priority: p, bits }));
    expect(sel.slice(0, 2)).toEqual([[0, TIER_SELECTION], [1, TIER_SELECTION]]);
    const ids = sel.map(([i]) => i);
    expect(ids).not.toContain(9);
    expect(ids).not.toContain(8);
    expect(ids.slice(2, 5)).toEqual([7, 6, 5]);
  });

  it('a greyed but selected node is still labelled (selection wins)', () => {
    const bits = new Uint8Array(3); bits[0] = HIGHLIGHT_SELECTED | HIGHLIGHT_FILTERED_OUT;
    expect(run(frame(3, { priority: [0, 1, 2], bits }))[0]).toEqual([0, TIER_SELECTION]);
  });

  it('focus neighbours fill the selection tier by priority, within selectedBudget', () => {
    const bits = new Uint8Array(6).fill(HIGHLIGHT_FOCUS_NEIGHBOUR);
    const sel = run(frame(6, { priority: [0, 1, 2, 3, 4, 5], bits }), { budget: 0, dynamicBudget: 0, selectedBudget: 2 });
    expect(sel).toEqual([[5, TIER_SELECTION], [4, TIER_SELECTION]]);
  });

  it('removes screen-space collisions: the lower-priority label in a taken cell is skipped', () => {
    const sx = [10, 12, 500];
    const sy = [10, 11, 10];
    const sel = run(frame(3, { priority: [5, 4, 1], sx, sy }), { budget: 3, dynamicBudget: 0, selectedBudget: 0 });
    expect(sel.map(([i]) => i)).toEqual([0, 2]);
  });

  it('priority is centrality when present, else degree', () => {
    const out = new Float32Array(3);
    const ri = [0, 1, 2];
    const deg = (i: number) => [3, 1, 2][i];
    expect(labelPriorities(ri, 3, deg, null, out)).toBe('degree');
    expect(Array.from(out)).toEqual([3, 1, 2]);
    const analytics = new Float32Array(15);
    analytics[1 * 5 + 3] = 0.5;
    expect(labelPriorities(ri, 3, deg, analytics, out)).toBe('centrality');
    expect(Array.from(out)).toEqual([0, 0.5, 0]);
  });
});
