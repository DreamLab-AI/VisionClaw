import { describe, it, expect } from 'vitest';
import {
  highlightVisualFor, createHighlightVisual, edgeHighlightAlpha, edgeIsEmphasised,
  backgroundFromSetting, EMPHASIS_SCALE, EMPHASIS_BRIGHTEN, type GreyoutStyle,
  linearBlendAlpha, srgbLuma, BLEND_REFERENCE_LINEAR,
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

describe('linear-space blend correction (ADR-2183 D3, WebGPURenderer)', () => {
  // sidecar measurement: 0.8-linear grey over #101018
  const lin = (s: number) => (s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4);
  const srgb = (l: number) => (l <= 0.0031308 ? l * 12.92 : 1.055 * l ** (1 / 2.4) - 0.055);
  const bg = srgbLuma('#101018');
  const blendGl = (a: number, cLin: number) => a * srgb(cLin) + (1 - a) * bg;
  const blendGpu = (a: number, cLin: number) => srgb(a * cLin + (1 - a) * lin(bg));

  it('reproduces the measured WebGPURenderer brightening it corrects', () => {
    // measured: 37/255 (WebGL) vs 82/255 (WebGPURenderer) at α 0.1, colour 0.8 linear
    expect(Math.round(blendGl(0.1, 0.8) * 255)).toBeGreaterThanOrEqual(36);
    expect(Math.round(blendGl(0.1, 0.8) * 255)).toBeLessThanOrEqual(38);
    expect(Math.round(blendGpu(0.1, 0.8) * 255)).toBeGreaterThanOrEqual(81);
    expect(Math.round(blendGpu(0.1, 0.8) * 255)).toBeLessThanOrEqual(83);
  });

  it('is exact for the reference colour and close for brighter / dimmer ones', () => {
    for (const a of [0.05, 0.1, 0.2, 0.4]) {
      const a2 = linearBlendAlpha(a, bg);
      expect(a2).toBeLessThan(a);
      expect(blendGpu(a2, BLEND_REFERENCE_LINEAR)).toBeCloseTo(blendGl(a, BLEND_REFERENCE_LINEAR), 6);
      for (const c of [0.2, 0.8]) expect(Math.abs(blendGpu(a2, c) - blendGl(a, c)) * 255).toBeLessThan(8);
    }
  });

  it('keeps 0 and 1, and is a no-op over a background brighter than the reference', () => {
    expect(linearBlendAlpha(0, bg)).toBe(0);
    expect(linearBlendAlpha(1, bg)).toBe(1);
    expect(linearBlendAlpha(0.3, 0.95)).toBe(0.3);
  });

  it('srgbLuma reads CSS colours in sRGB', () => {
    expect(srgbLuma('#000000')).toBeCloseTo(0, 6);
    expect(srgbLuma('#ffffff')).toBeCloseTo(1, 6);
    expect(srgbLuma('#808080')).toBeCloseTo(128 / 255, 2);
  });
});
