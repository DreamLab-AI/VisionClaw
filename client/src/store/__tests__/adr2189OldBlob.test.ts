// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

// --- Mock all external dependencies before importing the store ---

vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

vi.mock('../../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false,
  },
}));

vi.mock('../../features/design-system/components/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock('../../features/settings/config/viewportSettings', () => ({
  isViewportSetting: vi.fn(() => false),
}));

const mockGetSettingsByPaths = vi.fn().mockResolvedValue({});
const mockGetSettingByPath = vi.fn().mockResolvedValue(undefined);
const mockUpdateSettingByPath = vi.fn().mockResolvedValue(undefined);
const mockUpdateSettingsByPaths = vi.fn().mockResolvedValue(undefined);
const mockFlushPendingUpdates = vi.fn().mockResolvedValue(undefined);
const mockResetSettings = vi.fn().mockResolvedValue(undefined);
const mockExportSettings = vi.fn((s: unknown) => JSON.stringify(s));
const mockImportSettings = vi.fn((json: string) => JSON.parse(json));

vi.mock('../../api/settingsApi', () => ({
  settingsApi: {
    getSettingsByPaths: (...args: unknown[]) => mockGetSettingsByPaths(...args),
    getSettingByPath: (...args: unknown[]) => mockGetSettingByPath(...args),
    updateSettingByPath: (...args: unknown[]) => mockUpdateSettingByPath(...args),
    updateSettingsByPaths: (...args: unknown[]) => mockUpdateSettingsByPaths(...args),
    flushPendingUpdates: (...args: unknown[]) => mockFlushPendingUpdates(...args),
    resetSettings: (...args: unknown[]) => mockResetSettings(...args),
    exportSettings: (...args: unknown[]) => mockExportSettings(...args),
    importSettings: (...args: unknown[]) => mockImportSettings(...args),
  },
}));

vi.mock('../../services/nostrAuthService', () => ({
  nostrAuth: {
    initialized: true,
    initialize: vi.fn().mockResolvedValue(undefined),
    isAuthenticated: vi.fn(() => false),
    getCurrentUser: vi.fn(() => null),
  },
}));

const mockQueueChange = vi.fn();
const mockQueueChanges = vi.fn();
vi.mock('../autoSaveManager', () => ({
  autoSaveManager: {
    setSyncEnabled: vi.fn(),
    setInitialized: vi.fn(),
    queueChange: (...args: unknown[]) => mockQueueChange(...args),
    queueChanges: (...args: unknown[]) => mockQueueChanges(...args),
  },
}));


// Need to import AFTER mocks are set up
import { useSettingsStore } from '../settingsStore';
import { ADR_2189_REMOVED_PATHS, stripRemovedPaths } from '../settings/retiredPaths';

/**
 * ADR-2189 deleted 40 settings nothing read. A browser that ran an older client
 * still holds them in `graph-viz-settings-v2`; the server document may still
 * return some (shared Rust GraphSettings). Both must keep loading, with the dead
 * keys dropped and every live key intact.
 */

const get = (root: unknown, path: string): unknown =>
  path.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), root);

/** A blob shaped exactly as an older client persisted it (no auth material). */
const OLD_BLOB = {
  state: {
    authenticated: false,
    user: null,
    isPowerUser: false,
    partialSettings: {
      visualisation: {
        graphs: {
          knowledge: {
            nodes: { baseColor: '#c25b5b', colorScheme: 'sssp', sizeScheme: 'fileSize', nodeSize: 0.25, perNodeGlow: true, enableMetadataShape: false },
            edges: { color: '#a30000', baseWidth: 0.04, opacity: 0.3, colorByType: true, widthByWeight: true },
            labels: { labelDistanceThreshold: 900, maxLabelWidth: 7, desktopFontSize: 0.4, textOutlineColor: '#00ff40', textOutlineWidth: 0.0074725277 },
          },
          visionclaw: {
            nodes: { baseColor: '#3498DB', nodeSize: 0.5, opacity: 0.9, metalness: 0.1, roughness: 0.6 },
            edges: { color: '#fff', opacity: 0.5, baseWidth: 0.1, enableArrows: false, colorByType: true },
            labels: { enableLabels: true, desktopFontSize: 0.4, textColor: '#fff' },
          },
        },
        graphTypeVisuals: {
          ontology: { ringTintByClass: true, glowStrength: 1.8 },
          agent: {
            swarmTint: true, membraneOpacity: 0.7, showHealthBar: true, beamRadius: 0.35,
            healthColors: { excellent: '#0f0', good: '#2c7', warning: '#f90', critical: '#e00' },
          },
        },
        embeddingCloud: { enabled: false },
      },
      qualityGates: { showClusters: true, showCommunities: true, showAnomalies: true, gnnPhysics: false, maxNodeCount: 9007199254740991 },
      perplexity: { model: 'sonar', maxTokens: 4000, temperature: 0.5 },
      pocketTts: { apiUrl: 'http://tts', defaultVoice: 'alba', defaultSpeed: 1 },
      xr: { enabled: false, quality: 'high', renderScale: 1, enableHandTracking: true, enableHaptics: true },
      decisions: { showDecisionChains: false, highlightPrecedents: false },
      provenance: { showAttribution: true },
    },
  },
  version: 0,
};

