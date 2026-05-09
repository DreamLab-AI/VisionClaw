import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

const mockGetData = vi.fn();
vi.mock('../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    getData: (...args: unknown[]) => mockGetData(...args),
  },
}));

vi.mock('../utils/pollingPerformance', () => {
  class MockPollingPerformanceMonitor {
    recordPoll = vi.fn();
    recordError = vi.fn();
    getMetrics = vi.fn(() => ({}));
    getSummary = vi.fn(() => ({}));
    reset = vi.fn();
  }
  return { PollingPerformanceMonitor: MockPollingPerformanceMonitor };
});

import { AgentPollingService } from './AgentPollingService';

describe('AgentPollingService', () => {
  let service: AgentPollingService;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    // Create fresh instance by accessing the static method
    // Since it's a singleton, we need to reset it
    (AgentPollingService as any).instance = undefined;
    service = AgentPollingService.getInstance();
    mockGetData.mockResolvedValue({
      nodes: [],
      edges: [],
      metadata: { active_agents: 0, total_agents: 0 },
    });
  });

  afterEach(() => {
    service.stop();
    service.stop(); // ensure subscriber count hits 0
    vi.useRealTimers();
  });

  it('returns a singleton instance', () => {
    const instance2 = AgentPollingService.getInstance();
    expect(service).toBe(instance2);
  });

  it('starts polling with a subscriber', () => {
    service.start();
    const status = service.getStatus();
    expect(status.isPolling).toBe(true);
  });

  it('stops polling when last subscriber leaves', () => {
    service.start();
    service.stop();
    const status = service.getStatus();
    expect(status.isPolling).toBe(false);
  });

  it('subscribe returns an unsubscribe function', () => {
    const callback = vi.fn();
    const unsub = service.subscribe(callback);

    expect(typeof unsub).toBe('function');
    unsub();
  });

  it('notifies subscribers when data changes', async () => {
    const callback = vi.fn();
    service.subscribe(callback);
    service.start();

    // Initial delay is 5000ms
    await vi.advanceTimersByTimeAsync(5100);

    expect(callback).toHaveBeenCalled();
  });

  it('does not notify when data has not changed', async () => {
    const callback = vi.fn();
    service.subscribe(callback);
    service.start();

    // First poll
    await vi.advanceTimersByTimeAsync(5100);
    const callCount = callback.mock.calls.length;

    // Second poll with same data
    await vi.advanceTimersByTimeAsync(10100);

    // Should not have been called again (same hash)
    expect(callback.mock.calls.length).toBe(callCount);
  });

  it('configure updates polling config', () => {
    service.configure({
      activePollingInterval: 500,
      idlePollingInterval: 2000,
    });

    const status = service.getStatus();
    expect(status.currentInterval).toBeDefined();
  });

  it('handles polling errors with retry', async () => {
    const errorCallback = vi.fn();
    service.subscribe(vi.fn(), errorCallback);
    service.start();

    mockGetData.mockRejectedValueOnce(new Error('API down'));

    await vi.advanceTimersByTimeAsync(5100);

    expect(errorCallback).toHaveBeenCalledWith(expect.any(Error));
  });

  it('getPerformanceMetrics returns metrics object', () => {
    const metrics = service.getPerformanceMetrics();
    expect(metrics).toBeDefined();
  });

  it('getStatus returns comprehensive status', () => {
    const status = service.getStatus();
    expect(status).toHaveProperty('isPolling');
    expect(status).toHaveProperty('currentInterval');
    expect(status).toHaveProperty('activityLevel');
    expect(status).toHaveProperty('lastPollTime');
    expect(status).toHaveProperty('retryCount');
  });
});
