import { describe, it, expect } from 'vitest';
import { HighlightMask, HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_SELECTED } from '../../state/highlightMask';
import { ExploreStore, widgetSource } from '../exploreStore';
import { writeFilter, writeEmphasis, activeFilterSources, SOURCE } from '../filterRegistry';
import { exploreNodes } from './fixtures';

function setup() {
  const nodes = exploreNodes();
  const index = new Map(nodes.map((n, i) => [String(n.id), i]));
  const mask = new HighlightMask(() => index);
  const store = new ExploreStore(mask);
  store.setTable({ nodes, indexMap: index, nodeFilter: { includeLinkedPages: false } });
  const greyed = () => nodes.filter((n) => mask.bitsFor(String(n.id)) & HIGHLIGHT_FILTERED_OUT).map((n) => Number(n.id));
  return { nodes, index, mask, store, greyed };
}

describe('ExploreStore → highlight mask (ADR-2185 D3)', () => {
  it('each widget is its own filter source; the graph greys the intersection and keeps every slot', () => {
    const { mask, store, greyed } = setup();
    const size = mask.getMask().length;
    store.setClause('categories', { kind: 'values', column: 'domain', values: ['robotics'] });
    expect(activeFilterSources(mask)).toEqual([widgetSource('categories')]);
    // robotics = 1..4 and the stub 12 (it is not eligible for counts, but its id is robotics)
    expect(greyed()).toEqual([5, 6, 7, 8, 9, 10, 11]);
    store.setClause('histogram', { kind: 'range', column: 'quality_score', lo: 0.3, hi: Infinity });
    expect(greyed()).toEqual([1, 2, 5, 6, 7, 8, 9, 10, 11, 12]);
    // display-only: the mask keeps one slot per delivered node
    expect(mask.getMask().length).toBe(size);
    expect(size).toBe(12);
    expect(store.activeCount()).toBe(2);
  });

  it('clearAllFilters clears explore sources only: selection and settings filters survive', () => {
    const { mask, store, greyed } = setup();
    writeFilter(SOURCE.SETTINGS_FILTER, ['7'], 'exclude', mask);
    writeEmphasis(SOURCE.SELECTION, HIGHLIGHT_SELECTED, ['2'], mask);
    store.setClause('search', { kind: 'search', query: 'robot arm' });
    store.clearAll();
    expect(store.activeCount()).toBe(0);
    expect(activeFilterSources(mask)).toEqual([SOURCE.SETTINGS_FILTER]);
    expect(greyed()).toEqual([7]);
    expect(mask.bitsFor('2') & HIGHLIGHT_SELECTED).toBeTruthy();
  });

  it('re-applies clauses to a rebuilt table (a new graph load)', () => {
    const { nodes, index, mask, store, greyed } = setup();
    store.setClause('categories', { kind: 'values', column: 'domain', values: ['blockchain'] });
    store.setTable({ nodes, indexMap: index });
    expect(greyed()).not.toContain(5);
    expect(greyed()).toContain(1);
    expect(store.crossfilter!.count()).toBe(3);
    expect(mask.activeSources()).toEqual([widgetSource('categories')]);
  });

  it('an empty search clause is no clause', () => {
    const { store, mask } = setup();
    store.setClause('search', { kind: 'search', query: '   ' });
    expect(store.activeCount()).toBe(0);
    expect(mask.activeSources()).toEqual([]);
  });
});

describe('ExploreStore context clauses', () => {
  it('a graph selection narrows the widget counts (ADR-2185 D3) without greying the graph or counting as a filter', () => {
    const { mask, store } = setup();
    store.setGraphSelection(['5', '6', '1']);
    const xf = store.crossfilter!;
    const c = xf.categories('domain', 'categories');
    expect(Object.fromEntries(c.values.map((v, i) => [v, c.counts[i]]))).toEqual({ robotics: 1, blockchain: 2, 'artificial-intelligence': 0 });
    expect(mask.activeSources()).toEqual([]);
    expect(store.activeCount()).toBe(0);
    expect(store.graphSelectionSize()).toBe(3);
    // lit on screen ignores the selection (it only emphasises)
    expect(store.litCount()).toBe(11);
    store.clearAll();
    expect(store.graphSelectionSize()).toBe(3);
    store.setGraphSelection(null);
    expect(xf.count()).toBe(11);
  });

  it('mirrors the settings grey-out so counts agree with the screen', () => {
    const { mask, store } = setup();
    writeFilter(SOURCE.SETTINGS_FILTER, ['1', '2', '5'], 'exclude', mask);
    expect(store.litCount()).toBe(8);
    const c = store.crossfilter!.categories('domain', 'categories');
    expect(Object.fromEntries(c.values.map((v, i) => [v, c.counts[i]]))).toEqual({ robotics: 2, blockchain: 2, 'artificial-intelligence': 4 });
    expect(store.activeCount()).toBe(0);
    // survives a rebuild, and follows the settings source being cleared
    const nodes = exploreNodes();
    store.setTable({ nodes, indexMap: new Map(nodes.map((n, i) => [String(n.id), i])) });
    expect(store.litCount()).toBe(8);
    writeFilter(SOURCE.SETTINGS_FILTER, null, 'exclude', mask);
    expect(store.litCount()).toBe(11);
  });
});
