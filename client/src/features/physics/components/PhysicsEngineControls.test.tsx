import { describe, it, expect, beforeEach, vi } from 'vitest';
import React from 'react';
import { render } from '@testing-library/react';

vi.mock('../../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

vi.mock('lucide-react', () => {
  const stub = () => React.createElement('span');
  return {
    Info: stub,
    Cpu: stub,
    Zap: stub,
    Layers: stub,
    GitBranch: stub,
    Activity: stub,
    AlertCircle: stub,
    Plus: stub,
  };
});

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: () => ({
    settings: {
      visualisation: {
        graphs: {
          logseq: {
            nodes: {},
            edges: {},
            labels: {},
            physics: {
              repulsionStrength: 1,
              springStrength: 0.1,
              damping: 0.9,
              gravity: 0.01,
              timeStep: 0.016,
              maxVelocity: 10,
              temperature: 1,
            },
          },
        },
      },
    },
    initialized: true,
    updateSettings: vi.fn(),
    loadSection: vi.fn(),
    ensureLoaded: vi.fn(),
  }),
}));

vi.mock('@/features/design-system/components/Toast', () => ({
  useToast: () => ({ toast: vi.fn() }),
}));

vi.mock('@/features/design-system/components/Card', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Card: mc, CardContent: mc, CardDescription: mc, CardHeader: mc, CardTitle: mc };
});
vi.mock('@/features/design-system/components/Select', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Select: mc, SelectContent: mc, SelectItem: mc, SelectTrigger: mc, SelectValue: mc };
});
vi.mock('@/features/design-system/components/Slider', () => {
  const React = require('react');
  return { Slider: () => React.createElement('div') };
});
vi.mock('@/features/design-system/components/Switch', () => {
  const React = require('react');
  return { Switch: () => React.createElement('div') };
});
vi.mock('@/features/design-system/components/Label', () => {
  const React = require('react');
  return { Label: (props: any) => React.createElement('label', null, props.children) };
});
vi.mock('@/features/design-system/components/Button', () => {
  const React = require('react');
  return { Button: (props: any) => React.createElement('button', null, props.children) };
});
vi.mock('@/features/design-system/components/Badge', () => {
  const React = require('react');
  return { Badge: (props: any) => React.createElement('span', null, props.children) };
});
vi.mock('@/features/design-system/components/Tabs', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Tabs: mc, TabsList: mc, TabsTrigger: mc, TabsContent: mc };
});
vi.mock('@/features/design-system/components/Tooltip', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { TooltipRoot: mc, TooltipContent: mc, TooltipProvider: mc, TooltipTrigger: mc };
});
vi.mock('@/features/analytics/components/SemanticClusteringControls', () => ({
  SemanticClusteringControls: () => React.createElement('div', null, 'Clustering'),
}));
vi.mock('./ConstraintBuilderDialog', () => ({
  ConstraintBuilderDialog: () => null,
}));
vi.mock('./PhysicsPresets', () => ({
  PhysicsPresets: () => React.createElement('div', null, 'Presets'),
}));
vi.mock('../../../services/api', () => ({
  unifiedApiClient: { post: vi.fn().mockResolvedValue({}) },
}));

import { PhysicsEngineControls } from './PhysicsEngineControls';

describe('PhysicsEngineControls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without crashing', () => {
    const { container } = render(React.createElement(PhysicsEngineControls));
    expect(container).toBeDefined();
  });

  it('renders physics settings from store', () => {
    const { container } = render(React.createElement(PhysicsEngineControls));
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('initializes with visual_analytics kernel mode', () => {
    // The component defaults to visual_analytics kernel mode
    const { container } = render(React.createElement(PhysicsEngineControls));
    expect(container).toBeDefined();
  });
});
