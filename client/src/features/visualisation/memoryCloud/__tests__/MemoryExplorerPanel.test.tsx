import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, within } from '@testing-library/react';
import type { MemoryCloudSnapshot, MemoryCloudQueryResponse, MemoryCloudHealth } from '../types';
import type { QueryRun, SearchTree } from '../../memoryTrajectory/types';

const h = vi.hoisted(() => ({
  settings: {
    visualisation: {
      embeddingCloud: {
        enabled: true, pointSize: 7.5, opacity: 0.6, colorBy: 'namespace', rotationSpeed: 0.0005, maxPoints: 50000,
        cloudScale: 5, trajectoryView: 'canopy', routeGlow: 1.2, playbackSpeed: 1, showRejected: true, dimOffRoute: 0.75,
        learningEnabled: true, learningTargetRecall: 0.9, learningRate: 0.2, cinematic: false,
      } as Record<string, unknown>,
    },
  },
  set: vi.fn(),
  postQuery: vi.fn(),
  fetchHealth: vi.fn(),
}));

vi.mock('@/store/settingsStore', async () => {
  const { create } = await import('zustand');
  const store = create(() => ({ settings: h.settings, set: h.set }));
  return { useSettingsStore: store };
});

vi.mock('../memoryCloudInstance', async () => {
  const { createMemoryCloudStore } = await import('../memoryCloudStore');
  return {
    useMemoryCloudStore: createMemoryCloudStore({
      fetchSnapshot: vi.fn(),
      fetchVectors: vi.fn(),
      postQuery: h.postQuery,
      fetchHealth: h.fetchHealth,
      loadTrajectory: vi.fn(),
    }),
  };
});

import MemoryExplorerPanel, { RecallSparkline } from '../MemoryExplorerPanel';
import { useMemoryCloudStore } from '../memoryCloudInstance';
import { MEMORY_FOCUS_EVENT } from '../../cameraFocus';

const snapshot: MemoryCloudSnapshot = {
  version: 1, snapshotId: 's1', generatedAt: 1, dim: 4, count: 1234,
  positions: [], metadata: [], namespaces: ['patterns', 'project-state'], sourceTypes: ['memory'],
  strata: [], excludedNamespaces: ['personal-context'], vectorsUrl: '/v',
};

const response: MemoryCloudQueryResponse = {
  snapshotId: 's1', embedModel: 'bge-small-en-v1.5',
  query: { text: 'q', vector: [1, 0, 0, 0] },
  sidecar: {
    tookMs: 4.2,
    results: [
      { id: 'a', key: 'adr-2122', namespace: 'project-state', sourceType: 'memory', score: 0.912, snippet: 'role isolation', sampleIndex: 7 },
      { id: 'b', key: 'hook-noise', namespace: 'patterns', sourceType: 'hook', score: 0.801, snippet: '', sampleIndex: 9 },
      { id: 'c', key: 'unsampled', namespace: 'patterns', sourceType: 'memory', score: 0.7, snippet: 'x', sampleIndex: null },
    ],
  },
};

const run: QueryRun = {
  result: { top: [7, 3], beam: [7, 3], hops: [1, 2], distanceEvals: 321, trace: [] },
  tree: { root: 1, nodes: new Map(), list: [], path: [1, 2, 7], maxDepth: 2, leaves: 1, keptTotal: 3 } as unknown as SearchTree,
  exactTop: [7, 9], recall: 0.5, breadth: 48, hintsUsed: [11],
};

function seed(extra: Partial<ReturnType<typeof useMemoryCloudStore.getState>> = {}) {
  useMemoryCloudStore.setState({
    status: 'ready',
    snapshot,
    query: { ...useMemoryCloudStore.getState().query, status: 'done', k: 10, response, run, text: 'role isolation' },
    recallHistory: [0.4, 0.5],
    flashes: { key: 3, namespace: 1, none: 2 },
    ...extra,
  });
}

