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

vi.mock('lucide-react', () => ({
  Activity: () => React.createElement('span', null, 'Activity'),
  RefreshCw: () => React.createElement('span', null, 'RefreshCw'),
  Gauge: () => React.createElement('span', null, 'Gauge'),
  Layers: () => React.createElement('span', null, 'Layers'),
  TrendingUp: () => React.createElement('span', null, 'TrendingUp'),
  Settings: () => React.createElement('span', null, 'Settings'),
  Cpu: () => React.createElement('span', null, 'Cpu'),
  Monitor: () => React.createElement('span', null, 'Monitor'),
}));

vi.mock('../../../../rendering/rendererFactory', () => ({
  rendererCapabilities: {
    webgpu: false,
    webgl2: true,
    backend: 'webgl2',
    pixelRatio: 1.0,
    maxTextureSize: 4096,
    maxViewportDims: [4096, 4096],
  },
  isWebGPURenderer: () => false,
}));

vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: (selector: any) => {
    const state = {
      settings: {
        dashboard: {
          autoRefresh: false,
          refreshInterval: 10000,
          computeMode: 'basic-force-directed',
        },
      },
      updateSettings: vi.fn(),
    };
    return selector ? selector(state) : state;
  },
}));

// Mock all design system components - inline to avoid hoisting issues
vi.mock('@/features/design-system/components/Button', () => {
  const React = require('react');
  return { Button: (props: any) => React.createElement('div', null, props.children) };
});
vi.mock('@/features/design-system/components/Card', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Card: mc, CardContent: mc, CardDescription: mc, CardHeader: mc, CardTitle: mc };
});
vi.mock('@/features/design-system/components/Switch', () => {
  const React = require('react');
  return { Switch: (props: any) => React.createElement('div', null) };
});
vi.mock('@/features/design-system/components/Select', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Select: mc, SelectContent: mc, SelectItem: mc, SelectTrigger: mc, SelectValue: mc };
});
vi.mock('@/features/design-system/components/Slider', () => {
  const React = require('react');
  return { Slider: (props: any) => React.createElement('div', null) };
});
vi.mock('@/features/design-system/components/Badge', () => {
  const React = require('react');
  return { Badge: (props: any) => React.createElement('span', null, props.children) };
});
vi.mock('@/features/design-system/components/Separator', () => {
  const React = require('react');
  return { Separator: () => React.createElement('hr') };
});
vi.mock('@/features/design-system/components/Label', () => {
  const React = require('react');
  return { Label: (props: any) => React.createElement('label', null, props.children) };
});

import { DashboardControlPanel } from './DashboardControlPanel';

describe('DashboardControlPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }));
  });

  it('renders without crashing', () => {
    const { container } = render(React.createElement(DashboardControlPanel));
    expect(container).toBeDefined();
  });

  it('renders with settings from store', () => {
    const { container } = render(React.createElement(DashboardControlPanel));
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('does not poll when autoRefresh is disabled', () => {
    render(React.createElement(DashboardControlPanel));
    expect(fetch).not.toHaveBeenCalled();
  });
});
