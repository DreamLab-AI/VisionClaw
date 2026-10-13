/**
 * StatusFlyout jsdom coverage — the expanded body's sections and the SpacePilot
 * connect / guidance contract.
 *
 * The heavy data sources are mocked (constraint-stats polling, inferred-edges
 * store, graph manager, layout-motion sampler, settings store, bots context) so
 * the test asserts composition and interaction, not live telemetry. The
 * SpacePilot state is handed in as a prop, so the connect callback and the
 * support/secure-context guidance are driven directly.
 */

import React from 'react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';

const bots = {
  current: { botsData: { nodeCount: 3, edgeCount: 2, tokenCount: 500, mcpConnected: true, dataSource: 'live', multiAgentMetrics: { activeAgents: 2 } }, updateBotsData: vi.fn() } as {
    botsData: Record<string, unknown> | null;
    updateBotsData: (d: unknown) => void;
  },
};

vi.mock('../../../bots/contexts/BotsDataContext', () => ({
  useBotsDataOptional: () => bots.current,
}));
vi.mock('../../../bots/components', () => ({
  MultiAgentInitializationPrompt: () => <div data-testid="mock-init-prompt" />,
}));
vi.mock('../../../bots/services/BotsWebSocketIntegration', () => ({
  botsWebSocketIntegration: { clearAgents: vi.fn() },
}));
vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: { post: vi.fn(async () => ({ status: 200 })) },
}));
vi.mock('../../../ontology/hooks/useConstraintStats', () => ({
  useConstraintStats: () => ({
    stats: { activeConstraints: 0, axiomsProcessed: 0, gpuFailureCount: 0, cpuFallbackCount: 0 },
    loading: false,
    refresh: () => {},
  }),
}));
vi.mock('../../../ontology/store/useInferredEdgesStore', () => ({
  useInferredEdgesStore: (sel: (s: unknown) => unknown) =>
    sel({ report: { count: 0 }, refresh: () => Promise.resolve() }),
}));
vi.mock('../../../graph/managers/graphDataManager', () => ({
  graphDataManager: { getLastGraphData: () => null, onGraphDataChange: () => () => {} },
}));
vi.mock('../../agents/AgentOpsSurface', () => ({
  OPEN_AGENT_OPS_EVENT: 'visionclaw:open-agent-ops',
}));
const physicsState = vi.hoisted(() => ({
  current: { isSettled: false, stableFrameCount: 0, kineticEnergy: 0.207, telemetryLive: true, physicsRunning: true } as
    | { isSettled: boolean; stableFrameCount: number; kineticEnergy: number; telemetryLive: boolean; physicsRunning: boolean }
    | null,
}));
vi.mock('../usePhysicsState', () => ({ usePhysicsState: () => physicsState.current }));
vi.mock('../useConnectionTelemetry', () => ({
  useLayoutMotion: () => ({ motion: 'settled', ratePerSec: 0, feedFresh: true }),
}));
vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settingsSyncEnabled: true, setSettingsSyncEnabled: () => {} }),
}));

import { StatusFlyout } from '../StatusFlyout';
import type { SpacePilotState } from '../useSpacePilot';

const makeSpacePilot = (o: Partial<SpacePilotState> = {}): SpacePilotState => ({
  isSupported: true,
  isSecureContext: true,
  isLocalhost: false,
  connected: false,
  deviceName: undefined,
  buttons: [],
  connect: vi.fn(async () => {}),
  ...o,
});

afterEach(() => {
  cleanup();
  bots.current = {
    botsData: { nodeCount: 3, edgeCount: 2, tokenCount: 500, mcpConnected: true, dataSource: 'live', multiAgentMetrics: { activeAgents: 2 } },
    updateBotsData: vi.fn(),
  };
});

