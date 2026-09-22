// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

// --- Mock all dependencies before importing the module ---

vi.mock('../../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

vi.mock('../../../../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false,
  },
}));

vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({
      settings: {
        qualityGates: {},
        system: { debug: {} },
      },
      subscribe: vi.fn(),
    }),
    subscribe: vi.fn(),
  },
}));

const mockSetGraphData = vi.fn().mockResolvedValue(undefined);
const mockGetGraphData = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
const mockIsReady = vi.fn(() => true);
const mockOnGraphDataChange = vi.fn(() => vi.fn());
const mockOnPositionUpdate = vi.fn(() => vi.fn());
const mockUpdateNode = vi.fn().mockResolvedValue(undefined);
const mockRemoveNode = vi.fn().mockResolvedValue(undefined);
const mockProcessBinaryData = vi.fn().mockResolvedValue(undefined);
const mockHasUnknownNodes = vi.fn().mockResolvedValue(false);
const mockUpdateSettings = vi.fn().mockResolvedValue(undefined);
const mockSetTweeningSettings = vi.fn().mockResolvedValue(undefined);
// ADR-03 D7 / ADR-090: the worker surface the manager actually drives now.
// Topology (incl. all add/remove mutations) flows through setGraphTopology;
// binary position frames flow through processBinaryFrame(Uint8Array).
const mockSetGraphTopology = vi.fn().mockResolvedValue(undefined);
const mockProcessBinaryFrame = vi.fn().mockResolvedValue(undefined);

vi.mock('../graphWorkerProxy', () => ({
  graphWorkerProxy: {
    isReady: () => mockIsReady(),
    setGraphTopology: (...args: unknown[]) => mockSetGraphTopology(...args),
    processBinaryFrame: (...args: unknown[]) => mockProcessBinaryFrame(...args),
    setGraphData: (...args: unknown[]) => mockSetGraphData(...args),
    getGraphData: (...args: unknown[]) => mockGetGraphData(...args),
    onGraphDataChange: (...args: unknown[]) => mockOnGraphDataChange(...args),
    onPositionUpdate: (...args: unknown[]) => mockOnPositionUpdate(...args),
    updateNode: (...args: unknown[]) => mockUpdateNode(...args),
    removeNode: (...args: unknown[]) => mockRemoveNode(...args),
    processBinaryData: (...args: unknown[]) => mockProcessBinaryData(...args),
    hasUnknownNodes: (...args: unknown[]) => mockHasUnknownNodes(...args),
    updateSettings: (...args: unknown[]) => mockUpdateSettings(...args),
    setTweeningSettings: (...args: unknown[]) => mockSetTweeningSettings(...args),
  },
}));

vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    get: vi.fn().mockResolvedValue({
      data: {
        data: {
          nodes: [],
          edges: [],
        },
      },
    }),
  },
}));

vi.mock('../../../../store/websocketStore', () => ({
  WebSocketAdapter: vi.fn(),
}));

vi.mock('../../../../types/binaryProtocol', () => ({
  parseBinaryNodeData: vi.fn(() => []),
  createBinaryNodeData: vi.fn(() => new ArrayBuffer(0)),
  BINARY_NODE_SIZE: 28,
  PROTOCOL_V4: 4,
}));

vi.mock('../../../../services/BinaryWebSocketProtocol', () => ({
  binaryProtocol: {
    setUserInteracting: vi.fn(),
  },
}));

vi.mock('../../../../types/idMapping', () => ({
  stringToU32: vi.fn((str: string) => {
    // Simple deterministic hash for testing
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      hash = ((hash << 5) - hash + str.charCodeAt(i)) | 0;
    }
    return hash >>> 0;
  }),
}));

vi.mock('../../../../store/workerErrorStore', () => ({
  useWorkerErrorStore: {
    getState: () => ({
      setWorkerError: vi.fn(),
      resetTransientErrors: vi.fn(),
      recordTransientError: vi.fn(),
    }),
  },
}));

