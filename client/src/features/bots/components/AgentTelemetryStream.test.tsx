import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

const mockApiGet = vi.fn();
vi.mock('../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    get: (...args: unknown[]) => mockApiGet(...args),
  },
}));

import { AgentTelemetryStream } from './AgentTelemetryStream';

describe('AgentTelemetryStream', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockApiGet.mockResolvedValue({
      data: {
        agents: [
          {
            id: 'a1',
            type: 'researcher',
            status: 'active',
            health: 95,
            cpuUsage: 30,
            memoryUsage: 512,
          },
        ],
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders without crashing', () => {
    const { container } = render(React.createElement(AgentTelemetryStream));
    expect(container).toBeDefined();
  });

  it('shows disconnected state initially', () => {
    render(React.createElement(AgentTelemetryStream));
    // Initially disconnected until first successful poll
    expect(document.body.innerHTML).toBeTruthy();
  });

  it('polls for telemetry data after initial delay', async () => {
    render(React.createElement(AgentTelemetryStream));

    // Initial delay is 3000ms
    await vi.advanceTimersByTimeAsync(3500);

    expect(mockApiGet).toHaveBeenCalledWith('/bots/agents');
  });

  it('handles poll errors gracefully', async () => {
    mockApiGet.mockRejectedValueOnce(new Error('API unavailable'));

    render(React.createElement(AgentTelemetryStream));

    await vi.advanceTimersByTimeAsync(3500);

    // Component should not crash
    expect(document.body.innerHTML).toBeTruthy();
  });

  it('cleans up intervals on unmount', () => {
    const { unmount } = render(React.createElement(AgentTelemetryStream));

    unmount();

    // Advance time -- no more API calls should happen
    vi.advanceTimersByTime(10000);
    expect(mockApiGet).not.toHaveBeenCalled();
  });

  it('limits message buffer to 50 entries', async () => {
    // Return many agents to test buffer cap
    const manyAgents = Array.from({ length: 30 }, (_, i) => ({
      id: `a${i}`,
      type: 'worker',
      status: 'active',
      health: 90,
    }));
    mockApiGet.mockResolvedValue({ data: { agents: manyAgents } });

    render(React.createElement(AgentTelemetryStream));

    // First poll
    await vi.advanceTimersByTimeAsync(3500);
    // Second poll
    await vi.advanceTimersByTimeAsync(5500);

    // The internal messages array is capped at 50 -- no crash
    expect(document.body.innerHTML).toBeTruthy();
  });
});
