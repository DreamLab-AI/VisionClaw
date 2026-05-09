import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

vi.mock('../utils/loggerConfig', () => ({
  createAgentLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    logAgentAction: vi.fn(),
    logWebSocketMessage: vi.fn(),
    logThreeJSAction: vi.fn(),
    logPerformance: vi.fn(),
    getAgentTelemetry: vi.fn(() => []),
    getWebSocketTelemetry: vi.fn(() => []),
    getThreeJSTelemetry: vi.fn(() => []),
  }),
}));

const mockApiGet = vi.fn();
vi.mock('../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    get: (...args: unknown[]) => mockApiGet(...args),
  },
}));

// Reset singleton before each test
let AgentTelemetryServiceClass: typeof import('./AgentTelemetry').AgentTelemetryService;
let agentTelemetrySingleton: typeof import('./AgentTelemetry').agentTelemetry;

describe('AgentTelemetry', () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
    });

    const mod = await import('./AgentTelemetry');
    AgentTelemetryServiceClass = mod.AgentTelemetryService;
    agentTelemetrySingleton = mod.agentTelemetry;
  });

  afterEach(() => {
    agentTelemetrySingleton.destroy();
    vi.useRealTimers();
  });

  describe('singleton', () => {
    it('returns the same instance from getInstance', () => {
      const a = AgentTelemetryServiceClass.getInstance();
      const b = AgentTelemetryServiceClass.getInstance();
      expect(a).toBe(b);
    });
  });

  describe('enable', () => {
    it('enables telemetry service', () => {
      expect(() => agentTelemetrySingleton.enable()).not.toThrow();
    });

    it('is idempotent -- calling enable twice does not throw', () => {
      agentTelemetrySingleton.enable();
      expect(() => agentTelemetrySingleton.enable()).not.toThrow();
    });
  });

  describe('logAgentSpawn', () => {
    it('increments agentSpawns counter', () => {
      agentTelemetrySingleton.logAgentSpawn('agent-1', 'researcher');

      const data = agentTelemetrySingleton.getDebugOverlayData();
      expect(data.metrics.agentSpawns).toBe(1);
    });
  });

  describe('logWebSocketMessage', () => {
    it('increments webSocketMessages counter', () => {
      agentTelemetrySingleton.logWebSocketMessage('graph_update', 'incoming');

      const data = agentTelemetrySingleton.getDebugOverlayData();
      expect(data.metrics.webSocketMessages).toBe(1);
    });
  });

  describe('logRenderCycle', () => {
    it('tracks average frame time', () => {
      agentTelemetrySingleton.logRenderCycle(16.67);
      agentTelemetrySingleton.logRenderCycle(16.67);

      const data = agentTelemetrySingleton.getDebugOverlayData();
      expect(data.metrics.renderCycles).toBe(2);
      expect(data.metrics.averageFrameTime).toBeCloseTo(16.67, 1);
    });

    it('uses circular buffer -- does not grow unbounded', () => {
      for (let i = 0; i < 100; i++) {
        agentTelemetrySingleton.logRenderCycle(16);
      }

      const data = agentTelemetrySingleton.getDebugOverlayData();
      expect(data.metrics.renderCycles).toBe(100);
      // recentFrameTimes should be at most 10 entries
      expect(data.recentFrameTimes.length).toBeLessThanOrEqual(10);
    });
  });

  describe('getDebugOverlayData', () => {
    it('returns structured overlay data', () => {
      const data = agentTelemetrySingleton.getDebugOverlayData();

      expect(data).toHaveProperty('sessionId');
      expect(data).toHaveProperty('metrics');
      expect(data).toHaveProperty('recentFrameTimes');
      expect(data).toHaveProperty('agentTelemetry');
      expect(data).toHaveProperty('webSocketTelemetry');
      expect(data).toHaveProperty('threeJSTelemetry');
    });
  });

  describe('fetchAgentTelemetry', () => {
    it('fetches from status and data endpoints', async () => {
      mockApiGet.mockResolvedValueOnce({ data: { agents: [] } });
      mockApiGet.mockResolvedValueOnce({ data: { agents: [] } });

      await agentTelemetrySingleton.fetchAgentTelemetry();

      expect(mockApiGet).toHaveBeenCalledWith('/bots/status');
      expect(mockApiGet).toHaveBeenCalledWith('/bots/data');
    });

    it('falls back to cache on error', async () => {
      mockApiGet.mockRejectedValue(new Error('offline'));

      const result = await agentTelemetrySingleton.fetchAgentTelemetry();
      // Returns null if no cache
      expect(result).toBeNull();
    });
  });

  describe('destroy', () => {
    it('cleans up intervals and listeners', () => {
      agentTelemetrySingleton.enable();

      expect(() => agentTelemetrySingleton.destroy()).not.toThrow();
    });
  });
});
