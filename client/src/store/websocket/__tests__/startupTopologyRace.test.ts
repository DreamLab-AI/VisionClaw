// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * Integration: the real WS text handler and the real GraphDataManager.
 *
 * Reproduces the live startup race: the REST `/graph/data` load (full graph)
 * is in flight when the server's capped connect-time `initialGraphLoad`
 * arrives. The topology the worker resolves binary ids against must never be
 * the capped set; once REST settles it must be the full REST graph.
 * Only the network edge (restClient) and the worker proxy are stubbed.
 */

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));
vi.mock('../../../utils/clientDebugState', () => ({
  debugState: { isEnabled: () => false, isDataDebugEnabled: () => false },
}));

const topologies: number[] = [];
vi.mock('../../../features/graph/managers/graphWorkerProxy', () => ({
  graphWorkerProxy: {
    isReady: () => true,
    setGraphTopology: vi.fn(async (g: { nodes: unknown[] }) => { topologies.push(g.nodes.length); }),
    processBinaryFrame: vi.fn().mockResolvedValue(undefined),
    onGraphDataChange: vi.fn(() => vi.fn()),
    onPositionUpdate: vi.fn(() => vi.fn()),
  },
}));

type Deferred = { resolve: (v: unknown) => void; reject: (e: unknown) => void };
let pendingRest: Deferred | null = null;
vi.mock('../../../features/graph/managers/dataManager/restClient', () => ({
  fetchGraphData: vi.fn(() => new Promise((resolve, reject) => { pendingRest = { resolve, reject }; })),
  scheduleEmptyDataRetry: vi.fn(),
}));

vi.mock('../connectionManager', () => ({ emit: vi.fn(), notifyMessageHandlers: vi.fn() }));

import { graphDataManager } from '../../../features/graph/managers/graphDataManager';
import { handleTextMessage } from '../textMessageHandler';
import { expectFilterResponse, clearFilterResponseExpectation } from '../filterSync';

const restGraph = (n: number) => ({
  nodes: Array.from({ length: n }, (_, i) => ({ id: String(i), label: `n${i}`, position: { x: i, y: 0, z: 0 } })),
  edges: [],
});
const wsLoad = (n: number) => ({
  type: 'initialGraphLoad',
  nodes: Array.from({ length: n }, (_, i) => ({ id: i, label: `n${i}`, position: { x: i, y: 0, z: 0 } })),
  edges: [],
});
const dispatch = (m: Record<string, unknown>) =>
  handleTextMessage(m as never, (() => ({ forceReconnect: vi.fn() })) as never, vi.fn() as never, vi.fn() as never);
const settle = () => new Promise(r => setTimeout(r, 0));

describe('startup topology race (real handler + real manager)', () => {
  beforeEach(async () => {
    pendingRest = null;
    clearFilterResponseExpectation();
    await graphDataManager.setGraphData({ nodes: [], edges: [] });
    topologies.length = 0;
  });

  it('capped connect-time load during the REST load never becomes the topology', async () => {
    const rest = graphDataManager.fetchInitialData();
    await settle();

    dispatch(wsLoad(30));          // capped push lands first
    await settle();
    expect(graphDataManager.nodeIdMap.size).toBe(0);
    expect(topologies).toEqual([]);

    pendingRest!.resolve(restGraph(94));
    await rest;
    await settle();
    expect(graphDataManager.nodeIdMap.size).toBe(94);
    expect(topologies).toEqual([94]);
  });

  it('a filter response during the REST load lands after REST, narrowing it', async () => {
    const rest = graphDataManager.fetchInitialData();
    await settle();

    expectFilterResponse();
    dispatch(wsLoad(40));
    await settle();
    expect(graphDataManager.nodeIdMap.size).toBe(0);

    pendingRest!.resolve(restGraph(94));
    await rest;
    await settle();
    expect(topologies).toEqual([94, 40]);
    expect(graphDataManager.nodeIdMap.size).toBe(40);
  });
});
