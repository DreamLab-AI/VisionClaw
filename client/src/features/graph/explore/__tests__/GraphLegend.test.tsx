import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, within, act } from '@testing-library/react';
import type { GraphData } from '../../managers/graphDataManager';
import { exploreNodes } from './fixtures';

let explore: Record<string, unknown> = {};
vi.mock('../../../../store/settingsStore', () => {
  const state = () => ({ settings: { nodeFilter: { includeLinkedPages: false }, visualisation: { graphView: { explore } } } });
  const useSettingsStore = (sel: (s: unknown) => unknown) => sel(state());
  useSettingsStore.getState = state;
  return { useSettingsStore };
});
let emit: ((d: GraphData) => void) | null = null;
vi.mock('../../managers/graphDataManager', () => ({
  graphDataManager: { onGraphDataChange: (l: (d: GraphData) => void) => { emit = l; return () => { emit = null; }; } },
}));

import { GraphLegend, LegendView } from '../GraphLegend';
import { nodeEncoding, DEFAULT_NODE_ENCODER } from '../../state/nodeEncoding';

describe('<GraphLegend/> (ADR-2184 D4)', () => {
  beforeEach(() => { explore = { colorBy: 'scheme', sizeBy: 'none', showLegend: true }; });

  it('renders categorical swatches with counts, other and no-value rows', () => {
    render(<LegendView legend={{ entries: [{
      channel: 'color', kind: 'categorical', column: 'domain', other: 3, nulls: 2,
      swatches: [{ value: 'robotics', color: '#FFB74D', count: 1234 }],
    }] }} />);
    const legend = screen.getByRole('group', { name: 'Graph legend' });
    expect(legend).toHaveAttribute('data-scene-occluder');
    expect(within(legend).getByText('Colour: Domain')).toBeInTheDocument();
    expect(within(legend).getByText('robotics')).toBeInTheDocument();
    expect(within(legend).getByText('1,234')).toBeInTheDocument();
    expect(within(legend).getByText('Other')).toBeInTheDocument();
    expect(within(legend).getByText('No value')).toBeInTheDocument();
  });

  it('renders a numeric ramp with min and max', () => {
    render(<LegendView legend={{ entries: [{ channel: 'color', kind: 'sequential', column: 'quality_score', temporal: false, min: 0.1, max: 0.95, nulls: 0 }] }} />);
    expect(screen.getByRole('img', { name: 'Quality score from 0.1 to 0.95' })).toBeInTheDocument();
  });

  it('renders nothing for the default encoding', () => {
    const { container } = render(<GraphLegend />);
    expect(container).toBeEmptyDOMElement();
  });

  it('installs the column encoder from the settings and reflects the graph load', () => {
    explore = { colorBy: 'domain', sizeBy: 'quality_score', showLegend: true };
    render(<GraphLegend />);
    act(() => { emit!({ nodes: exploreNodes(), edges: [] } as GraphData); });
    expect(screen.getByTestId('legend-color')).toHaveTextContent('robotics');
    expect(screen.getByTestId('legend-size')).toHaveTextContent('Size: Quality score');
    expect(nodeEncoding.getEncoder().color).not.toBe(DEFAULT_NODE_ENCODER.color);
    expect(nodeEncoding.getEncoder().size).not.toBe(DEFAULT_NODE_ENCODER.size);
  });

  it('with showLegend off still encodes but renders nothing', () => {
    explore = { colorBy: 'domain', sizeBy: 'none', showLegend: false };
    const { container, unmount } = render(<GraphLegend />);
    act(() => { emit!({ nodes: exploreNodes(), edges: [] } as GraphData); });
    expect(container).toBeEmptyDOMElement();
    expect(nodeEncoding.getEncoder().color).not.toBe(DEFAULT_NODE_ENCODER.color);
    unmount();
    expect(nodeEncoding.getEncoder()).toBe(DEFAULT_NODE_ENCODER);
  });
});