beforeEach(() => {
  h.set.mockReset();
  h.postQuery.mockReset();
  h.fetchHealth.mockReset();
  h.settings.visualisation.embeddingCloud.cinematic = false;
  useMemoryCloudStore.setState({
    status: 'idle', snapshot: null, health: null, healthError: null, recallHistory: [],
    flashes: { key: 0, namespace: 0, none: 0 },
  });
});

describe('MemoryExplorerPanel — explore', () => {
  it('states honestly what the route is drawn over', () => {
    seed();
    render(<MemoryExplorerPanel />);
    expect(screen.getByText(/Route drawn over a local index of the 1,234-point sample; sidecar results shown for comparison\./)).toBeInTheDocument();
    expect(screen.getByText(/2 live flashes fell outside the sample/)).toBeInTheDocument();
  });

  it('lists sampled namespaces and runs a query with k and namespace', async () => {
    seed();
    const runQuery = vi.spyOn(useMemoryCloudStore.getState(), 'runQuery').mockResolvedValue();
    render(<MemoryExplorerPanel />);
    const ns = screen.getByLabelText('namespace') as HTMLSelectElement;
    expect(Array.from(ns.options).map((o) => o.value)).toEqual(['', 'patterns', 'project-state']);
    fireEvent.change(screen.getByLabelText('Query'), { target: { value: '  what is ADR-2122?  ' } });
    fireEvent.change(screen.getByLabelText('k'), { target: { value: '7' } });
    fireEvent.change(ns, { target: { value: 'project-state' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(runQuery).toHaveBeenCalledWith('what is ADR-2122?', { k: 7, namespace: 'project-state' });
    runQuery.mockRestore();
  });

  it('shows the HUD stats for the run', () => {
    seed();
    render(<MemoryExplorerPanel />);
    const stats = screen.getByLabelText('Search statistics');
    expect(within(stats).getByText('hops').nextSibling).toHaveTextContent('2');
    expect(within(stats).getByText('distance evals').nextSibling).toHaveTextContent('321');
    expect(within(stats).getByText('recall@10').nextSibling).toHaveTextContent('0.50');
    expect(within(stats).getByText('ef / breadth').nextSibling).toHaveTextContent('48');
    expect(within(stats).getByText('hints used').nextSibling).toHaveTextContent('1');
    expect(within(stats).getByText(/26% of the sample evaluated/)).toBeInTheDocument();
  });

  it('marks sidecar hits in or out of the sample and flies to sampled ones', () => {
    seed();
    render(<MemoryExplorerPanel />);
    const list = screen.getByRole('list', { name: 'Sidecar results' });
    expect(within(list).getByText('in sample · local agrees')).toBeInTheDocument();
    expect(within(list).getByText('in sample')).toBeInTheDocument();
    expect(within(list).getByText('not sampled')).toBeInTheDocument();
    expect(screen.getByText('1/2 sampled agree with the local top-k')).toBeInTheDocument();
    const seen: number[] = [];
    const onFocus = (e: Event) => seen.push((e as CustomEvent).detail.sampleIndex);
    window.addEventListener(MEMORY_FOCUS_EVENT, onFocus);
    fireEvent.click(within(list).getByText('adr-2122').closest('button')!);
    fireEvent.click(within(list).getByText('unsampled').closest('button')!);
    window.removeEventListener(MEMORY_FOCUS_EVENT, onFocus);
    expect(seen).toEqual([7]);
  });

  it('writes view and learning changes to settings so they persist', () => {
    seed();
    render(<MemoryExplorerPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Hyper' }));
    expect(h.set).toHaveBeenCalledWith('visualisation.embeddingCloud.trajectoryView', 'hyper');
    fireEvent.click(screen.getByLabelText('Learn from queries'));
    expect(h.set).toHaveBeenCalledWith('visualisation.embeddingCloud.learningEnabled', false);
    fireEvent.change(screen.getByLabelText('target recall'), { target: { value: '0.8' } });
    expect(h.set).toHaveBeenCalledWith('visualisation.embeddingCloud.learningTargetRecall', 0.8);
  });

  it('reset learning resets the engine', () => {
    seed();
    const reset = vi.spyOn(useMemoryCloudStore.getState(), 'resetLearning');
    render(<MemoryExplorerPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Reset learning' }));
    expect(reset).toHaveBeenCalled();
    reset.mockRestore();
  });

  it('shows build progress while the local index builds', () => {
    useMemoryCloudStore.setState({ status: 'building', buildProgress: 0.42, snapshot });
    render(<MemoryExplorerPanel />);
    expect(screen.getByText('Building local index 42%')).toBeInTheDocument();
  });

  it('hides the cinematic section unless the setting is on', () => {
    seed();
    const { unmount } = render(<MemoryExplorerPanel />);
    expect(screen.queryByLabelText('Cinematic')).toBeNull();
    unmount();
    h.settings.visualisation.embeddingCloud.cinematic = true;
    render(<MemoryExplorerPanel />);
    expect(screen.getByLabelText('Cinematic')).toBeInTheDocument();
    expect(screen.getByText(/decoded on this device and never uploaded/)).toBeInTheDocument();
  });
});

describe('MemoryExplorerPanel — health', () => {
  const health: MemoryCloudHealth = {
    snapshotId: 's1', generatedAt: 1_700_000_000_000,
    sidecar: { reachable: true, extensionVersion: '2.0.4', error: null },
    embedder: { url: 'http://x', model: 'bge-small-en-v1.5', reachable: false },
    namespaces: [{ namespace: 'patterns', total: 5000, embedded: 4900, sampled: 1200 }],
    recallProbe: { k: 10, probes: 64, recall: 0.957, indexMs: 1.8, exactMs: 41, measuredAt: 1_700_000_000_000 },
  };

  it('fetches health and renders namespaces, recall probe, versions and flash counts', async () => {
    seed();
    h.fetchHealth.mockResolvedValue(health);
    render(<MemoryExplorerPanel />);
    await act(async () => {
      fireEvent.click(screen.getByRole('tab', { name: 'Health' }));
    });
    expect(h.fetchHealth).toHaveBeenCalled();
    const table = screen.getByRole('table', { name: 'Namespace coverage' });
    expect(within(table).getByText('patterns')).toBeInTheDocument();
    expect(within(table).getByText('4,900')).toBeInTheDocument();
    expect(within(table).getByText('1,200')).toBeInTheDocument();
    const probe = screen.getByLabelText('Recall probe');
    expect(within(probe).getByText('0.957')).toBeInTheDocument();
    expect(within(probe).getByText('1.80 ms')).toBeInTheDocument();
    expect(within(probe).getByText('41 ms')).toBeInTheDocument();
    expect(screen.getByText(/ruvector 2\.0\.4/)).toBeInTheDocument();
    expect(screen.getByText('unreachable')).toBeInTheDocument();
    expect(screen.getByText(/3 on sampled entries · 1 on namespace stand-ins · 2 unmatched \(33% unmatched\)/)).toBeInTheDocument();
    expect(screen.getByText(/Withheld by server policy: personal-context/)).toBeInTheDocument();
  });

  it('reports a health failure', async () => {
    seed();
    h.fetchHealth.mockRejectedValue(new Error('HTTP 503: sidecar down'));
    render(<MemoryExplorerPanel />);
    await act(async () => {
      fireEvent.click(screen.getByRole('tab', { name: 'Health' }));
    });
    expect(screen.getByRole('alert')).toHaveTextContent('HTTP 503: sidecar down');
  });
});

describe('RecallSparkline', () => {
  it('draws one vertex per query and labels the latest recall', () => {
    const { container } = render(<RecallSparkline values={[0.2, 0.6, 0.95]} />);
    const pts = container.querySelector('polyline')!.getAttribute('points')!.split(' ');
    expect(pts.length).toBe(3);
    expect(screen.getByRole('img')).toHaveAttribute('aria-label', 'Recall over the last 3 queries, latest 0.95');
  });
  it('degrades to text with fewer than two points', () => {
    render(<RecallSparkline values={[]} />);
    expect(screen.getByText('no queries yet')).toBeInTheDocument();
  });
});
