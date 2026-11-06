// What survives a reload. The server bucket is one shared document for every viewer,
// so a value it stores must win over an older copy this browser kept; a value this
// browser changed and could not save, a browser-only setting, and a value the server
// has never stored must not be thrown away.
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));
vi.mock('../../utils/clientDebugState', () => ({ debugState: { isEnabled: () => false, isDataDebugEnabled: () => false } }));
vi.mock('../../features/settings/config/viewportSettings', () => ({ isViewportSetting: vi.fn(() => false) }));
vi.mock('../../services/nostrAuthService', () => ({
  nostrAuth: {
    initialized: true,
    initialize: vi.fn().mockResolvedValue(undefined),
    isAuthenticated: vi.fn(() => false),
    getCurrentUser: vi.fn(() => null),
  },
}));

const server = { doc: {} as Record<string, unknown>, stored: new Set<string>() };
vi.mock('../../api/settingsApi', () => ({
  settingsApi: {
    getSettingsByPaths: vi.fn(async () => structuredClone(server.doc)),
    getStoredServerPaths: () => server.stored,
    updateSettingsByPaths: vi.fn().mockResolvedValue(undefined),
    updateSettingByPath: vi.fn().mockResolvedValue(undefined),
  },
}));

const OPACITY = 'visualisation.graphs.knowledge.edges.opacity';

/** The server: edges.opacity 0.12 and graphView.nodes.spriteSizePx 6.5 stored, the rest defaults. */
function seedServer() {
  server.doc = {
    visualisation: {
      graphView: { nodes: { spriteSizePx: 6.5, gemLodDistance: 250 } },
      graphs: { knowledge: { edges: { opacity: 0.12, baseWidth: 0.1 }, nodes: { nodeSize: 1 } } },
      rendering: { ambientLightIntensity: 0.5 },
    },
  };
  server.stored = new Set([
    OPACITY,
    'visualisation.graphView.nodes.spriteSizePx',
    'visualisation.rendering.ambientLightIntensity',
  ]);
}

/** A reload: persisted browser state, then a fresh store module that rehydrates it. */
async function reload(persisted: Record<string, unknown> | null, unsynced: Record<string, string> = {}) {
  localStorage.clear();
  if (persisted) {
    localStorage.setItem('graph-viz-settings-v2', JSON.stringify({ state: { partialSettings: persisted }, version: 0 }));
  }
  localStorage.setItem('graph-viz-settings-unsynced', JSON.stringify({ state: { unsaved: unsynced }, version: 0 }));
  vi.resetModules();
  const { useSettingsStore } = await import('../settingsStore');
  await useSettingsStore.getState().initialize();
  return useSettingsStore.getState();
}

describe('settings load precedence', () => {
  beforeEach(seedServer);

  it('a fresh browser reads server values through get(), not the shipped defaults', async () => {
    const s = await reload(null);
    expect(s.get<{ nodes: { spriteSizePx: number } }>('visualisation.graphView')?.nodes.spriteSizePx).toBe(6.5);
    expect(s.get<number>(OPACITY)).toBe(0.12);
  });

  it("a stale browser copy of a server-stored value loses to the server (another viewer's change)", async () => {
    const s = await reload({ visualisation: { graphs: { knowledge: { edges: { opacity: 0.3 } } } } });
    expect(s.get<number>(OPACITY)).toBe(0.12);
  });

  it('a value whose save failed or was dropped survives the reload', async () => {
    const s = await reload(
      { visualisation: { graphs: { knowledge: { edges: { opacity: 0.3 } } } } },
      { [OPACITY]: 'failed' },
    );
    expect(s.get<number>(OPACITY)).toBe(0.3);
  });

  it('a browser-only setting survives the reload', async () => {
    const s = await reload({ provenance: { showAttribution: true } });
    expect(s.get<boolean>('provenance.showAttribution')).toBe(true);
  });

  it('a server-bucket value the server has never stored survives (it was never superseded)', async () => {
    const s = await reload({ visualisation: { rendering: { agentColors: { coder: '#13579b' } } } });
    expect(s.get<string>('visualisation.rendering.agentColors.coder')).toBe('#13579b');
    // ...while the server's own rendering value still wins
    expect(s.get<number>('visualisation.rendering.ambientLightIntensity')).toBe(0.5);
  });

  it('a retired path in an old browser blob is dropped on load (node opacity has no renderer)', async () => {
    const s = await reload({ visualisation: { graphs: { knowledge: { nodes: { opacity: 0.4, nodeSize: 2 } } } } });
    expect(s.get<number>('visualisation.graphs.knowledge.nodes.opacity')).toBeUndefined();
  });
});
