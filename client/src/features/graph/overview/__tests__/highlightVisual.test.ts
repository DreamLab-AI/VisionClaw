import { describe, it, expect } from 'vitest';
import {
  highlightVisualFor, createHighlightVisual, edgeHighlightAlpha, edgeIsEmphasised,
  backgroundFromSetting, EMPHASIS_SCALE, EMPHASIS_BRIGHTEN, type GreyoutStyle,
} from '../highlightVisual';
import {
  HIGHLIGHT_SELECTED, HIGHLIGHT_HOVERED, HIGHLIGHT_SEARCH_HIT, HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_FOCUS_NEIGHBOUR,
  HIGHLIGHT_LEVEL_GREYED, HIGHLIGHT_LEVEL_NORMAL, HIGHLIGHT_LEVEL_EMPHASISED,
} from '../../state/highlightMask';
import { DEFAULT_GRAPH_VIEW } from '../../../../api/settings/defaults';

const style: GreyoutStyle = {
  greyoutMix: DEFAULT_GRAPH_VIEW.explore.greyoutMix,
  greyoutOpacity: DEFAULT_GRAPH_VIEW.explore.greyoutOpacity,
  background: { r: 0, g: 0, b: 0 },
};

describe('highlight level → visual (ADR-2183 D3, ADR-2182 D4)', () => {
  const v = createHighlightVisual();

  it('normal: encoded colour, opaque, unscaled, labelled, no bloom under emphasis scope', () => {
    highlightVisualFor(0, 0.2, 0.4, 0.6, style, 'emphasis', v);
    expect(v.level).toBe(HIGHLIGHT_LEVEL_NORMAL);
    expect([v.r, v.g, v.b, v.alpha, v.scale]).toEqual([0.2, 0.4, 0.6, 1, 1]);
    expect(v.bloom).toBe(false);
    expect(v.label).toBe(true);
  });

  it("normal nodes bloom only under bloomScope 'scene'", () => {
    expect(highlightVisualFor(0, 1, 1, 1, style, 'scene', v).bloom).toBe(true);
    expect(highlightVisualFor(0, 1, 1, 1, style, 'off', v).bloom).toBe(false);
  });

  it('greyed: desaturated, mixed toward the background, greyoutOpacity, no label, no glow', () => {
    highlightVisualFor(HIGHLIGHT_FILTERED_OUT, 1, 0, 0, style, 'scene', v);
    expect(v.level).toBe(HIGHLIGHT_LEVEL_GREYED);
    const l = 0.2126;
    const want = l * (1 - style.greyoutMix);
    expect(v.r).toBeCloseTo(want, 6);
    expect(v.g).toBeCloseTo(want, 6);
    expect(v.b).toBeCloseTo(want, 6);
    expect(v.alpha).toBe(style.greyoutOpacity);
    expect(v.bloom).toBe(false);
    expect(v.label).toBe(false);
    expect(v.scale).toBe(1);
  });

  it('greyed mixes toward a light background too', () => {
    highlightVisualFor(HIGHLIGHT_FILTERED_OUT, 0, 0, 0, { ...style, greyoutMix: 1, background: { r: 1, g: 1, b: 1 } }, 'emphasis', v);
    expect([v.r, v.g, v.b]).toEqual([1, 1, 1]);
  });

  it('emphasised (selected, hovered, search hit, neighbour): larger, brighter, blooms', () => {
    for (const bit of [HIGHLIGHT_SELECTED, HIGHLIGHT_HOVERED, HIGHLIGHT_SEARCH_HIT, HIGHLIGHT_FOCUS_NEIGHBOUR]) {
      highlightVisualFor(bit, 0.5, 0.5, 0.5, style, 'emphasis', v);
      expect(v.level).toBe(HIGHLIGHT_LEVEL_EMPHASISED);
      expect(v.scale).toBe(EMPHASIS_SCALE);
      expect(v.r).toBeCloseTo(0.5 * EMPHASIS_BRIGHTEN, 6);
      expect(v.bloom).toBe(true);
      expect(v.label).toBe(true);
    }
    expect(highlightVisualFor(HIGHLIGHT_SELECTED, 1, 1, 1, style, 'off', v).bloom).toBe(false);
  });

  it('a selected node that a filter greys still emphasises; a greyed search hit stays greyed', () => {
    expect(highlightVisualFor(HIGHLIGHT_SELECTED | HIGHLIGHT_FILTERED_OUT, 1, 1, 1, style, 'emphasis', v).level).toBe(HIGHLIGHT_LEVEL_EMPHASISED);
    expect(highlightVisualFor(HIGHLIGHT_SEARCH_HIT | HIGHLIGHT_FILTERED_OUT, 1, 1, 1, style, 'emphasis', v).level).toBe(HIGHLIGHT_LEVEL_GREYED);
  });
});

describe('edge highlight', () => {
  it('an edge is greyed when either endpoint is', () => {
    expect(edgeHighlightAlpha(0, 0, 0.1)).toBe(1);
    expect(edgeHighlightAlpha(HIGHLIGHT_FILTERED_OUT, 0, 0.1)).toBe(0.1);
    expect(edgeHighlightAlpha(0, HIGHLIGHT_FILTERED_OUT, 0.1)).toBe(0.1);
  });
  it('an edge is emphasised when it touches an emphasised node and neither end is greyed', () => {
    expect(edgeIsEmphasised(HIGHLIGHT_SELECTED, 0)).toBe(true);
    expect(edgeIsEmphasised(HIGHLIGHT_SELECTED, HIGHLIGHT_FILTERED_OUT)).toBe(false);
    expect(edgeIsEmphasised(0, 0)).toBe(false);
  });
});

describe('backgroundFromSetting', () => {
  it('parses hex to linear RGB and falls back to black', () => {
    const w = backgroundFromSetting('#ffffff');
    expect([w.r, w.g, w.b]).toEqual([1, 1, 1]);
    const m = backgroundFromSetting('#808080');
    expect(m.r).toBeCloseTo(0.2158605, 5);
    expect(backgroundFromSetting('nonsense')).toEqual({ r: 0, g: 0, b: 0 });
    expect(backgroundFromSetting(undefined)).toEqual({ r: 0, g: 0, b: 0 });
  });
});
