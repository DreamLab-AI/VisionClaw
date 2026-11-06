import { describe, it, expect } from 'vitest';
import { overviewEdgeOpacity, OVERVIEW_EDGE_BASE_OPACITY, EDGE_OPACITY_SLIDER_MAX } from '../edgeStyle';
import { edgeOpacityFromCommand } from '../../../visualisation/components/CommandInput';
import { serverBucketForPath } from '../../../../api/settings/schemaMappings';
import { getSectionPaths } from '../../../../store/settings/settingsHelpers';
import { DEFAULT_EDGES_SETTINGS } from '../../../../api/settings/defaults';
import { ALL_FIELDS } from '../../../control-center/registry/settingsRegistry';

const D = DEFAULT_EDGES_SETTINGS.opacity;

describe('overviewEdgeOpacity: the Edge Opacity setting drives the overview lines', () => {
  it('reproduces the shipped look at the shipped default', () => {
    expect(overviewEdgeOpacity(D, D)).toBeCloseTo(OVERVIEW_EDGE_BASE_OPACITY);
  });
  it('changes visibly when the setting changes', () => {
    expect(overviewEdgeOpacity(D / 2, D)).toBeCloseTo(OVERVIEW_EDGE_BASE_OPACITY / 2);
    expect(overviewEdgeOpacity(0, D)).toBe(0);
  });
  it('the whole slider range is effective: no dead top, still 0..1 at the ends', () => {
    const top = EDGE_OPACITY_SLIDER_MAX;
    const samples = Array.from({ length: 61 }, (_, i) => (top * i) / 60);
    const alphas = samples.map((v) => overviewEdgeOpacity(v, D));
    for (let i = 1; i < alphas.length; i++) expect(alphas[i], `at ${samples[i]}`).toBeGreaterThan(alphas[i - 1]);
    expect(overviewEdgeOpacity(top, D)).toBe(1);
    expect(overviewEdgeOpacity(top * 0.9, D)).toBeLessThan(1);
    // above the slider's top it saturates, never exceeds 1
    expect(overviewEdgeOpacity(top * 2, D)).toBe(1);
  });
  it('EDGE_OPACITY_SLIDER_MAX is the registry slider top', () => {
    expect(ALL_FIELDS.find((f) => f.path === 'visualisation.graphs.knowledge.edges.opacity')?.max).toBe(EDGE_OPACITY_SLIDER_MAX);
  });
  it('falls back to the base alpha for a missing or bad setting', () => {
    expect(overviewEdgeOpacity(undefined, D)).toBe(OVERVIEW_EDGE_BASE_OPACITY);
    expect(overviewEdgeOpacity(Number.NaN, D)).toBe(OVERVIEW_EDGE_BASE_OPACITY);
  });
});

describe('edgeOpacityFromCommand: chat edge requests target edges', () => {
  it('ignores commands that are not about edges', () => {
    expect(edgeOpacityFromCommand('make nodes transparent')).toBeNull();
    expect(edgeOpacityFromCommand('make edges thicker')).toBeNull();
  });
  it('reads transparency as the inverse of opacity, in slider units', () => {
    expect(edgeOpacityFromCommand('set edge transparency to 70%')).toBeCloseTo(0.09);
    expect(edgeOpacityFromCommand('edge opacity 50%')).toBeCloseTo(0.15);
    expect(edgeOpacityFromCommand('make the edges 100% transparent')).toBe(0);
  });
  it('handles relative wording without a number', () => {
    expect(edgeOpacityFromCommand('make the links fainter')).toBeCloseTo(D * 0.5);
    expect(edgeOpacityFromCommand('make edges more opaque')).toBeCloseTo(D * 1.6);
  });
});

describe('getSectionPaths("visual"): a visual broadcast re-reads edge opacity', () => {
  it('covers the edges section and never physics or the server rendering document', () => {
    const paths = getSectionPaths('visual');
    expect(paths).toContain('visualisation.graphs.knowledge.edges');
    // only the rendering keys the visual bucket holds (agent colours, budgets), never the
    // RenderingSettings fields /api/settings/rendering owns
    expect(paths.filter((p) => p.startsWith('visualisation.rendering')).every((p) => serverBucketForPath(p) === 'visual')).toBe(true);
    expect(paths).not.toContain('visualisation.rendering.ambientLightIntensity');
    expect(paths.some((p) => p.includes('.physics'))).toBe(false);
  });
});
