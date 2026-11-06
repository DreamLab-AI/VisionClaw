// Settings integrity: every registry path must reach a server bucket that keeps it,
// and come back on the next load. Each case below reproduces a loss the live round
// trip of 2026-11-06 found (21 of 132 writes did not survive).
import { describe, it, expect, vi, beforeEach } from 'vitest';

const put = vi.fn();
const get = vi.fn();
vi.mock('axios', () => {
  const ax = {
    get: (...a: unknown[]) => get(...a),
    put: (...a: unknown[]) => put(...a),
    post: vi.fn().mockResolvedValue({ data: {} }),
    interceptors: { request: { use: vi.fn() } },
  };
  return { default: ax, ...ax };
});
vi.mock('../../../services/nostrAuthService', () => ({
  nostrAuth: { isAuthenticated: () => false, getCurrentUser: () => null },
}));

import {
  updateSettingsByPaths,
  updateSettingByPath,
  invalidateSettingsCache,
  getSettingsByPaths,
  getStoredServerPaths,
} from '../endpoints';
import { transformApiToClientSettings, getNestedValue, serverBucketForPath } from '../schemaMappings';
import { ALL_FIELDS } from '../../../features/control-center/registry/settingsRegistry';
import { serverBucketFor } from '../../../features/control-center/registry/manifest';
import { getSectionPaths } from '../../../store/settings/settingsHelpers';
import type { AllSettings } from '../types';

const RENDERING_DOC = {
  ambientLightIntensity: 0.5, backgroundColor: '#000000', directionalLightIntensity: 0.4,
  enableAmbientOcclusion: false, enableAntialiasing: true, enableShadows: false, environmentIntensity: 0.3,
};

beforeEach(() => {
  put.mockReset().mockResolvedValue({ data: {} });
  get.mockReset().mockImplementation(async (url: string) => {
    if (url.endsWith('/rendering')) return { data: RENDERING_DOC };
    return { data: {} };
  });
  invalidateSettingsCache();
});

const putBody = (suffix: string) => put.mock.calls.find((c) => String(c[0]).endsWith(suffix))?.[1];

describe('rendering paths the server RenderingSettings struct does not have', () => {
  it('agent colours keep their nesting and go to the visual bucket, not flattened into rendering', async () => {
    await updateSettingsByPaths([
      { path: 'visualisation.rendering.agentColors.coder', value: '#13579b' },
      { path: 'visualisation.rendering.labelLayoutEvery', value: 4 },
      { path: 'visualisation.rendering.ambientLightIntensity', value: 0.7 },
    ]);
    const rendering = putBody('/api/settings/rendering');
    expect(rendering).toMatchObject({ ambientLightIntensity: 0.7 });
    expect(rendering).not.toHaveProperty('coder');
    expect(rendering).not.toHaveProperty('labelLayoutEvery');
    // agent colours are shared; the label-layout cadence is a per-device budget and stays in
    // this browser (one weak laptop must not change it for every viewer)
    expect(putBody('/api/settings/visual')).toEqual({ rendering: { agentColors: { coder: '#13579b' } } });
  });

  it('the per-device performance budgets have no server bucket', () => {
    for (const k of ['maxEdgesCeiling', 'softwareFallback', 'labelLayoutEvery']) {
      expect(serverBucketForPath(`visualisation.rendering.${k}`), k).toBeNull();
      expect(serverBucketFor(`visualisation.rendering.${k}`), k).toBeNull();
    }
  });

  it('the single-path writer routes the same way', async () => {
    await updateSettingByPath('visualisation.rendering.agentColors.queen', '#2468ac');
    expect(putBody('/api/settings/visual')).toEqual({ rendering: { agentColors: { queen: '#2468ac' } } });
    expect(putBody('/api/settings/rendering')).toBeUndefined();
  });

  it('they are read back from the visual bucket on load, under the server rendering document', () => {
    const client = transformApiToClientSettings({
      rendering: RENDERING_DOC,
      visual: { rendering: { agentColors: { coder: '#13579b' }, ambientLightIntensity: 9 } },
    } as unknown as AllSettings);
    expect(getNestedValue(client, 'visualisation.rendering.agentColors.coder')).toBe('#13579b');
    // the server's own rendering document wins for the keys it owns
    expect(getNestedValue(client, 'visualisation.rendering.ambientLightIntensity')).toBe(0.5);
  });
});

