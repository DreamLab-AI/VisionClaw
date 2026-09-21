// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock dependencies before importing the store
vi.mock('../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn()
  }),
  createErrorMetadata: vi.fn()
}));

vi.mock('../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false
  }
}));

vi.mock('./settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({
      settings: {},
      subscribe: vi.fn()
    }),
    subscribe: vi.fn()
  }
}));

vi.mock('../features/graph/managers/graphDataManager', () => ({
  graphDataManager: {
    getGraphType: () => 'knowledge',
    updateNodePositions: vi.fn(),
    setGraphData: vi.fn().mockResolvedValue(undefined)
  }
}));

vi.mock('../types/binaryProtocol', () => ({
  parseBinaryNodeData: vi.fn(() => []),
  isAgentNode: vi.fn(() => false),
  createBinaryNodeData: vi.fn()
}));

vi.mock('../utils/BatchQueue', () => ({
  NodePositionBatchQueue: vi.fn().mockImplementation(() => ({
    enqueuePositionUpdate: vi.fn(),
    flush: vi.fn().mockResolvedValue(undefined),
    getMetrics: vi.fn(() => ({})),
    destroy: vi.fn()
  })),
  createWebSocketBatchProcessor: vi.fn(() => ({
    processBatch: vi.fn(),
    onError: vi.fn(),
    onSuccess: vi.fn()
  }))
}));

vi.mock('../utils/validation', () => ({
  validateNodePositions: vi.fn(() => ({ valid: true, errors: [] })),
  createValidationMiddleware: vi.fn(() => (batch: unknown[]) => batch)
}));

vi.mock('../services/BinaryWebSocketProtocol', () => ({
  binaryProtocol: {
    parseHeader: vi.fn(),
    extractPayload: vi.fn(),
    createBroadcastAck: vi.fn(() => new ArrayBuffer(0))
  },
  MessageType: {
    GRAPH_UPDATE: 1,
    VOICE_DATA: 2,
    POSITION_UPDATE: 3,
    AGENT_POSITIONS: 4
  },
  GraphTypeFlag: {
    KNOWLEDGE_GRAPH: 0,
    ONTOLOGY: 1
  }
}));

vi.mock('../services/nostrAuthService', () => ({
  nostrAuth: {
    getSessionToken: vi.fn(() => null),
    getCurrentUser: vi.fn(() => null)
  }
}));

// Mock global objects for Node.js testing environment
const mockLocation = {
  protocol: 'http:',
  hostname: 'localhost',
  port: '3000'
};

// @ts-ignore
global.window = global.window || { location: mockLocation };
// @ts-ignore
global.window.location = mockLocation;
// @ts-ignore
global.window.setTimeout = setTimeout;
// @ts-ignore
global.window.clearTimeout = clearTimeout;
// @ts-ignore
global.window.setInterval = setInterval;
// @ts-ignore
global.window.clearInterval = clearInterval;

// jsdom installs `localStorage` as a non-writable accessor on the global, so a
// bare `global.localStorage = ...` throws under jsdom >= 27. vi.stubGlobal
// redefines the property instead of assigning through the accessor.
vi.stubGlobal('localStorage', {
  getItem: vi.fn(() => null),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  clear: vi.fn(),
  length: 0,
  key: vi.fn(() => null),
} as Storage);

