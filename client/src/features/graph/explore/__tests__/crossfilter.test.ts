import { describe, it, expect } from 'vitest';
import { buildNodeColumns } from '../tableSchema';
import { Crossfilter } from '../crossfilter';
import { exploreNodes } from './fixtures';

function make() {
  const nodes = exploreNodes();
  const indexMap = new Map(nodes.map((n, i) => [String(n.id), i]));
  const cols = buildNodeColumns(nodes, { indexMap });
  const eligible = new Uint8Array(cols.rowCount).fill(1);
  eligible[11] = 0; // the linked_page stub (population scope)
  return new Crossfilter(cols, eligible);
}

const counts = (xf: Crossfilter, col: string, except: string) => {
  const c = xf.categories(col, except);
  return Object.fromEntries(c.values.map((v, i) => [v, c.counts[i]]));
};

describe('Crossfilter (ADR-2185 D2: each widget aggregates excluding its own clause)', () => {
  it('counts only eligible rows', () => {
    const xf = make();
    expect(xf.eligibleCount()).toBe(11);
    expect(counts(xf, 'domain', 'cat')).toEqual({ robotics: 4, blockchain: 3, 'artificial-intelligence': 4 });
  });

  it('brushing widget A re-aggregates B and C but not A', () => {
    const xf = make();
    // A: categories → robotics only
    xf.setClause('cat', { kind: 'values', column: 'domain', values: ['robotics'] });
    // A's own bars keep the full distribution
    expect(counts(xf, 'domain', 'cat')).toEqual({ robotics: 4, blockchain: 3, 'artificial-intelligence': 4 });
    // B: the quality histogram narrows to the four robotics rows (0.1..0.4)
    const h = xf.histogram('quality_score', 4, 'hist');
    expect(Array.from(h.totals).reduce((a, b) => a + b, 0)).toBe(11);
    expect(Array.from(h.counts).reduce((a, b) => a + b, 0)).toBe(4);
    // C: the timeline narrows too (robotics rows 1-4 all carry a date)
    const t = xf.histogram('last_commit', 2, 'time');
    expect(Array.from(t.counts)).toEqual([3, 1]);
    expect(Array.from(t.totals)).toEqual([3, 3]);
    expect(t.nulls).toBe(5);

    // B brushes quality ≥ 0.3: A's counts now reflect B, B's own do not reflect B
    xf.setClause('hist', { kind: 'range', column: 'quality_score', lo: 0.3, hi: Infinity });
    expect(counts(xf, 'domain', 'cat')).toEqual({ robotics: 2, blockchain: 3, 'artificial-intelligence': 4 });
    expect(Array.from(xf.histogram('quality_score', 4, 'hist').counts).reduce((a, b) => a + b, 0)).toBe(4);
    // the combined selection is the intersection
    expect(xf.selectedIds()).toEqual(['3', '4']);
    expect(xf.count()).toBe(2);
  });

  it('clearing a clause restores the others', () => {
    const xf = make();
    xf.setClause('cat', { kind: 'values', column: 'domain', values: ['blockchain'] });
    xf.setClause('cat', null);
    expect(xf.selectedIds()).toBeNull();
    expect(xf.count()).toBe(11);
  });

  it('search matches label and tags, case-insensitively; one widget clause = one id set', () => {
    const xf = make();
    xf.setClause('search', { kind: 'search', query: 'ROBOT' });
    expect(xf.idsPassingClause('search')).toEqual(['3']);
    xf.setClause('search', { kind: 'search', query: 'consensus' });
    expect(xf.idsPassingClause('search')).toEqual(['5']);
  });

  it('bins span [min, p99] and the last bin is open-topped', () => {
    const xf = make();
    const h = xf.histogram('quality_score', 5, 'x');
    expect(h.min).toBeCloseTo(0.1);
    expect(h.max).toBeCloseTo(0.95);
    expect(Array.from(h.totals).reduce((a, b) => a + b, 0)).toBe(11);
  });
});

describe('Crossfilter fixed brush edges and id clauses', () => {
  it('bins by the edges captured at brush time, whatever the current domain', () => {
    const xf = make();
    const edges = [0, 0.5, 2, 3]; // last bin open-topped, first open below
    const h = xf.histogram('quality_score', 20, 'x', edges);
    expect(Array.from(h.edges)).toEqual(edges);
    expect(Array.from(h.totals)).toEqual([4, 7, 0, 0]);
  });

  it('an ids clause includes or excludes by node id', () => {
    const xf = make();
    xf.setClause('sel', { kind: 'ids', ids: new Set(['1', '12', '5']), mode: 'include' });
    expect(xf.selectedIds()).toEqual(['1', '5']); // 12 is not eligible
    xf.setClause('sel', { kind: 'ids', ids: new Set(['1']), mode: 'exclude' });
    expect(xf.count()).toBe(10);
  });
});
