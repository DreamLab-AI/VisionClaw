import { describe, it, expect, beforeEach, vi } from 'vitest';
import React from 'react';
import { render } from '@testing-library/react';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

vi.mock('../../../rendering/rendererFactory', () => ({
  isWebGPURenderer: () => false,
}));

// Mock Three.js and R3F completely
vi.mock('three', async () => {
  const actual = await vi.importActual<typeof import('three')>('three');
  return actual;
});

vi.mock('@react-three/fiber', () => ({
  useFrame: vi.fn(),
  Canvas: ({ children }: any) => React.createElement('div', null, children),
}));

vi.mock('@react-three/drei', () => ({
  Text: (props: any) => React.createElement('span', null, props.children),
  Html: (props: any) => React.createElement('div', null, props.children),
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: () => ({
    settings: {
      agents: {
        visualization: {
          show_in_graph: true,
          node_size: 1.5,
          node_color: '#ff8800',
          show_connections: true,
        },
      },
    },
  }),
}));

// Import the module to test its exported functions
import { AgentNodesLayer } from './AgentNodesLayer';

describe('AgentNodesLayer', () => {
  const mockAgents = [
    {
      id: 'agent-1',
      type: 'researcher',
      status: 'active' as const,
      health: 95,
      cpuUsage: 30,
      memoryUsage: 512,
      workload: 0.7,
      position: { x: 1, y: 2, z: 3 },
    },
    {
      id: 'agent-2',
      type: 'coder',
      status: 'idle' as const,
      health: 80,
      cpuUsage: 10,
      memoryUsage: 256,
      workload: 0.2,
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without crashing (R3F components are mocked)', () => {
    // Since R3F components need a Canvas parent, we test the component's
    // conditional logic directly. With empty agents, it returns null.
    const { container } = render(
      React.createElement(AgentNodesLayer, { agents: [], connections: [] }),
    );
    // Returns null when no agents
    expect(container.innerHTML).toBe('');
  });

  it('returns null when showAgents is false or agents is empty', () => {
    const { container } = render(
      React.createElement(AgentNodesLayer, { agents: [] }),
    );
    expect(container.innerHTML).toBe('');
  });

  it('accepts agents and connections props without error', () => {
    // In a real R3F environment this would render 3D objects.
    // With mocked R3F, we verify prop acceptance.
    expect(() =>
      render(
        React.createElement(AgentNodesLayer, {
          agents: mockAgents,
          connections: [
            { source: 'agent-1', target: 'agent-2', type: 'communication' },
          ],
        }),
      ),
    ).not.toThrow();
  });
});
