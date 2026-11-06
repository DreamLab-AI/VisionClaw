// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, vi } from 'vitest';

/**
 * ADR-2189: the server's stored `/visual` document (written by older clients) and
 * its shared Rust structs can still return settings the menus deleted. They are
 * dropped at the API boundary so they never re-enter the store or localStorage.
 */

const get = vi.fn();
vi.mock('axios', () => ({
  default: {
    get: (...a: unknown[]) => get(...a),
    put: vi.fn(),
    post: vi.fn(),
    interceptors: { request: { use: vi.fn() }, response: { use: vi.fn() } },
  },
}));

import { getSettingsByPaths } from '../endpoints';
import { ADR_2189_REMOVED_PATHS } from '../../../store/settings/retiredPaths';

const at = (root: unknown, path: string): unknown =>
  path.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), root);

describe('getSettingsByPaths drops ADR-2189 removed paths from the server document', () => {
  it('keeps live values and removes dead ones', async () => {
    get.mockResolvedValue({
      data: {
        visual: {
          nodes: { baseColor: '#123456', perNodeGlow: true, enableMetadataShape: true, sizeScheme: 'degree' },
          edges: { opacity: 0.2, colorByType: false, widthByWeight: false },
          labels: { desktopFontSize: 0.5, textOutlineWidth: 0.01 },
          graphTypeVisuals: { ontology: { ringTintByClass: false, glowStrength: 2 }, agent: { swarmTint: false, beamOpacity: 0.5 } },
        },
        qualityGates: { autoAdjust: false, gnnPhysics: true, maxNodeCount: 5000 },
      },
    });
    const all = await getSettingsByPaths(['visualisation', 'qualityGates']);
    for (const p of ADR_2189_REMOVED_PATHS) expect(at(all, p), p).toBeUndefined();
    expect(at(all, 'visualisation.graphs.knowledge.nodes.baseColor')).toBe('#123456');
    expect(at(all, 'visualisation.graphs.knowledge.edges.opacity')).toBe(0.2);
    expect(at(all, 'visualisation.graphs.knowledge.labels.desktopFontSize')).toBe(0.5);
    expect(at(all, 'visualisation.graphTypeVisuals.ontology.glowStrength')).toBe(2);
    expect(at(all, 'visualisation.graphTypeVisuals.agent.beamOpacity')).toBe(0.5);
    expect(at(all, 'qualityGates.autoAdjust')).toBe(false);
  });
});