describe('ADR-2189: an old persisted settings blob still loads', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('rehydrates, drops every removed path and keeps every live one', async () => {
    localStorage.setItem('graph-viz-settings-v2', JSON.stringify(OLD_BLOB));
    await useSettingsStore.persist.rehydrate();
    const { settings, partialSettings, loadedPaths } = useSettingsStore.getState();

    for (const p of ADR_2189_REMOVED_PATHS) {
      expect(get(settings, p), p).toBeUndefined();
      expect(get(partialSettings, p), p).toBeUndefined();
      expect(loadedPaths.has(p), p).toBe(false);
    }
    // live values survive untouched
    expect(get(settings, 'visualisation.graphs.knowledge.nodes.baseColor')).toBe('#c25b5b');
    expect(get(settings, 'visualisation.graphs.knowledge.nodes.colorScheme')).toBe('sssp');
    expect(get(settings, 'visualisation.graphs.knowledge.edges.opacity')).toBe(0.3);
    expect(get(settings, 'visualisation.graphs.knowledge.labels.maxLabelWidth')).toBe(7);
    expect(get(settings, 'visualisation.graphs.visionclaw.nodes.baseColor')).toBe('#3498DB');
    expect(get(settings, 'visualisation.graphTypeVisuals.ontology.glowStrength')).toBe(1.8);
    expect(get(settings, 'visualisation.graphTypeVisuals.agent.beamRadius')).toBe(0.35);
    expect(get(settings, 'visualisation.embeddingCloud.enabled')).toBe(false);
    expect(get(settings, 'qualityGates.showAnomalies')).toBe(true);
    // merged: the generic knowledge edge colour moved to agent mode's own colour
    expect(get(settings, 'visualisation.graphTypeVisuals.agent.edgeColor')).toBe('#a30000');
    expect(get(settings, 'pocketTts.defaultVoice')).toBe('alba');
    expect(get(settings, 'provenance.showAttribution')).toBe(true);
    // branches the removal emptied are pruned, not left as {}
    expect(get(settings, 'xr')).toBeUndefined();
    expect(get(settings, 'decisions')).toBeUndefined();
    expect(get(settings, 'perplexity')).toBeUndefined();
    expect(get(settings, 'visualisation.graphs.visionclaw.edges')).toBeUndefined();
  });

  it('the next persist writes no removed path back to localStorage', async () => {
    localStorage.setItem('graph-viz-settings-v2', JSON.stringify(OLD_BLOB));
    await useSettingsStore.persist.rehydrate();
    // force a write through the persist middleware
    useSettingsStore.setState({ isPowerUser: false });
    const written = JSON.parse(localStorage.getItem('graph-viz-settings-v2') as string);
    for (const p of ADR_2189_REMOVED_PATHS) expect(get(written.state.partialSettings, p), p).toBeUndefined();
    expect(get(written.state.partialSettings, 'visualisation.graphs.knowledge.nodes.baseColor')).toBe('#c25b5b');
  });
});

describe('stripRemovedPaths is tolerant of any shape', () => {
  it('leaves non-objects and partial trees alone', () => {
    expect(stripRemovedPaths(null)).toBeNull();
    expect(stripRemovedPaths(undefined)).toBeUndefined();
    expect(stripRemovedPaths('x' as unknown)).toBe('x');
    expect(stripRemovedPaths({})).toEqual({});
    // a scalar where a branch is expected, and an array, are not walked into
    expect(stripRemovedPaths({ xr: 5, qualityGates: [1, 2] })).toEqual({ xr: 5, qualityGates: [1, 2] });
  });

  it('does not mutate its input', () => {
    const input = { qualityGates: { gnnPhysics: true, autoAdjust: true } };
    const out = stripRemovedPaths(input);
    expect(input.qualityGates.gnnPhysics).toBe(true);
    expect(out).toEqual({ qualityGates: { autoAdjust: true } });
  });

  it('a rename never overwrites a value already stored at the new path', async () => {
    const { stripRemovedPaths } = await import('../settings/retiredPaths');
    const blob = {
      visualisation: {
        graphs: { knowledge: { edges: { color: '#a30000', opacity: 0.3 } } },
        graphTypeVisuals: { agent: { edgeColor: '#00ff00' } },
      },
    };
    const out = stripRemovedPaths(blob);
    expect(get(out, 'visualisation.graphTypeVisuals.agent.edgeColor')).toBe('#00ff00');
    expect(get(out, 'visualisation.graphs.knowledge.edges.color')).toBeUndefined();
    expect(get(out, 'visualisation.graphs.knowledge.edges.opacity')).toBe(0.3);
    // the input is not mutated
    expect(blob.visualisation.graphs.knowledge.edges.color).toBe('#a30000');
  });
});
