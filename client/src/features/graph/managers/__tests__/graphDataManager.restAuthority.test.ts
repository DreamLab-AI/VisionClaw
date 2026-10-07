// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * REST topology is authoritative at startup.
 *
 * Defect: the server pushes a capped (3,000-node) `initialGraphLoad` on WS
 * connect. When it lands while the full REST `/graph/data` load (9,473 nodes,
 * ~38 MB) is still in flight, it seeded the topology; the next full binary
 * frame then carried 6,473 ids the client did not know until REST settled.
 *
 * Rule under test: while a REST load is in flight, a server-pushed graph load
 * never replaces or seeds the topology. It is held until REST settles, then
 *   - dropped when REST succeeded (REST is the full picture), unless it is
 *     the answer to our own filter_update, which narrows REST and so lands;
 *   - applied when REST failed, so the client still shows something.
 */

vi.mock('../../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));
vi.mock('../../../../utils/clientDebugState', () => ({
  debugState: { isEnabled: () => false, isDataDebugEnabled: () => false },
}));
vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({ settings: { qualityGates: {}, system: { debug: {} } }, subscribe: vi.fn() }),
    subscribe: vi.fn(),
  },
}));
const mockSetGraphTopology = vi.fn().mockResolvedValue(undefined);
vi.mock('../graphWorkerProxy', () => ({
  graphWorkerProxy: {
    isReady: () => true,
    setGraphTopology: (...a: unknown[]) => mockSetGraphTopology(...a),
    processBinaryFrame: vi.fn().mockResolvedValue(undefined),
    onGraphDataChange: vi.fn(() => vi.fn()),
    onPositionUpdate: vi.fn(() => vi.fn()),
  },
}));
vi.mock('../../../../services/BinaryWebSocketProtocol', () => ({
  binaryProtocol: { setUserInteracting: vi.fn() },
}));
vi.mock('../../../../store/workerErrorStore', () => ({
  useWorkerErrorStore: {
    getState: () => ({ setWorkerError: vi.fn(), resetTransientErrors: vi.fn(), recordTransientError: vi.fn() }),
  },
}));
vi.mock('react', () => ({ startTransition: (fn: () => void) => fn() }));

type Deferred = { resolve: (v: unknown) => void; reject: (e: unknown) => void };
let pendingRest: Deferred | null = null;
vi.mock('../dataManager/restClient', () => ({
  fetchGraphData: vi.fn(
    () => new Promise((resolve, reject) => { pendingRest = { resolve, reject }; }),
  ),
  scheduleEmptyDataRetry: vi.fn(),
}));

import { graphDataManager } from '../graphDataManager';

const graph = (n: number) => ({
  nodes: Array.from({ length: n }, (_, i) => ({
    id: String(i), label: `n${i}`, position: { x: i, y: 0, z: 0 },
  })),
  edges: [],
});

const flush = () => new Promise(r => setTimeout(r, 0));

describe('GraphDataManager — REST topology is authoritative at startup', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    pendingRest = null;
    await graphDataManager.setGraphData({ nodes: [], edges: [] });
    mockSetGraphTopology.mockClear();
  });

  it('applies a server graph load at once when no REST load is in flight', async () => {
    const outcome = await graphDataManager.offerServerGraphLoad(graph(3), { isFilterResponse: false });
    expect(outcome).toBe('applied');
    expect(graphDataManager.nodeIdMap.size).toBe(3);
  });

  it('never seeds the topology with a capped load while REST is in flight', async () => {
    const rest = graphDataManager.fetchInitialData();
    await flush();
    expect(graphDataManager.isRestLoadInFlight()).toBe(true);

    const outcome = await graphDataManager.offerServerGraphLoad(graph(3), { isFilterResponse: false });
    expect(outcome).toBe('deferred');
    expect(graphDataManager.nodeIdMap.size).toBe(0);
    expect(mockSetGraphTopology).not.toHaveBeenCalled();

    pendingRest!.resolve(graph(9));
    await rest;
    expect(graphDataManager.isRestLoadInFlight()).toBe(false);
    expect(graphDataManager.nodeIdMap.size).toBe(9);
    // The capped load was dropped, not applied after REST.
    expect(mockSetGraphTopology).toHaveBeenCalledTimes(1);
  });

  it('never replaces an existing topology with a capped load while a REST refetch is in flight', async () => {
    await graphDataManager.setGraphData(graph(9));
    const rest = graphDataManager.fetchInitialData();
    await flush();

    await graphDataManager.offerServerGraphLoad(graph(3), { isFilterResponse: false });
    expect(graphDataManager.nodeIdMap.size).toBe(9);

    pendingRest!.resolve(graph(9));
    await rest;
    expect(graphDataManager.nodeIdMap.size).toBe(9);
  });

  it('applies a filter response after REST settles, so the narrower filtered graph wins', async () => {
    const rest = graphDataManager.fetchInitialData();
    await flush();

    const outcome = await graphDataManager.offerServerGraphLoad(graph(4), { isFilterResponse: true });
    expect(outcome).toBe('deferred');
    expect(graphDataManager.nodeIdMap.size).toBe(0);

    pendingRest!.resolve(graph(9));
    await rest;
    expect(graphDataManager.nodeIdMap.size).toBe(4);
  });

  it('keeps only the latest held load', async () => {
    const rest = graphDataManager.fetchInitialData();
    await flush();
    await graphDataManager.offerServerGraphLoad(graph(4), { isFilterResponse: true });
    await graphDataManager.offerServerGraphLoad(graph(5), { isFilterResponse: true });

    pendingRest!.resolve(graph(9));
    await rest;
    expect(graphDataManager.nodeIdMap.size).toBe(5);
  });

  it('falls back to the held load when the REST load fails', async () => {
    const rest = graphDataManager.fetchInitialData();
    await flush();
    await graphDataManager.offerServerGraphLoad(graph(3), { isFilterResponse: false });

    pendingRest!.reject(new Error('network'));
    await expect(rest).rejects.toThrow('network');
    expect(graphDataManager.isRestLoadInFlight()).toBe(false);
    expect(graphDataManager.nodeIdMap.size).toBe(3);
  });
});
