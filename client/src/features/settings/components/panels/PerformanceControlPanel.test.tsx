import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
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
  Zap: () => React.createElement('span', null, 'Zap'),
  Cpu: () => React.createElement('span', null, 'Cpu'),
  MemoryStick: () => React.createElement('span', null, 'MemoryStick'),
  Gauge: () => React.createElement('span', null, 'Gauge'),
  TrendingUp: () => React.createElement('span', null, 'TrendingUp'),
  Activity: () => React.createElement('span', null, 'Activity'),
  Settings: () => React.createElement('span', null, 'Settings'),
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: (selector: any) => {
    const state = {
      settings: {
        performance: {
          showFPS: true,
          targetFPS: 60,
          levelOfDetail: 'medium',
          enableAdaptiveQuality: true,
        },
      },
      updateSettings: vi.fn(),
    };
    return selector ? selector(state) : state;
  },
}));

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
  return { Switch: () => React.createElement('div') };
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
vi.mock('@/features/design-system/components/Alert', () => {
  const React = require('react');
  const mc = (props: any) => React.createElement('div', null, props.children);
  return { Alert: mc, AlertDescription: mc };
});

import { PerformanceControlPanel } from './PerformanceControlPanel';

describe('PerformanceControlPanel', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders without crashing', () => {
    const { container } = render(React.createElement(PerformanceControlPanel));
    expect(container).toBeDefined();
  });

  it('renders performance controls from settings', () => {
    const { container } = render(React.createElement(PerformanceControlPanel));
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('polls metrics after initial delay', async () => {
    render(React.createElement(PerformanceControlPanel));

    await vi.advanceTimersByTimeAsync(3500);

    expect(fetch).toHaveBeenCalledWith('/api/performance/metrics');
  });
});
