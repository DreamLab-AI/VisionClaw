// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

// --- Mock all external dependencies before importing the module under test ---

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

vi.mock('../../../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false,
  },
}));

const mockIsRestLoadInFlight = vi.fn(() => false);
const mockOfferServerGraphLoad = vi.fn().mockResolvedValue('applied');
const { mockNodeIdMap } = vi.hoisted(() => ({ mockNodeIdMap: new Map<string, number>() }));
vi.mock('../../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: {
    nodeIdMap: mockNodeIdMap,
    fetchInitialData: vi.fn().mockResolvedValue(undefined),
    setGraphData: vi.fn().mockResolvedValue(undefined),
    isRestLoadInFlight: () => mockIsRestLoadInFlight(),
    offerServerGraphLoad: (...a: unknown[]) => mockOfferServerGraphLoad(...a),
  },
}));

vi.mock('../../../features/ontology/store/useInferredEdgesStore', () => ({
  useInferredEdgesStore: {
    getState: () => ({
      refresh: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

const mockSet = vi.fn();
const mockGetSectionPaths = vi.fn();
vi.mock('../../settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({
      settings: {},
      set: mockSet,
    }),
  },
  settingsStoreUtils: {
    getSectionPaths: (...args: unknown[]) => mockGetSectionPaths(...args),
  },
}));

const mockGetCurrentUser = vi.fn();
vi.mock('../../../services/nostrAuthService', () => ({
  nostrAuth: {
    getCurrentUser: (...args: unknown[]) => mockGetCurrentUser(...args),
  },
}));

const mockGetSettingsByPaths = vi.fn();
vi.mock('../../../api/settingsApi', () => ({
  settingsApi: {
    getSettingsByPaths: (...args: unknown[]) => mockGetSettingsByPaths(...args),
  },
}));

const mockEmit = vi.fn();
const mockNotifyMessageHandlers = vi.fn();
vi.mock('../connectionManager', () => ({
  emit: (...args: unknown[]) => mockEmit(...args),
  notifyMessageHandlers: (...args: unknown[]) => mockNotifyMessageHandlers(...args),
}));

vi.mock('../binaryProtocol', () => ({
  handleErrorFrame: vi.fn(),
}));

const mockIsFilterResponseExpected = vi.fn(() => false);
const mockClearFilterResponseExpectation = vi.fn();
vi.mock('../filterSync', () => ({
  isFilterResponseExpected: () => mockIsFilterResponseExpected(),
  clearFilterResponseExpectation: () => mockClearFilterResponseExpectation(),
}));

// Need to import AFTER mocks are set up
import { handleTextMessage } from '../textMessageHandler';

const noopGet = () => ({ forceReconnect: vi.fn() });
const noopSet = vi.fn();
const noopProcessQueue = vi.fn();

const CURRENT_USER_PUBKEY = 'aaaa000000000000000000000000000000000000000000000000000000aa';
const OTHER_PUBKEY = 'bbbb111111111111111111111111111111111111111111111111111111bb';

function dispatch(message: Record<string, unknown>) {
  handleTextMessage(
    message as never,
    noopGet as never,
    noopSet as never,
    noopProcessQueue as never,
  );
}

describe('handleTextMessage — settingsUpdated (ADR-2047)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetCurrentUser.mockReturnValue({ pubkey: CURRENT_USER_PUBKEY });
    mockGetSectionPaths.mockImplementation((section: string) => {
      if (section === 'physics') return ['visualisation.graphs.knowledge.physics'];
      if (section === 'rendering') return ['visualisation.rendering.ambientLightIntensity'];
      if (section === 'nodeFilter') return ['nodeFilter'];
      return [];
    });
  });

  it('applies the supplied settings object directly for the nodeFilter category', () => {
    const nodeFilterSettings = {
      enabled: true,
      qualityThreshold: 0.5,
      authorityThreshold: 0.3,
      filterByQuality: true,
      filterByAuthority: false,
      filterMode: 'and',
      includeLinkedPages: true,
    };

    dispatch({
      type: 'settingsUpdated',
      category: 'nodeFilter',
      updatedBy: OTHER_PUBKEY,
      timestamp: 1000,
      settings: nodeFilterSettings,
    });

    expect(mockSet).toHaveBeenCalledWith('nodeFilter', nodeFilterSettings, true);
    // No follow-up re-read should happen for nodeFilter.
    expect(mockGetSettingsByPaths).not.toHaveBeenCalled();
  });

  it('re-reads the category from the settings API for a physics update', async () => {
    mockGetSettingsByPaths.mockResolvedValue({
      visualisation: { graphs: { knowledge: { physics: { springK: 0.42 } } } },
    });

    dispatch({
      type: 'settingsUpdated',
      category: 'physics',
      updatedBy: OTHER_PUBKEY,
      timestamp: 1000,
    });

    expect(mockGetSectionPaths).toHaveBeenCalledWith('physics');
    expect(mockGetSettingsByPaths).toHaveBeenCalledWith(['visualisation.graphs.knowledge.physics']);

    // Allow the settingsApi promise to resolve.
    await Promise.resolve();
    await Promise.resolve();

    expect(mockSet).toHaveBeenCalledWith(
      'visualisation.graphs.knowledge.physics',
      { springK: 0.42 },
      true,
    );
  });

  it('ignores the echo of the current user\'s own write', () => {
    dispatch({
      type: 'settingsUpdated',
      category: 'rendering',
      updatedBy: CURRENT_USER_PUBKEY,
      timestamp: 1000,
    });

    expect(mockGetSettingsByPaths).not.toHaveBeenCalled();
    expect(mockSet).not.toHaveBeenCalled();
  });

  it('ignores a stale settingsUpdated message (older timestamp than last applied)', async () => {
    mockGetSettingsByPaths.mockResolvedValue({
      visualisation: { rendering: { ambientLightIntensity: 1.5 } },
    });

    // First message establishes the "last applied" timestamp for 'rendering'.
    dispatch({
      type: 'settingsUpdated',
      category: 'rendering',
      updatedBy: OTHER_PUBKEY,
      timestamp: 2000,
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(mockGetSettingsByPaths).toHaveBeenCalledTimes(1);

    mockGetSettingsByPaths.mockClear();
    mockSet.mockClear();

    // A second message with an older timestamp must be dropped.
    dispatch({
      type: 'settingsUpdated',
      category: 'rendering',
      updatedBy: OTHER_PUBKEY,
      timestamp: 1000,
    });

    expect(mockGetSettingsByPaths).not.toHaveBeenCalled();
    expect(mockSet).not.toHaveBeenCalled();
  });
});