vi.mock('react', () => ({
  startTransition: (fn: () => void) => fn(),
}));

// --- Import after mocks ---
// We need to import fresh for each test, so we use dynamic import
// But for the singleton pattern, we access the class through the module

// Get access to the internal class via the module's exported singleton
import { graphDataManager } from '../graphDataManager';
import type { Node, GraphData } from '../graphWorkerProxy';

describe('GraphDataManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset the singleton's internal state
    graphDataManager.nodeIdMap.clear();
    (graphDataManager as any).reverseNodeIdMap?.clear?.();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // ---- fetchInitialData ----

  describe('fetchInitialData', () => {
    it('should populate nodeIdMap correctly for numeric IDs', async () => {
      const { unifiedApiClient } = await import('../../../../services/api/UnifiedApiClient');
      (unifiedApiClient.get as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        data: {
          data: {
            nodes: [
              { id: 42, label: 'Node A', position: { x: 1, y: 2, z: 3 } },
              { id: 99, label: 'Node B', position: { x: 4, y: 5, z: 6 } },
            ],
            edges: [
              { id: '42-99', source: 42, target: 99 },
            ],
          },
        },
      });

      mockGetGraphData.mockResolvedValueOnce({
        nodes: [
          { id: '42', label: 'Node A', position: { x: 1, y: 2, z: 3 } },
          { id: '99', label: 'Node B', position: { x: 4, y: 5, z: 6 } },
        ],
        edges: [
          { id: '42-99', source: '42', target: '99' },
        ],
      });

      const data = await graphDataManager.fetchInitialData();

      // nodeIdMap should have string keys mapping to numeric values
      expect(graphDataManager.nodeIdMap.has('42')).toBe(true);
      expect(graphDataManager.nodeIdMap.has('99')).toBe(true);
      expect(graphDataManager.nodeIdMap.get('42')).toBe(42);
      expect(graphDataManager.nodeIdMap.get('99')).toBe(99);
    });

    it('should coerce edge source/target to strings', async () => {
      const { unifiedApiClient } = await import('../../../../services/api/UnifiedApiClient');
      (unifiedApiClient.get as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        data: {
          data: {
            nodes: [
              { id: 1, label: 'A', position: { x: 0, y: 0, z: 0 } },
              { id: 2, label: 'B', position: { x: 1, y: 1, z: 1 } },
            ],
            edges: [
              { id: '1-2', source: 1, target: 2 },
            ],
          },
        },
      });

      mockGetGraphData.mockResolvedValueOnce({
        nodes: [
          { id: '1', label: 'A', position: { x: 0, y: 0, z: 0 } },
          { id: '2', label: 'B', position: { x: 1, y: 1, z: 1 } },
        ],
        edges: [
          { id: '1-2', source: '1', target: '2' },
        ],
      });

      await graphDataManager.fetchInitialData();

      // Verify the topology delivered to the worker carries string-coerced edges
      expect(mockSetGraphTopology).toHaveBeenCalled();
      const calledData = mockSetGraphTopology.mock.calls[0][0] as GraphData;
      expect(calledData.edges[0].source).toBe('1');
      expect(calledData.edges[0].target).toBe('2');
      expect(typeof calledData.edges[0].source).toBe('string');
    });

    it('should filter out edges with undefined source/target', async () => {
      const { unifiedApiClient } = await import('../../../../services/api/UnifiedApiClient');
      // Use an edge where source/target are literally the string "undefined"
      // (simulating a previous String(undefined) coercion) and the id does NOT
      // have a recoverable format. The code guards against this exact pattern.
      (unifiedApiClient.get as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        data: {
          data: {
            nodes: [
              { id: 1, label: 'A', position: { x: 0, y: 0, z: 0 } },
            ],
            edges: [
              { id: 'noformat', source: 'undefined', target: 'undefined' },
            ],
          },
        },
      });

      mockGetGraphData.mockResolvedValueOnce({
        nodes: [{ id: '1', label: 'A', position: { x: 0, y: 0, z: 0 } }],
        edges: [],
      });

      await graphDataManager.fetchInitialData();

      const calledData = mockSetGraphTopology.mock.calls[0][0] as GraphData;
      // The edge source/target are "undefined" strings, which are guarded against
      // and set to undefined, then the id "noformat" has no '-' so parts.length < 2.
      // Final String(undefined) = "undefined" which is filtered by the .filter() call.
      expect(calledData.edges.length).toBe(0);
    });

    it('should normalize node IDs to strings', async () => {
      const { unifiedApiClient } = await import('../../../../services/api/UnifiedApiClient');
      (unifiedApiClient.get as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
        data: {
          data: {
            nodes: [
              { id: 123, label: 'Numeric ID Node', position: { x: 1, y: 2, z: 3 } },
            ],
            edges: [],
          },
        },
      });

      mockGetGraphData.mockResolvedValueOnce({
        nodes: [{ id: '123', label: 'Numeric ID Node', position: { x: 1, y: 2, z: 3 } }],
        edges: [],
      });

      const data = await graphDataManager.fetchInitialData();

      // Worker topology should have been delivered with string IDs
      const calledData = mockSetGraphTopology.mock.calls[0][0] as GraphData;
      expect(calledData.nodes[0].id).toBe('123');
      expect(typeof calledData.nodes[0].id).toBe('string');
    });
  });

  // ---- setGraphData / node ID mapping ----

  describe('setGraphData', () => {
    it('should build nodeIdMap for numeric string IDs', async () => {
      await graphDataManager.setGraphData({
        nodes: [
          { id: '10', label: 'A', position: { x: 0, y: 0, z: 0 } },
          { id: '20', label: 'B', position: { x: 1, y: 1, z: 1 } },
        ],
        edges: [],
      });

      expect(graphDataManager.nodeIdMap.get('10')).toBe(10);
      expect(graphDataManager.nodeIdMap.get('20')).toBe(20);
    });

    it('should handle non-numeric string IDs via hash', async () => {
      await graphDataManager.setGraphData({
        nodes: [
          { id: 'alpha-node', label: 'Alpha', position: { x: 0, y: 0, z: 0 } },
        ],
        edges: [],
      });

      // The non-numeric ID should still be mapped
      expect(graphDataManager.nodeIdMap.has('alpha-node')).toBe(true);
      const mapped = graphDataManager.nodeIdMap.get('alpha-node');
      expect(typeof mapped).toBe('number');
      expect(mapped).toBeGreaterThanOrEqual(0);
    });

    it('should build reverse map correctly', async () => {
      await graphDataManager.setGraphData({
        nodes: [
          { id: '55', label: 'A', position: { x: 0, y: 0, z: 0 } },
        ],
        edges: [],
      });

      expect(graphDataManager.reverseNodeIds.get(55)).toBe('55');
    });

    it('should handle empty graph data without crash', async () => {
      await graphDataManager.setGraphData({ nodes: [], edges: [] });

      expect(graphDataManager.nodeIdMap.size).toBe(0);
      expect(mockSetGraphTopology).toHaveBeenCalledWith(expect.objectContaining({
        nodes: [],
        edges: [],
      }));
    });

    it('should handle null/undefined nodes gracefully', async () => {
      // @ts-ignore - testing edge case with invalid data
      await graphDataManager.setGraphData({ nodes: null, edges: [] });

      expect(graphDataManager.nodeIdMap.size).toBe(0);
    });

    it('should handle duplicate node IDs by overwriting', async () => {
      await graphDataManager.setGraphData({
        nodes: [
          { id: '100', label: 'First', position: { x: 0, y: 0, z: 0 } },
          { id: '100', label: 'Second', position: { x: 1, y: 1, z: 1 } },
        ],
        edges: [],
      });

      // Both use the same string key; second write overwrites
      expect(graphDataManager.nodeIdMap.has('100')).toBe(true);
      expect(graphDataManager.nodeIdMap.get('100')).toBe(100);
    });
  });

  // ---- mergeGraphData (additive expansion) ----

  describe('mergeGraphData', () => {
    it('additively merges new nodes/edges while preserving existing ones', async () => {
      await graphDataManager.setGraphData({
        nodes: [{ id: '1', label: 'Anchor', position: { x: 10, y: 0, z: 0 } }],
        edges: [],
      });

      // Expansion payload uses NUMERIC ids (the id-coercion trap): merge must
      // String()-coerce so they key the same string-keyed topology.
      // Live worker/SAB anchor position (far from the stale cached {10,0,0}) —
      // the merge must seed from THIS, not the cached node.position.
      const added = await graphDataManager.mergeGraphData(
        [
          { id: '2' as any, label: 'Neighbour', position: { x: 0, y: 0, z: 0 } },
          { id: '1' as any, label: 'Anchor dup', position: { x: 0, y: 0, z: 0 } }, // existing → skipped
        ] as Node[],
        [{ id: '1-2-rel', source: '1', target: '2', label: 'rel' } as any],
        '1',
        { x: 500, y: 0, z: 0 },
      );

      expect(added.nodesAdded).toBe(1);
      expect(added.edgesAdded).toBe(1);

      const data = graphDataManager.getLastGraphData();
      expect(data?.nodes.length).toBe(2);
      // Existing anchor kept its position untouched.
      const anchor = data?.nodes.find(n => String(n.id) === '1');
      expect(anchor?.position).toEqual({ x: 10, y: 0, z: 0 });
      // New node seeded near the LIVE anchor position (~500), not the origin
      // and not the stale cached {10}.
      const neighbour = data?.nodes.find(n => String(n.id) === '2');
      expect(neighbour).toBeDefined();
      const p = neighbour!.position;
      expect(p.x === 0 && p.y === 0 && p.z === 0).toBe(false);
      expect(Math.abs(p.x - 500)).toBeLessThan(50); // seeded around the live x≈500
    });

    it('is a no-op when nothing new is supplied', async () => {
      await graphDataManager.setGraphData({
        nodes: [{ id: '1', label: 'Anchor', position: { x: 0, y: 0, z: 0 } }],
        edges: [],
      });
      const added = await graphDataManager.mergeGraphData(
        [{ id: '1' as any, label: 'dup', position: { x: 0, y: 0, z: 0 } }] as Node[],
        [],
        '1',
      );
      expect(added).toEqual({ nodesAdded: 0, edgesAdded: 0 });
    });
  });

  // ---- ensureNodeHasValidPosition ----

  describe('ensureNodeHasValidPosition', () => {
    it('should add default position for nodes missing position', () => {
      const node: Node = { id: '1', label: 'Test' } as Node;
      const result = graphDataManager.ensureNodeHasValidPosition(node);
      expect(result.position).toEqual({ x: 0, y: 0, z: 0 });
    });

    it('should preserve valid positions', () => {
      const node: Node = { id: '1', label: 'Test', position: { x: 5, y: 10, z: 15 } };
      const result = graphDataManager.ensureNodeHasValidPosition(node);
      expect(result.position).toEqual({ x: 5, y: 10, z: 15 });
    });

    it('should fix invalid coordinate types', () => {
      const node = { id: '1', label: 'Test', position: { x: 'bad' as unknown as number, y: 2, z: 3 } } as Node;
      const result = graphDataManager.ensureNodeHasValidPosition(node);
      expect(result.position.x).toBe(0);
      expect(result.position.y).toBe(2);
      expect(result.position.z).toBe(3);
    });
  });

  // ---- Listener management ----

  describe('listeners', () => {
    it('should register and unregister graph data listeners', () => {
      const listener = vi.fn();
      const unsubscribe = graphDataManager.onGraphDataChange(listener);
      expect(typeof unsubscribe).toBe('function');
      unsubscribe();
    });

    it('should register and unregister position update listeners', () => {
      const listener = vi.fn();
      const unsubscribe = graphDataManager.onPositionUpdate(listener);
      expect(typeof unsubscribe).toBe('function');
      unsubscribe();
    });
  });

  // ---- updateNodePositions ----

  describe('updateNodePositions', () => {
    it('should skip empty ArrayBuffer', async () => {
      await graphDataManager.updateNodePositions(new ArrayBuffer(0));
      expect(mockProcessBinaryFrame).not.toHaveBeenCalled();
    });

    it('should process non-empty ArrayBuffer', async () => {
      // Create a minimal buffer that represents position data
      const buffer = new ArrayBuffer(28); // BINARY_NODE_SIZE
      await graphDataManager.updateNodePositions(buffer);
      // ADR-03 D7: the frame is handed to the worker zero-copy as a Uint8Array
      // view over the same buffer via processBinaryFrame.
      expect(mockProcessBinaryFrame).toHaveBeenCalled();
      const frame = mockProcessBinaryFrame.mock.calls[0][0] as Uint8Array;
      expect(frame).toBeInstanceOf(Uint8Array);
      expect(frame.buffer).toBe(buffer);
    });
  });

  // ---- Graph type ----

  describe('graphType', () => {
    it('should default to knowledge', () => {
      expect(graphDataManager.getGraphType()).toBe('knowledge');
    });

    it('should allow setting graph type', () => {
      graphDataManager.setGraphType('visionclaw');
      expect(graphDataManager.getGraphType()).toBe('visionclaw');
      // Reset
      graphDataManager.setGraphType('knowledge');
    });

    // ADR-2115: `logseq` is retired from the send-side API; receive-side
    // normalisation of persisted state is covered by settingsMigration.test.ts.
  });

  // ---- addNode / removeNode ----

  describe('addNode', () => {
    it('should add to nodeIdMap and forward to worker', async () => {
      await graphDataManager.addNode({
        id: '77',
        label: 'New Node',
        position: { x: 0, y: 0, z: 0 },
      });

      expect(graphDataManager.nodeIdMap.has('77')).toBe(true);
      expect(graphDataManager.nodeIdMap.get('77')).toBe(77);
      // ADR-03 D7: mutations are forwarded to the worker as a full topology
      // update (setGraphTopology), not a per-node updateNode call.
      expect(mockSetGraphTopology).toHaveBeenCalled();
      const forwarded = mockSetGraphTopology.mock.calls.at(-1)![0] as GraphData;
      expect(forwarded.nodes.some(n => n.id === '77')).toBe(true);
    });
  });

  describe('removeNode', () => {
    it('should remove from nodeIdMap and forward to worker', async () => {
      // First add a node
      await graphDataManager.addNode({
        id: '88',
        label: 'To Remove',
        position: { x: 0, y: 0, z: 0 },
      });
      expect(graphDataManager.nodeIdMap.has('88')).toBe(true);

      // Now remove
      await graphDataManager.removeNode('88');
      expect(graphDataManager.nodeIdMap.has('88')).toBe(false);
      // ADR-03 D7: removal is forwarded as a topology update with the node gone,
      // not a per-node removeNode call.
      expect(mockSetGraphTopology).toHaveBeenCalled();
      const forwarded = mockSetGraphTopology.mock.calls.at(-1)![0] as GraphData;
      expect(forwarded.nodes.some(n => n.id === '88')).toBe(false);
    });
  });

  // ---- dispose ----

  describe('dispose', () => {
    it('should clear all internal state', () => {
      // Pre-populate
      graphDataManager.nodeIdMap.set('test', 1);

      graphDataManager.dispose();

      expect(graphDataManager.nodeIdMap.size).toBe(0);
      expect(graphDataManager.webSocketService).toBeNull();
    });
  });
});
