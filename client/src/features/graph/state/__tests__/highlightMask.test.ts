import { describe, it, expect, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import {
  HighlightMask,
  HIGHLIGHT_SELECTED,
  HIGHLIGHT_HOVERED,
  HIGHLIGHT_SEARCH_HIT,
  HIGHLIGHT_FILTERED_OUT,
  HIGHLIGHT_FOCUS_NEIGHBOUR,
  highlightLevel,
} from '../highlightMask';
import { useHighlightMaskVersion } from '../useHighlightMaskVersion';

const mapOf = (ids: string[]) => new Map(ids.map((id, i) => [id, i]));

function maskWith(ids: string[]) {
  let map: ReadonlyMap<string, number> = mapOf(ids);
  const mask = new HighlightMask(() => map);
  return { mask, setMap: (m: ReadonlyMap<string, number>) => { map = m; } };
}

describe('HighlightMask (ADR-2183)', () => {
  it('sizes from the index map and starts all-normal', () => {
    const { mask } = maskWith(['a', 'b', 'c']);
    expect(Array.from(mask.getMask())).toEqual([0, 0, 0]);
    expect(highlightLevel(mask.bitsFor('a'))).toBe(1);
  });

  it('D1/D2: two filter predicates and a selection combine into the expected mask', () => {
    const { mask } = maskWith(['a', 'b', 'c', 'd']);
    mask.setFilter('maturity', ['a', 'b', 'c']);          // include-set: d greyed
    mask.setFilter('timeline', ['b'], 'exclude');          // b greyed
    mask.setEmphasis('selection', HIGHLIGHT_SELECTED, ['c']);
    const m = mask.getMask();
    expect(m[0]).toBe(0);
    expect(m[1]).toBe(HIGHLIGHT_FILTERED_OUT);
    expect(m[2]).toBe(HIGHLIGHT_SELECTED);
    expect(m[3]).toBe(HIGHLIGHT_FILTERED_OUT);
    expect(Array.from(m, highlightLevel)).toEqual([1, 0, 2, 0]);
    expect(mask.presentFlags).toBe(HIGHLIGHT_FILTERED_OUT | HIGHLIGHT_SELECTED);
  });

  it('writers do not clobber each other; clearing one source restores only its effect', () => {
    const { mask } = maskWith(['a', 'b']);
    mask.setFilter('w1', ['a']);
    mask.setFilter('w2', ['b']);
    expect(Array.from(mask.getMask())).toEqual([HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_FILTERED_OUT]);
    mask.setFilter('w1', null);
    expect(Array.from(mask.getMask())).toEqual([HIGHLIGHT_FILTERED_OUT, 0]);
    mask.setEmphasis('hover', HIGHLIGHT_HOVERED, ['b']);
    mask.setEmphasis('search', HIGHLIGHT_SEARCH_HIT, ['b']);
    mask.setEmphasis('focus', HIGHLIGHT_FOCUS_NEIGHBOUR, ['a']);
    expect(mask.bitsFor('b')).toBe(HIGHLIGHT_HOVERED | HIGHLIGHT_SEARCH_HIT);
    expect(mask.bitsFor('a')).toBe(HIGHLIGHT_FILTERED_OUT | HIGHLIGHT_FOCUS_NEIGHBOUR);
    expect(highlightLevel(mask.bitsFor('a'))).toBe(0); // greyed neighbour stays greyed
    mask.clearAll();
    expect(Array.from(mask.getMask())).toEqual([0, 0]);
  });

  it('version is monotonic and bumps once per batch', () => {
    const { mask } = maskWith(['a', 'b']);
    mask.getMask();
    const listener = vi.fn();
    mask.subscribe(listener);
    const v0 = mask.version;
    mask.batch(() => {
      mask.setFilter('w1', ['a']);
      mask.setEmphasis('sel', HIGHLIGHT_SELECTED, ['a']);
    });
    expect(mask.version).toBe(v0 + 1);
    expect(listener).toHaveBeenCalledTimes(1);
    mask.setFilter('w1', null);
    expect(mask.version).toBe(v0 + 2);
  });

  it('a new index map (graph reload) recomposes sources by id', () => {
    const { mask, setMap } = maskWith(['a', 'b']);
    mask.setEmphasis('sel', HIGHLIGHT_SELECTED, ['b']);
    expect(Array.from(mask.getMask())).toEqual([0, HIGHLIGHT_SELECTED]);
    setMap(new Map([['b', 0], ['x', 1], ['a', 2]]));
    expect(Array.from(mask.getMask())).toEqual([HIGHLIGHT_SELECTED, 0, 0]);
    expect(mask.size).toBe(3);
  });

  it('D4: an id the server withheld has no slot and is ignored', () => {
    const { mask } = maskWith(['a']);
    mask.setEmphasis('search', HIGHLIGHT_SEARCH_HIT, ['private-node']);
    expect(mask.bitsFor('private-node')).toBe(0);
    expect(Array.from(mask.getMask())).toEqual([0]);
  });

  it('resize(n) forces the slot count', () => {
    const { mask } = maskWith(['a']);
    mask.getMask();
    mask.resize(4);
    expect(mask.size).toBe(4);
  });

  it('useHighlightMaskVersion re-renders on recomposition', () => {
    const { mask } = maskWith(['a']);
    mask.getMask();
    const { result } = renderHook(() => useHighlightMaskVersion(mask));
    const before = result.current;
    act(() => mask.setEmphasis('sel', HIGHLIGHT_SELECTED, ['a']));
    expect(result.current).toBe(before + 1);
  });
});