describe('StatusFlyout', () => {
  it('renders every section: connection, agents, SpacePilot, motion', () => {
    render(<StatusFlyout websocketStatus="connected" spacePilot={makeSpacePilot()} onClose={() => {}} />);
    expect(screen.getByTestId('status-flyout')).toBeInTheDocument();
    expect(screen.getByTestId('status-connection')).toBeInTheDocument();
    expect(screen.getByTestId('status-ws')).toBeInTheDocument();
    expect(screen.getByTestId('status-meta')).toBeInTheDocument();
    expect(screen.getByTestId('status-mcp')).toBeInTheDocument();
    expect(screen.getByTestId('status-agents')).toBeInTheDocument();
    expect(screen.getByTestId('status-spacepilot')).toBeInTheDocument();
    expect(screen.getByTestId('status-motion')).toBeInTheDocument();
  });

  it('shows the server physics state, and says so plainly when the GPU is not reporting', () => {
    const { rerender } = render(<StatusFlyout websocketStatus="connected" spacePilot={makeSpacePilot({})} onClose={() => {}} />);
    expect(screen.getByTestId('status-physics')).toHaveTextContent('Settling');
    expect(screen.getByTestId('status-physics')).toHaveTextContent('KE 0.21');

    physicsState.current = { isSettled: true, stableFrameCount: 5, kineticEnergy: 0, telemetryLive: false, physicsRunning: true };
    rerender(<StatusFlyout websocketStatus="connected" spacePilot={makeSpacePilot({})} onClose={() => {}} />);
    expect(screen.getByTestId('status-physics')).toHaveTextContent('GPU not reporting');

    physicsState.current = null;
    rerender(<StatusFlyout websocketStatus="connected" spacePilot={makeSpacePilot({})} onClose={() => {}} />);
    expect(screen.getByTestId('status-physics')).toHaveTextContent('Unavailable');
  });

  it('fires the SpacePilot connect callback when a device can be connected', () => {
    const sp = makeSpacePilot({ isSupported: true, isSecureContext: true, connected: false });
    render(<StatusFlyout websocketStatus="connected" spacePilot={sp} onClose={() => {}} />);

    const connectBtn = screen.getByTestId('status-spacepilot-connect');
    fireEvent.click(connectBtn);
    expect(sp.connect).toHaveBeenCalledTimes(1);
  });

  it('shows secure-context guidance (and no connect button) in an insecure context', () => {
    const sp = makeSpacePilot({ isSupported: true, isSecureContext: false });
    render(<StatusFlyout websocketStatus="connected" spacePilot={sp} onClose={() => {}} />);

    expect(screen.getByTestId('status-spacepilot-guidance')).toHaveTextContent(/secure context/i);
    expect(screen.queryByTestId('status-spacepilot-connect')).not.toBeInTheDocument();
  });

  it('shows unsupported-browser guidance when WebHID is absent', () => {
    const sp = makeSpacePilot({ isSupported: false });
    render(<StatusFlyout websocketStatus="connected" spacePilot={sp} onClose={() => {}} />);

    expect(screen.getByTestId('status-spacepilot-guidance')).toHaveTextContent(/not supported/i);
  });

  it('shows the device name once connected', () => {
    const sp = makeSpacePilot({ connected: true, deviceName: 'SpaceMouse Pro' });
    render(<StatusFlyout websocketStatus="connected" spacePilot={sp} onClose={() => {}} />);

    expect(screen.getByTestId('status-spacepilot')).toHaveTextContent('SpaceMouse Pro');
    expect(screen.queryByTestId('status-spacepilot-connect')).not.toBeInTheDocument();
  });

  it('offers Initialize when there are no active agents', () => {
    bots.current = { botsData: { nodeCount: 0, edgeCount: 0, tokenCount: 0, mcpConnected: false, dataSource: 'live' }, updateBotsData: vi.fn() };
    render(<StatusFlyout websocketStatus="connected" spacePilot={makeSpacePilot()} onClose={() => {}} />);

    const initBtn = screen.getByTestId('status-agents-initialize');
    expect(initBtn).toBeInTheDocument();
    fireEvent.click(initBtn);
    expect(screen.getByTestId('mock-init-prompt')).toBeInTheDocument();
  });
});
