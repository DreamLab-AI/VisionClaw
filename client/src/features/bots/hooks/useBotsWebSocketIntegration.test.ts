import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

const mockOn = vi.fn(() => vi.fn()); // returns unsubscribe fn
const mockGetConnectionStatus = vi.fn(() => ({
  mcp: false,
  logseq: false,
  overall: false,
}));

vi.mock('../services/BotsWebSocketIntegration', () => ({
  botsWebSocketIntegration: {
    on: (...args: unknown[]) => mockOn(...args),
    getConnectionStatus: () => mockGetConnectionStatus(),
  },
}));

vi.mock('../../../telemetry/AgentTelemetry', () => ({
  agentTelemetry: {
    enable: vi.fn(),
    logAgentAction: vi.fn(),
  },
}));

vi.mock('../../../telemetry/useTelemetry', () => ({
  useTelemetry: () => ({
    track: vi.fn(),
    isEnabled: true,
  }),
}));

import { useBotsWebSocketIntegration } from './useBotsWebSocketIntegration';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';

describe('useBotsWebSocketIntegration', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns initial connection status', () => {
    const { result } = renderHook(() => useBotsWebSocketIntegration());

    expect(result.current).toEqual({
      mcp: false,
      logseq: false,
      overall: false,
    });
  });

  it('enables agent telemetry on mount', () => {
    renderHook(() => useBotsWebSocketIntegration());

    expect(agentTelemetry.enable).toHaveBeenCalled();
  });

  it('subscribes to mcp-connected and logseq-connected events', () => {
    renderHook(() => useBotsWebSocketIntegration());

    expect(mockOn).toHaveBeenCalledWith('mcp-connected', expect.any(Function));
    expect(mockOn).toHaveBeenCalledWith('logseq-connected', expect.any(Function));
  });

  it('logs initialization telemetry action', () => {
    renderHook(() => useBotsWebSocketIntegration());

    expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith(
      'websocket',
      'hook',
      'initialized_position_updates',
    );
  });

  it('cleans up subscriptions and interval on unmount', () => {
    const unsubMcp = vi.fn();
    const unsubLogseq = vi.fn();
    mockOn.mockReturnValueOnce(unsubMcp).mockReturnValueOnce(unsubLogseq);

    const { unmount } = renderHook(() => useBotsWebSocketIntegration());

    unmount();

    expect(unsubMcp).toHaveBeenCalled();
    expect(unsubLogseq).toHaveBeenCalled();
    expect(agentTelemetry.logAgentAction).toHaveBeenCalledWith('websocket', 'hook', 'cleanup');
  });

  it('polls connection status every 2 seconds', () => {
    renderHook(() => useBotsWebSocketIntegration());

    // Advance past polling interval
    vi.advanceTimersByTime(4100);

    // getConnectionStatus should have been called by the interval
    expect(mockGetConnectionStatus).toHaveBeenCalled();
  });
});
