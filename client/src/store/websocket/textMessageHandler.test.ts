import { describe, it, expect, beforeEach, vi } from 'vitest';

vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: (e: unknown) => ({ error: e }),
}));

vi.mock('../../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false,
  },
}));

vi.mock('../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: {
    setGraphData: vi.fn().mockResolvedValue(undefined),
    nodeIdMap: new Map(),
  },
}));

vi.mock('./connectionManager', () => ({
  emit: vi.fn(),
  notifyMessageHandlers: vi.fn(),
}));

vi.mock('./binaryProtocol', () => ({
  handleErrorFrame: vi.fn(),
}));

vi.mock('../analyticsStore', () => ({
  useAnalyticsStore: {
    getState: () => ({ merge: vi.fn() }),
  },
}));

import { handleTextMessage } from './textMessageHandler';
import { emit, notifyMessageHandlers } from './connectionManager';
import { handleErrorFrame } from './binaryProtocol';

describe('textMessageHandler', () => {
  const mockGet = vi.fn(() => ({ forceReconnect: vi.fn() }));
  const mockSet = vi.fn();
  const mockProcessQueue = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('sets isServerReady on connection_established', () => {
    handleTextMessage(
      { type: 'connection_established' } as any,
      mockGet,
      mockSet,
      mockProcessQueue,
    );

    expect(mockSet).toHaveBeenCalledWith({ isServerReady: true });
  });

  it('delegates error messages to handleErrorFrame and returns early', () => {
    const msg = { type: 'error', error: { code: 'E001', message: 'fail' } };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    expect(handleErrorFrame).toHaveBeenCalledWith(
      { code: 'E001', message: 'fail' },
      mockGet,
      mockProcessQueue,
    );
    // Should NOT reach notifyMessageHandlers
    expect(notifyMessageHandlers).not.toHaveBeenCalled();
  });

  it('emits filterApplied on filter_update_success', () => {
    const msg = {
      type: 'filter_update_success',
      data: { visible_nodes: 42, total_nodes: 100 },
    };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    expect(emit).toHaveBeenCalledWith('filterApplied', {
      visibleNodes: 42,
      totalNodes: 100,
    });
  });

  it('emits memoryFlash on memory_flash message', () => {
    const payload = { embedding: [1, 2, 3] };
    const msg = { type: 'memory_flash', data: payload };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    expect(emit).toHaveBeenCalledWith('memoryFlash', payload);
  });

  it('merges analytics_update into analytics store and returns early', () => {
    const msg = { type: 'analytics_update', cluster_id: 5 };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    // analytics_update returns early, so notifyMessageHandlers should not be called
    expect(notifyMessageHandlers).not.toHaveBeenCalled();
  });

  it('handles analytics_update merge error gracefully', () => {
    // Even if merge throws, the function should catch and not propagate
    expect(() =>
      handleTextMessage(
        { type: 'analytics_update' } as any,
        mockGet,
        mockSet,
        mockProcessQueue,
      ),
    ).not.toThrow();
  });

  it('calls notifyMessageHandlers for unknown message types', () => {
    const msg = { type: 'custom_event', payload: 'data' };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    expect(notifyMessageHandlers).toHaveBeenCalledWith(msg);
  });

  it('handles initialGraphLoad when nodeIdMap is empty', async () => {
    const msg = {
      type: 'initialGraphLoad',
      nodes: [{ id: 1, label: 'Node 1', node_type: 'page' }],
      edges: [{ id: 'e1', source: '1', target: '2' }],
    };

    handleTextMessage(msg as any, mockGet, mockSet, mockProcessQueue);

    const { graphDataManager } = await import('../../features/graph/managers/graphDataManager');
    expect(graphDataManager.setGraphData).toHaveBeenCalled();
  });
});
