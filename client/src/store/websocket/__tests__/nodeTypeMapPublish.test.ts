// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * The store's `nodeTypeMap` is a dependency of the graph's per-node visual-mode map,
 * which in turn rebuilds the whole edge plan (one pass over every edge). Publishing a
 * fresh Map on every position chunk rebuilt that plan about twice a second on a live
 * graph. A chunk that teaches the client nothing new about node types must not publish.
 * Real encoder and parser; only the heavy collaborators are stubbed.
 */

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));
vi.mock('../../../utils/clientDebugState', () => ({
  debugState: { isEnabled: () => false, isDataDebugEnabled: () => false },
}));
vi.mock('../../settingsStore', () => ({
  useSettingsStore: { getState: () => ({ settings: {} }) },
}));
vi.mock('../../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: { getGraphType: () => 'knowledge', updateNodePositions: vi.fn(() => Promise.resolve()) },
}));
vi.mock('../../../features/analytics/store/nodeAnalyticsStore', () => ({
  nodeAnalyticsStore: { ingest: vi.fn() },
}));
vi.mock('../../transientBeamStore', () => ({ pushTransientBeams: vi.fn() }));
vi.mock('../connectionManager', () => ({ emit: vi.fn(), notifyBinaryMessageHandlers: vi.fn() }));
vi.mock('../../../utils/BatchQueue', () => ({
  NodePositionBatchQueue: class {},
  createWebSocketBatchProcessor: vi.fn(),
}));
vi.mock('../../../utils/validation', () => ({
  validateNodePositions: () => ({ valid: true, errors: [] }),
  createValidationMiddleware: vi.fn(),
}));

import { processBinaryData, resetBinaryState } from '../binaryProtocol';
import { KNOWLEDGE_NODE_FLAG, ONTOLOGY_CLASS_FLAG, NodeType } from '../../../types/binaryProtocol';

/** A V5 chunk: [0x05][u64 sequence][n x 52-byte records], one record per wire id. */
function v5Chunk(sequence: number, wireIds: number[]): ArrayBuffer {
  const buf = new ArrayBuffer(1 + 8 + wireIds.length * 52);
  const v = new DataView(buf);
  v.setUint8(0, 5);
  v.setUint32(1, sequence >>> 0, true);
  wireIds.forEach((id, i) => {
    const o = 9 + i * 52;
    v.setUint32(o, id >>> 0, true);
    v.setFloat32(o + 4, sequence + i * 1.5, true); // positions move every chunk
  });
  return buf;
}

function harness() {
  const socket = { readyState: WebSocket.OPEN, send: vi.fn() } as unknown as WebSocket;
  const set = vi.fn<(partial: { nodeTypeMap: Map<number, NodeType> }) => void>();
  return { get: () => ({ socket }), set };
}

const K = (id: number) => (id | KNOWLEDGE_NODE_FLAG) >>> 0;
const O = (id: number) => (id | ONTOLOGY_CLASS_FLAG) >>> 0;

describe('node type map publication', () => {
  beforeEach(() => resetBinaryState());

  it('publishes the types the first chunk carries', async () => {
    const h = harness();
    await processBinaryData(v5Chunk(1, [K(1), O(2)]), h.get, h.set);
    expect(h.set).toHaveBeenCalledTimes(1);
    const map = h.set.mock.calls[0][0].nodeTypeMap;
    expect(map.get(1)).toBe(NodeType.Knowledge);
    expect(map.get(2)).toBe(NodeType.OntologyClass);
  });

  it('does not publish again when later chunks only move the same nodes', async () => {
    const h = harness();
    for (let seq = 1; seq <= 5; seq++) {
      await processBinaryData(v5Chunk(seq, [K(1), O(2)]), h.get, h.set);
    }
    expect(h.set).toHaveBeenCalledTimes(1);
  });

  it('publishes when a node appears or a known node changes type, with a new Map each time', async () => {
    const h = harness();
    await processBinaryData(v5Chunk(1, [K(1), O(2)]), h.get, h.set);
    await processBinaryData(v5Chunk(2, [K(1), O(2), K(3)]), h.get, h.set); // node 3 appears
    expect(h.set).toHaveBeenCalledTimes(2);
    await processBinaryData(v5Chunk(3, [O(1), O(2), K(3)]), h.get, h.set); // node 1 changes type
    expect(h.set).toHaveBeenCalledTimes(3);
    const [first, second, third] = h.set.mock.calls.map((c) => c[0].nodeTypeMap);
    expect(second).not.toBe(first);
    expect(third).not.toBe(second);
    expect(first.has(3)).toBe(false); // an earlier publication is never mutated
    expect(second.get(1)).toBe(NodeType.Knowledge);
    expect(third.get(1)).toBe(NodeType.OntologyClass);
    expect(third.get(3)).toBe(NodeType.Knowledge);
  });
});
