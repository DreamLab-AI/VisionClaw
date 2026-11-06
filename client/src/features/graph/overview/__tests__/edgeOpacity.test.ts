import { describe, it, expect } from 'vitest';
import { overviewEdgeOpacity, OVERVIEW_EDGE_BASE_OPACITY } from '../edgeStyle';
import { edgeOpacityFromCommand } from '../../../visualisation/components/CommandInput';
import { serverBucketForPath } from '../../../../api/settings/schemaMappings';
import { getSectionPaths } from '../../../../store/settings/settingsHelpers';
import { DEFAULT_EDGES_SETTINGS } from '../../../../api/settings/defaults';

const D = DEFAULT_EDGES_SETTINGS.opacity;

describe('overviewEdgeOpacity: the Edge Opacity setting drives the overview lines', () => {
  it('reproduces the shipped look at the shipped default', () => {
    expect(overviewEdgeOpacity(D, D)).toBeCloseTo(OVERVIEW_EDGE_BASE_OPACITY);
  });
  it('changes visibly when the setting changes', () => {
    expect(overviewEdgeOpacity(D / 2, D)).toBeCloseTo(OVERVIEW_EDGE_BASE_OPACITY / 2);
    expect(overviewEdgeOpacity(0, D)).toBe(0);
    expect(overviewEdgeOpacity(0.3, D)).toBe(1);
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
