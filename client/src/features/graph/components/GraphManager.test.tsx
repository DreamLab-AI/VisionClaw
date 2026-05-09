import { describe, it, expect, vi } from 'vitest';

// GraphManager.tsx is ~1050 lines with heavy Three.js/R3F dependencies.
// These tests verify the module can be loaded and its key exports exist.
// Full rendering tests require a Canvas/GPU context.

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

vi.mock('three', async () => {
  const actual = await vi.importActual<typeof import('three')>('three');
  return actual;
});

vi.mock('@react-three/fiber', () => ({
  useFrame: vi.fn(),
  useThree: vi.fn(() => ({ camera: {}, gl: {}, scene: {} })),
  Canvas: vi.fn(),
}));

vi.mock('@react-three/drei', () => ({
  Text: vi.fn(),
  Html: vi.fn(),
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: () => ({
    settings: {
      visualisation: {
        graphs: {
          logseq: {
            nodes: {},
            edges: {},
            labels: {},
            physics: {},
          },
        },
      },
    },
  }),
}));

vi.mock('../../settings/config/settings', () => ({
  getDefaultSettings: vi.fn(() => ({})),
}));

// Blanket mock for any remaining internal imports that would fail in jsdom
vi.mock('../managers/graphDataManager', () => ({
  graphDataManager: {
    nodeIdMap: new Map(),
    edgeIdMap: new Map(),
    getGraphData: vi.fn(() => ({ nodes: [], edges: [] })),
    setGraphData: vi.fn(),
    subscribe: vi.fn(() => vi.fn()),
  },
}));

vi.mock('../../../store/websocketStore', () => ({
  useWebSocketStore: () => ({
    isConnected: false,
    on: vi.fn(() => vi.fn()),
    onBinaryMessage: vi.fn(() => vi.fn()),
  }),
}));

vi.mock('../../../rendering/rendererFactory', () => ({
  isWebGPURenderer: () => false,
  rendererCapabilities: { webgpu: false },
}));

describe('GraphManager module', () => {
  it('can be imported without error', async () => {
    // The module should not throw on import even in jsdom
    const mod = await import('./GraphManager');
    expect(mod).toBeDefined();
  });

  it('exports a GraphManager component', async () => {
    const mod = await import('./GraphManager');
    // Default or named export
    const Component = mod.default || (mod as any).GraphManager;
    expect(Component).toBeDefined();
  });
});
