import { describe, it, expect } from 'vitest';
import { HighlightMask, HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_HOVERED, HIGHLIGHT_SEARCH_HIT } from '../../state/highlightMask';
import {
  writeFilter, writeEmphasis, isFilteredOut, activeFilterSources, setHoveredNodeEmphasis,
  filterRegistryVersion, SOURCE,
} from '../filterRegistry';

const fresh = () => {
  const index = new Map([['a', 0], ['b', 1], ['c', 2]]);
  return new HighlightMask(() => index);
};

describe('filterRegistry (ADR-2183 mask writes)', () => {
  it('skips a write whose id set is already in force (no recomposition, no version bump)', () => {
    const mask = fresh();
    expect(writeFilter('filter:x', ['a', 'b'], 'include', mask)).toBe(true);
    const v = mask.version;
    expect(writeFilter('filter:x', new Set(['b', 'a']), 'include', mask)).toBe(false);
    expect(mask.version).toBe(v);
    expect(mask.bitsFor('c') & HIGHLIGHT_FILTERED_OUT).toBeTruthy();
    // a different mode is a real change
    expect(writeFilter('filter:x', ['a', 'b'], 'exclude', mask)).toBe(true);
    expect(mask.bitsFor('a') & HIGHLIGHT_FILTERED_OUT).toBeTruthy();
  });

  it('rewrites after something else cleared the source from the mask', () => {
    const mask = fresh();
    writeFilter('filter:x', ['a'], 'include', mask);
    mask.clearAll();
    expect(writeFilter('filter:x', ['a'], 'include', mask)).toBe(true);
    expect(activeFilterSources(mask)).toEqual(['filter:x']);
  });

  it('answers isFilteredOut by id across include and exclude sources', () => {
    const mask = fresh();
    writeFilter('filter:inc', ['a', 'b'], 'include', mask);
    writeFilter('filter:exc', ['b'], 'exclude', mask);
    expect(['a', 'b', 'c'].map((id) => isFilteredOut(id, mask))).toEqual([false, true, true]);
    const v = filterRegistryVersion(mask);
    writeFilter('filter:exc', null, 'exclude', mask);
    expect(filterRegistryVersion(mask)).toBeGreaterThan(v);
    expect(isFilteredOut('b', mask)).toBe(false);
  });

  it('keeps emphasis out of the filter count and clears an empty emphasis', () => {
    const mask = fresh();
    writeEmphasis(SOURCE.SEARCH, HIGHLIGHT_SEARCH_HIT, ['a'], mask);
    setHoveredNodeEmphasis('b', mask);
    expect(activeFilterSources(mask)).toEqual([]);
    expect(mask.bitsFor('b') & HIGHLIGHT_HOVERED).toBeTruthy();
    writeEmphasis(SOURCE.SEARCH, HIGHLIGHT_SEARCH_HIT, [], mask);
    setHoveredNodeEmphasis(null, mask);
    expect(mask.activeSources()).toEqual([]);
  });

  it('rejects a source outside its namespace', () => {
    const mask = fresh();
    expect(() => writeFilter('emph:x', ['a'], 'include', mask)).toThrow();
    expect(() => writeEmphasis('filter:x', HIGHLIGHT_SEARCH_HIT, ['a'], mask)).toThrow();
  });
});