import { useWebSocketStore, webSocketService, WebSocketServiceCompat } from './websocketStore';

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState = MockWebSocket.CONNECTING;
  onopen: ((ev: Event) => void) | null = null;
  onclose: ((ev: CloseEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;

  constructor(public url: string) {
    // Simulate async open
    setTimeout(() => {
      this.readyState = MockWebSocket.OPEN;
      this.onopen?.(new Event('open'));
    }, 0);
  }

  send = vi.fn();
  close = vi.fn((code?: number, reason?: string) => {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.(new CloseEvent('close', { code, reason, wasClean: true }));
  });
  addEventListener = vi.fn();
  removeEventListener = vi.fn();
}

describe('WebSocketStore', () => {
  beforeEach(() => {
    // Reset the store before each test
    useWebSocketStore.getState()._reset();
    // Reset the singleton
    WebSocketServiceCompat.resetInstance();
    // Mock WebSocket globally
    vi.stubGlobal('WebSocket', MockWebSocket);
  });

  describe('Initial State', () => {
    it('should have correct initial state', () => {
      const state = useWebSocketStore.getState();
      expect(state.isConnected).toBe(false);
      expect(state.isServerReady).toBe(false);
      expect(state.socket).toBeNull();
      expect(state.connectionState.status).toBe('disconnected');
      expect(state.messageQueue).toHaveLength(0);
    });
  });

  describe('Connection Management', () => {
    it('should queue messages when not connected', () => {
      const state = useWebSocketStore.getState();
      state.sendMessage('test', { data: 'hello' });

      const newState = useWebSocketStore.getState();
      expect(newState.messageQueue).toHaveLength(1);
      expect(newState.messageQueue[0].type).toBe('text');
    });

    it('should provide connection state through getConnectionState', () => {
      const state = useWebSocketStore.getState();
      const connState = state.getConnectionState();

      expect(connState).toEqual({
        status: 'disconnected',
        reconnectAttempts: 0
      });
    });
  });

  describe('Event Handling', () => {
    it('should register and unregister message handlers', () => {
      const handler = vi.fn();
      const unsubscribe = useWebSocketStore.getState().onMessage(handler);

      // Verify handler is registered by checking internals
      const internals = useWebSocketStore.getState()._getInternals();
      expect(internals.messageHandlers).toContain(handler);

      // Unsubscribe
      unsubscribe();
      const internalsAfter = useWebSocketStore.getState()._getInternals();
      expect(internalsAfter.messageHandlers).not.toContain(handler);
    });

    it('should emit and receive events', () => {
      const handler = vi.fn();
      const unsubscribe = useWebSocketStore.getState().on('custom-event', handler);

      useWebSocketStore.getState().emit('custom-event', { test: true });

      expect(handler).toHaveBeenCalledWith({ test: true });

      unsubscribe();

      // Should not be called after unsubscribe
      useWebSocketStore.getState().emit('custom-event', { test: false });
      expect(handler).toHaveBeenCalledTimes(1);
    });
  });

  describe('Backward Compatibility', () => {
    it('webSocketService singleton should work as expected', () => {
      expect(webSocketService).toBeDefined();
      expect(typeof webSocketService.connect).toBe('function');
      expect(typeof webSocketService.disconnect).toBe('function');
      expect(typeof webSocketService.sendMessage).toBe('function');
      expect(typeof webSocketService.on).toBe('function');
      expect(typeof webSocketService.emit).toBe('function');
    });

    it('webSocketService.isConnected should reflect store state', () => {
      expect(webSocketService.isConnected).toBe(false);
    });

    it('should have sendErrorFrame method', () => {
      expect(typeof webSocketService.sendErrorFrame).toBe('function');
    });

    it('should have send method for WebSocketAdapter compatibility', () => {
      expect(typeof webSocketService.send).toBe('function');
    });
  });

  describe('Solid WebSocket', () => {
    it('should track Solid subscriptions', () => {
      const callback = vi.fn();
      const unsubscribe = useWebSocketStore.getState().subscribeSolidResource('https://example.com/resource', callback);

      const subscriptions = useWebSocketStore.getState().getSolidSubscriptions();
      expect(subscriptions).toContain('https://example.com/resource');

      unsubscribe();

      const subscriptionsAfter = useWebSocketStore.getState().getSolidSubscriptions();
      expect(subscriptionsAfter).not.toContain('https://example.com/resource');
    });
  });

  describe('Testing Utilities', () => {
    it('should reset all state with _reset', () => {
      // Modify some state
      useWebSocketStore.getState().sendMessage('test', {});

      // Reset
      useWebSocketStore.getState()._reset();

      const state = useWebSocketStore.getState();
      expect(state.messageQueue).toHaveLength(0);
      expect(state.isConnected).toBe(false);
    });

    it('should provide internals for testing', () => {
      const internals = useWebSocketStore.getState()._getInternals();

      expect(internals).toHaveProperty('messageHandlers');
      expect(internals).toHaveProperty('binaryMessageHandlers');
      expect(internals).toHaveProperty('connectionStatusHandlers');
      expect(internals).toHaveProperty('eventHandlers');
    });
  });
});
