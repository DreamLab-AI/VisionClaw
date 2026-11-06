import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, within, act, fireEvent } from '@testing-library/react';
import type { GraphData } from '../../managers/graphDataManager';
import { exploreNodes, node } from './fixtures';

vi.mock('../../../../store/settingsStore', () => {
  const state = () => ({ settings: { nodeFilter: { includeLinkedPages: false }, visualisation: { graphView: { explore: {} } } } });
  const useSettingsStore = (sel: (s: unknown) => unknown) => sel(state());
  useSettingsStore.getState = state;
  return { useSettingsStore };
});
let emit: ((d: GraphData) => void) | null = null;
vi.mock('../../managers/graphDataManager', () => ({
  graphDataManager: { onGraphDataChange: (l: (d: GraphData) => void) => { emit = l; return () => { emit = null; }; } },
}));

import { ExplorePanel } from '../ExplorePanel';
import { exploreStore, clearAllFilters } from '../exploreStore';
import { highlightMask, HIGHLIGHT_FILTERED_OUT } from '../../state/highlightMask';
import { setSharedNodeIdToIndexMap } from '../../contexts/NodePositionContext';

function load(nodes = exploreNodes()) {
  setSharedNodeIdToIndexMap(new Map(nodes.map((n, i) => [String(n.id), i])));
  act(() => { emit!({ nodes, edges: [] } as GraphData); });
  return nodes;
}

describe('<ExplorePanel/> (ADR-2185)', () => {
  beforeEach(() => { act(() => clearAllFilters()); });

  it('is a labelled scene occluder that waits for the graph', () => {
    render(<ExplorePanel />);
    const panel = screen.getByRole('region', { name: 'Explore graph' });
    expect(panel).toHaveAttribute('data-scene-occluder');
    expect(within(panel).getByText('Waiting for the graph to load…')).toBeInTheDocument();
  });

  it('shows an explicit "no time data" state instead of hiding the timeline', () => {
    render(<ExplorePanel />);
    load(exploreNodes().map((n) => node(n.id, n.label, { domain: String(n.metadata?.domain), quality_score: '0.5' })));
    expect(screen.getByTestId('explore-timeline-empty')).toHaveTextContent('No time data');
  });

  it('a category click greys the graph and narrows the other widgets, not its own bars', () => {
    render(<ExplorePanel />);
    const nodes = load();
    expect(screen.getByText('11 of 11 nodes')).toBeInTheDocument();

    const cats = screen.getByRole('group', { name: 'Domain categories' });
    fireEvent.click(within(cats).getByRole('button', { name: /^blockchain: 3 of 3 nodes/ }));

    // the mask: blockchain (ids 5-7) lit, the rest greyed, no node removed
    const greyed = nodes.filter((n) => highlightMask.bitsFor(String(n.id)) & HIGHLIGHT_FILTERED_OUT).map((n) => Number(n.id));
    expect(greyed).toEqual([1, 2, 3, 4, 8, 9, 10, 11, 12]);
    expect(screen.getByText('3 of 11 nodes')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Clear filters \(1\)/ })).toBeEnabled();

    // own bars keep the full distribution
    expect(within(cats).getByRole('button', { name: /^robotics: 4 of 4 nodes/ })).toHaveAttribute('aria-pressed', 'false');
    // the timeline narrows: of the 6 dated rows only ids 5, 6 are blockchain (both
    // September); January's bin keeps its total but counts 0
    const timeline = screen.getByRole('group', { name: 'Last commit timeline' });
    const labels = within(timeline).getAllByRole('button').map((b) => b.getAttribute('aria-label')!);
    expect(labels.filter((l) => !/: 0 of/.test(l))).toEqual([expect.stringMatching(/2026: 2 of 3 nodes$/)]);
    expect(labels.some((l) => /: 0 of 3 nodes$/.test(l))).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: /Clear filters/ }));
    expect(highlightMask.activeSources()).toEqual([]);
    expect(exploreStore.activeCount()).toBe(0);
  });

  it('timeline bars are keyboard-brushable buttons', () => {
    render(<ExplorePanel />);
    load();
    const timeline = screen.getByRole('group', { name: 'Last commit timeline' });
    const bars = within(timeline).getAllByRole('button');
    const first = bars.findIndex((b) => /: 3 of 3 nodes/.test(b.getAttribute('aria-label')!));
    fireEvent.click(bars[first], { detail: 0 }); // keyboard activation
    expect(bars[first]).toHaveAttribute('aria-pressed', 'true');
    expect(exploreStore.getClause('timeline')).toMatchObject({ kind: 'range', column: 'last_commit' });
    fireEvent.keyDown(bars[first], { key: 'Escape' });
    expect(exploreStore.getClause('timeline')).toBeUndefined();
  });

  it('search narrows after the debounce and reports the matches', async () => {
    vi.useFakeTimers();
    try {
      render(<ExplorePanel />);
      load();
      fireEvent.change(screen.getByLabelText('Search'), { target: { value: 'robot arm' } });
      act(() => { vi.advanceTimersByTime(250); });
      expect(screen.getByRole('status')).toHaveTextContent('1 nodes match');
      expect(exploreStore.getClause('search')).toEqual({ kind: 'search', query: 'robot arm' });
    } finally {
      vi.useRealTimers();
    }
  });
});