describe('handleTextMessage — initialGraphLoad defers to an in-flight REST load', () => {
  const capped = {
    type: 'initialGraphLoad',
    nodes: [{ id: 0, label: 'a' }, { id: 1, label: 'b' }],
    edges: [{ id: '0-1', source: 0, target: 1 }],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockNodeIdMap.clear();
    mockIsRestLoadInFlight.mockReturnValue(false);
    mockIsFilterResponseExpected.mockReturnValue(false);
    mockOfferServerGraphLoad.mockResolvedValue('applied');
  });

  it('hands the capped load to the manager instead of seeding the topology while REST is in flight', () => {
    mockIsRestLoadInFlight.mockReturnValue(true);
    mockOfferServerGraphLoad.mockResolvedValue('deferred');
    dispatch(capped);
    expect(mockOfferServerGraphLoad).toHaveBeenCalledTimes(1);
    const [data, opts] = mockOfferServerGraphLoad.mock.calls[0];
    expect((data as { nodes: unknown[] }).nodes).toHaveLength(2);
    expect(opts).toEqual({ isFilterResponse: false });
  });

  it('marks the load as a filter response and consumes the expectation while REST is in flight', () => {
    mockIsRestLoadInFlight.mockReturnValue(true);
    mockIsFilterResponseExpected.mockReturnValue(true);
    dispatch(capped);
    expect(mockClearFilterResponseExpectation).toHaveBeenCalledTimes(1);
    expect(mockOfferServerGraphLoad.mock.calls[0][1]).toEqual({ isFilterResponse: true });
  });

  it('still skips a smaller unsolicited load once REST has settled', () => {
    for (let i = 0; i < 9; i++) mockNodeIdMap.set(String(i), i);
    dispatch(capped);
    expect(mockOfferServerGraphLoad).not.toHaveBeenCalled();
  });
});