describe('visual-bucket paths outside the categories the loader reads', () => {
  it('tweening and the agent node colour are read back', () => {
    const client = transformApiToClientSettings({
      visual: {
        graphs: {
          knowledge: { tweening: { enabled: false, responseMs: 190 } },
          visionclaw: { nodes: { baseColor: '#13579b' } },
          logseq: { tweening: { lerpBase: 0.0001 } },
        },
      },
    } as unknown as AllSettings);
    expect(getNestedValue(client, 'visualisation.graphs.knowledge.tweening.responseMs')).toBe(190);
    expect(getNestedValue(client, 'visualisation.graphs.knowledge.tweening.enabled')).toBe(false);
    expect(getNestedValue(client, 'visualisation.graphs.visionclaw.nodes.baseColor')).toBe('#13579b');
    // the stale pre-ADR-2041 subtree on the server is not resurrected
    expect(getNestedValue(client, 'visualisation.graphs.logseq')).toBeUndefined();
  });
});

describe('a failed single-path write is reported, not swallowed', () => {
  it('rejects so the caller (retry manager, setByPath) can record the loss', async () => {
    put.mockRejectedValueOnce(Object.assign(new Error('boom'), { response: { status: 500 } }));
    await expect(updateSettingByPath('visualisation.glow.intensity', 0.5)).rejects.toThrow('boom');
  });
});

describe('every registry path has exactly one bucket, and the IA agrees with the writer', () => {
  it('serverBucketFor (manifest) matches the routing updateSettingsByPaths uses', () => {
    const mismatches = ALL_FIELDS.filter((f) => f.path)
      .map((f) => [f.path!, serverBucketFor(f.path), serverBucketForPath(f.path!)] as const)
      .filter(([, ia, writer]) => ia !== writer);
    expect(mismatches).toEqual([]);
  });

  it("a 'visual' broadcast re-reads every registry path stored in the visual bucket", () => {
    const sections = getSectionPaths('visual');
    const uncovered = ALL_FIELDS.filter((f) => f.path && serverBucketForPath(f.path) === 'visual')
      .map((f) => f.path!)
      .filter((p) => !sections.some((s) => p === s || p.startsWith(`${s}.`)));
    expect(uncovered).toEqual([]);
  });
});

describe('the settings cache can be invalidated (a broadcast must not read a 2 s-old copy)', () => {
  it('re-fetches after invalidateSettingsCache', async () => {
    get.mockResolvedValueOnce({ data: { visual: { glow: { intensity: 0.1 } } } });
    const a = await getSettingsByPaths(['visualisation.glow']);
    get.mockResolvedValueOnce({ data: { visual: { glow: { intensity: 0.9 } } } });
    invalidateSettingsCache();
    const b = await getSettingsByPaths(['visualisation.glow']);
    expect(getNestedValue(a, 'visualisation.glow.intensity')).toBe(0.1);
    expect(getNestedValue(b, 'visualisation.glow.intensity')).toBe(0.9);
  });

  it('records which client paths the server actually stores', async () => {
    get.mockResolvedValueOnce({
      data: { rendering: RENDERING_DOC, visual: { edges: { opacity: 0.12 }, graphView: { nodes: { spriteSizePx: 6.5 } } } },
    });
    await getSettingsByPaths(['visualisation']);
    const stored = getStoredServerPaths();
    expect(stored.has('visualisation.graphs.knowledge.edges.opacity')).toBe(true);
    expect(stored.has('visualisation.graphView.nodes.spriteSizePx')).toBe(true);
    expect(stored.has('visualisation.rendering.ambientLightIntensity')).toBe(true);
    // a default the loader filled in is not a stored value
    expect(stored.has('visualisation.glow.intensity')).toBe(false);
  });
});
