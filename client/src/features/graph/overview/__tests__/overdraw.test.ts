/**
 * Overview edge overdraw at the shipped ADR-2181 D2 defaults (cx/webgpu, 2026-11-06).
 *
 * The live frame that showed the "white slab": 140,352 overview edges, most of
 * them inside a ~400 × 450 px box at default fit, measured identical on the
 * WebGPU and WebGL2 backends (crop grey-mean 185.6, 94% of pixels > 180), and
 * still saturated at a quarter of the default alpha (183.5, 94%).
 */
import { describe, it, expect } from 'vitest';
import {
  edgeOverdrawCoverage, maxEdgeAlphaForCoverage, maxEdgesForCoverage,
  fadeAlpha, edgeWidthPx, OVERVIEW_EDGE_BASE_OPACITY,
} from '../edgeStyle';
import { DEFAULT_GRAPH_VIEW } from '../../../../api/settings/defaults';

const oe = DEFAULT_GRAPH_VIEW.overviewEdges;
const LIVE_EDGES = 140_352;
const BOX_AREA_PX = 400 * 450;
/** Mean distance between two uniform points in a near-square box ≈ 0.52 × side. */
const MEAN_LEN_PX = 0.52 * Math.sqrt(BOX_AREA_PX);
/** Per-edge alpha of a long edge at the defaults (fade floor × base opacity). */
const DEFAULT_ALPHA = fadeAlpha(MEAN_LEN_PX, oe.fadeNearPx, oe.fadeFarPx, oe.minOpacity) * OVERVIEW_EDGE_BASE_OPACITY;
const WIDTH = edgeWidthPx(1, oe.widthPx, oe.maxWidthPx);
const scene = { edgeCount: LIVE_EDGES, areaPx: BOX_AREA_PX, meanLengthPx: MEAN_LEN_PX, widthPx: WIDTH };
/** One step of an 8-bit render target: an edge fainter than this does not show. */
const ALPHA_8BIT_STEP = 1 / 255;

describe('edgeOverdrawCoverage (the model)', () => {
  it('is 0 with no edges or no alpha, and grows towards 1 with overdraw', () => {
    expect(edgeOverdrawCoverage({ ...scene, edgeCount: 0, alpha: 0.5 })).toBe(0);
    expect(edgeOverdrawCoverage({ ...scene, alpha: 0 })).toBe(0);
    const a = edgeOverdrawCoverage({ ...scene, edgeCount: 1000, alpha: 0.1 });
    const b = edgeOverdrawCoverage({ ...scene, edgeCount: 2000, alpha: 0.1 });
    expect(b).toBeGreaterThan(a);
    expect(b).toBeLessThan(1);
  });

  it('maxEdgeAlphaForCoverage and maxEdgesForCoverage invert it', () => {
    const alpha = maxEdgeAlphaForCoverage(0.5, scene);
    expect(edgeOverdrawCoverage({ ...scene, alpha })).toBeCloseTo(0.5, 9);
    const n = maxEdgesForCoverage(0.5, { ...scene, alpha: 0.15 });
    expect(edgeOverdrawCoverage({ ...scene, edgeCount: n, alpha: 0.15 })).toBeCloseTo(0.5, 9);
  });
});

describe('ADR-2181 D2 defaults at the live edge count (flags the thresholds; not a fix)', () => {
  it('the long-edge alpha at the defaults is the fade floor × base opacity, 0.15', () => {
    expect(DEFAULT_ALPHA).toBeCloseTo(0.15, 9);
    expect(WIDTH).toBe(1);
  });

  it('saturates the fit box: the slab is overdraw, on any backend', () => {
    expect(edgeOverdrawCoverage({ ...scene, alpha: DEFAULT_ALPHA })).toBeGreaterThan(0.99);
  });

  it('still saturates at a quarter of the default alpha, as measured on WebGPU', () => {
    expect(edgeOverdrawCoverage({ ...scene, alpha: DEFAULT_ALPHA / 4 })).toBeGreaterThan(0.95);
  });

  it('alpha alone cannot fix it: half coverage needs a per-edge alpha (~0.004) within two 8-bit steps', () => {
    // A single edge at that alpha is at the edge of what an 8-bit target shows.
    expect(maxEdgeAlphaForCoverage(0.5, scene)).toBeLessThan(2 * ALPHA_8BIT_STEP);
  });

  it('at the default alpha the box reaches half coverage at a few thousand edges (the D4 density threshold)', () => {
    const n = maxEdgesForCoverage(0.5, { ...scene, alpha: DEFAULT_ALPHA });
    expect(n).toBeGreaterThan(2_000);
    expect(n).toBeLessThan(6_000);
  });
});
