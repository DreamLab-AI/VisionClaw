/**
 * ADR-2189 §3: one right-hand drawer with Explore, Memory and Node tabs.
 * Covers the occlusion fix (one place on screen), the node tab's Esc and focus
 * behaviour (audit finding 15: NodeDetailPanel had no Escape and no focus
 * handling) and the memory tab following the embedding-cloud setting.
 */
import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, cleanup, waitFor } from '@testing-library/react';
import { useControlCenterUI } from '../../state/useControlCenterUI';
import { useSettingsStore } from '../../../../store/settingsStore';

vi.mock('../../integrationSlots', () => ({
  ExplorePanel: () => (
    <div data-testid="explore-panel">
      <input aria-label="Search nodes" data-explore-search="" />
    </div>
  ),
}));
vi.mock('../../../visualisation/memoryCloud/MemoryExplorerPanel', () => ({
  default: ({ embedded }: { embedded?: boolean }) => <div data-testid="memory-explorer-panel" data-embedded={String(!!embedded)} />,
}));
const h = vi.hoisted(() => ({ throwNode: false }));
vi.mock('../../../graph/components/NodeDetailPanel', () => ({
  NodeDetailPanel: ({ embedded }: { embedded?: boolean }) => {
    if (h.throwNode) throw new TypeError("Cannot read properties of undefined (reading 'length')");
    return <div data-testid="node-detail-panel" data-embedded={String(!!embedded)} />;
  },
}));

import { RightDrawer } from '../RightDrawer';

function setCloud(enabled: boolean) {
  const cur = useSettingsStore.getState().settings as Record<string, any>;
  act(() => {
    useSettingsStore.setState({
      settings: { ...cur, visualisation: { ...cur?.visualisation, embeddingCloud: { ...cur?.visualisation?.embeddingCloud, enabled } } },
    } as never);
  });
}

function selectNode(label: string | null) {
  act(() => {
    window.dispatchEvent(new CustomEvent('visionclaw:node-selected', { detail: label ? { nodeId: '1', label } : null }));
  });
}

beforeEach(() => {
  useControlCenterUI.setState({ drawerTab: null, previousDrawerTab: null });
  setCloud(false);
});
afterEach(() => cleanup());

describe('RightDrawer', () => {
  it('is hidden and inert while closed, so nothing behind it takes focus', () => {
    render(<RightDrawer />);
    const root = screen.getByTestId('right-drawer');
    expect(root).not.toBeVisible();
    expect(root.hasAttribute('inert')).toBe(true);
  });

  it('a node selection opens the Node tab without taking focus, and announces it', () => {
    render(<><button data-testid="elsewhere">x</button><RightDrawer /></>);
    selectNode('Ontology Loom');
    expect(useControlCenterUI.getState().drawerTab).toBe('node');
    expect(screen.getByRole('tab', { name: 'Node' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByTestId('right-drawer-announce')).toHaveTextContent('Node details: Ontology Loom');
    expect(screen.getByTestId('node-detail-panel')).toHaveAttribute('data-embedded', 'true');
    // a canvas click leaves focus where it was
    expect(document.activeElement).toBe(document.body);
  });

  it('closing the Node tab deselects the node and returns to the tab shown before it', () => {
    const deselect = vi.fn();
    window.addEventListener('visionclaw:node-deselect', deselect);
    render(<RightDrawer />);
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    selectNode('A');
    expect(useControlCenterUI.getState().drawerTab).toBe('node');
    fireEvent.click(screen.getByRole('button', { name: 'Close node details' }));
    window.removeEventListener('visionclaw:node-deselect', deselect);
    expect(deselect).toHaveBeenCalledTimes(1);
    expect(useControlCenterUI.getState().drawerTab).toBe('explore');
    expect(screen.queryByRole('tab', { name: 'Node' })).toBeNull();
  });

  it('a deselection from the canvas closes the Node tab', () => {
    render(<RightDrawer />);
    selectNode('A');
    selectNode(null);
    expect(useControlCenterUI.getState().drawerTab).toBeNull();
  });

  it('Esc inside the drawer closes the active tab and focus returns to the opener', async () => {
    render(<><button data-testid="opener" onClick={() => useControlCenterUI.getState().openDrawer('explore')}>Explore</button><RightDrawer /></>);
    const opener = screen.getByTestId('opener');
    opener.focus();
    fireEvent.click(opener);
    // focus moves into the drawer, onto the active tab
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Explore' })));
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(useControlCenterUI.getState().drawerTab).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('Esc on the Node tab closes it like the close button (deselects)', () => {
    const deselect = vi.fn();
    window.addEventListener('visionclaw:node-deselect', deselect);
    render(<RightDrawer />);
    selectNode('A');
    fireEvent.keyDown(screen.getByTestId('right-drawer'), { key: 'Escape' });
    window.removeEventListener('visionclaw:node-deselect', deselect);
    expect(deselect).toHaveBeenCalledTimes(1);
    expect(useControlCenterUI.getState().drawerTab).toBeNull();
  });

  it('the memory tab follows the embedding cloud: shown on switch-on, gone on switch-off', () => {
    render(<RightDrawer />);
    expect(screen.queryByRole('tab', { name: 'Memory' })).toBeNull();
    setCloud(true);
    expect(useControlCenterUI.getState().drawerTab).toBe('memory');
    expect(screen.getByTestId('memory-explorer-panel')).toHaveAttribute('data-embedded', 'true');
    setCloud(false);
    expect(useControlCenterUI.getState().drawerTab).toBeNull();
    expect(screen.queryByTestId('memory-explorer-panel')).toBeNull();
  });

  it('with the cloud on, explore and node detail share the one drawer: one panel visible at a time', () => {
    render(<RightDrawer />);
    setCloud(true);
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    selectNode('A');
    const visible = ['explore', 'memory', 'node'].filter((t) => screen.getByTestId(`drawer-panel-${t}`).hidden === false);
    expect(visible).toEqual(['node']);
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual(['Explore', 'Memory', 'Node']);
  });

  it('arrow keys move between tabs (roving tabindex)', () => {
    render(<RightDrawer />);
    setCloud(true);
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    const explore = screen.getByRole('tab', { name: 'Explore' });
    expect(explore).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('tab', { name: 'Memory' })).toHaveAttribute('tabindex', '-1');
    fireEvent.keyDown(explore, { key: 'ArrowRight' });
    expect(useControlCenterUI.getState().drawerTab).toBe('memory');
    expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Memory' }));
    fireEvent.keyDown(document.activeElement!, { key: 'Home' });
    expect(useControlCenterUI.getState().drawerTab).toBe('explore');
  });

  it('a panel that throws shows a message in its tab; the drawer and its controls stay', () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    h.throwNode = true;
    render(<RightDrawer />);
    selectNode('Broken');
    expect(screen.getByTestId('drawer-panel-error')).toHaveTextContent('node detail panel could not be shown');
    expect(screen.getByRole('button', { name: 'Close node details' })).toBeInTheDocument();
    h.throwNode = false;
    err.mockRestore();
  });
});
