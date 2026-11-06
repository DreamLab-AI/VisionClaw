import { describe, it, expect } from 'vitest';
import { anomalyRgb, centralityHsl, clusterHsl, communityHsl } from '../analyticColours';

// Characterisation: the formulas each copy carried before the merge
// (nodeEncoding, GemNodes, columnEncoder, its legend, ClusterHulls).
describe('analytic colour formulas (one copy)', () => {
  it('community and cluster hues are the old multiply-mod-360 hues', () => {
    for (const id of [1, 2, 7, 42, 431, 9999]) {
      expect(communityHsl(id)).toEqual([((id * 83) % 360) / 360, 0.65, 0.5]);
      expect(clusterHsl(id)).toEqual([((id * 137) % 360) / 360, 0.7, 0.55]);
    }
  });

  it('the centrality ramp runs blue → yellow and clamps like Math.min(t, 1)', () => {
    for (const t of [0, 0.1, 0.5, 0.9, 1]) expect(centralityHsl(t)).toEqual([0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t]);
    expect(centralityHsl(3)).toEqual(centralityHsl(1));
  });

  it('the anomaly overlay is the old red ramp', () => {
    for (const a of [0.02, 0.3, 1, 2]) {
      const t = Math.min(a, 1);
      expect(anomalyRgb(a)).toEqual([0.9 * t + 0.1, 0.15 * (1 - t), 0.1]);
    }
  });
});
