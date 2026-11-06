/**
 * NodeDetailPanel inside the right-hand drawer (ADR-2189 §3).
 * Regression: an agent-marker selection (bots/agentSelection.ts) carries no
 * `neighbors`, and `detail.neighbors.length` threw — inside the control centre
 * that took the dock down with it.
 */
import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, act, cleanup } from '@testing-library/react';
import { NodeDetailPanel } from '../NodeDetailPanel';
import { agentMarkerSelection } from '../../../bots/agentSelection';

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: false, status: 404, json: async () => ({}) })));
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('NodeDetailPanel', () => {
  it('renders an agent-marker selection that has no neighbors', () => {
    render(<NodeDetailPanel embedded />);
    act(() => {
      window.dispatchEvent(new CustomEvent('visionclaw:node-selected', { detail: agentMarkerSelection(7, undefined, 'planner') }));
    });
    expect(screen.getByRole('heading', { name: 'planner' })).toBeInTheDocument();
  });

  it('embedded, it has no fixed position and no own close button (the drawer owns closing)', () => {
    render(<NodeDetailPanel embedded />);
    act(() => {
      window.dispatchEvent(new CustomEvent('visionclaw:node-selected', {
        detail: { nodeId: '1', label: 'Loom', connectionCount: 1, neighbors: [{ id: '2', label: 'Model' }] },
      }));
    });
    const root = screen.getByTestId('node-detail-panel');
    expect(root.style.position).not.toBe('fixed');
    expect(screen.queryByRole('button', { name: 'Close node details' })).toBeNull();
  });
});
