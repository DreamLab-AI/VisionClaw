/**
 * The settings panel and the right-hand drawer never overlap (ADR-2189).
 * Below the width where both fit (~830 px at default widths, or at any width
 * with the panel dragged wide) the one opened last wins.
 */
import React from 'react';
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, act, cleanup } from '@testing-library/react';
import { useControlCenterUI, PANEL_DEFAULT_WIDTH } from '../../state/useControlCenterUI';
import { useExclusiveSidePanels, sidePanelsCollide } from '../useExclusiveSidePanels';

const Harness: React.FC = () => {
  useExclusiveSidePanels();
  return null;
};

const realWidth = window.innerWidth;
function setViewport(width: number) {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: width });
}

beforeEach(() => {
  useControlCenterUI.setState({
    openPanel: false, activeGroup: null, drawerTab: null, previousDrawerTab: null, panelWidth: PANEL_DEFAULT_WIDTH,
  });
});
afterEach(() => {
  cleanup();
  setViewport(realWidth);
});

describe('useExclusiveSidePanels', () => {
  it('collision maths: default widths fit at 1024 and 830, not at 800', () => {
    const ui = useControlCenterUI.getState();
    expect(sidePanelsCollide(ui, 1024)).toBe(false);
    expect(sidePanelsCollide(ui, 830)).toBe(false);
    expect(sidePanelsCollide(ui, 800)).toBe(true);
    // a panel dragged to 700 px collides even at 1024
    expect(sidePanelsCollide({ ...ui, panelWidth: 700 }, 1024)).toBe(true);
  });

  it('at 800 px, opening the drawer closes the settings panel', () => {
    setViewport(800);
    render(<Harness />);
    act(() => useControlCenterUI.getState().openGroup('encode'));
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    const s = useControlCenterUI.getState();
    expect(s.drawerTab).toBe('explore');
    expect(s.openPanel).toBe(false);
  });

  it('at 800 px, opening a section hides the drawer but keeps its tab (no deselect)', () => {
    setViewport(800);
    render(<Harness />);
    act(() => useControlCenterUI.getState().openDrawer('node'));
    act(() => useControlCenterUI.getState().openGroup('layout'));
    const s = useControlCenterUI.getState();
    expect(s.openPanel).toBe(true);
    expect(s.drawerTab).toBeNull();
    expect(s.previousDrawerTab).toBe('node');
  });

  it('at 1024 px with default widths both stay open', () => {
    setViewport(1024);
    render(<Harness />);
    act(() => useControlCenterUI.getState().openGroup('encode'));
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    const s = useControlCenterUI.getState();
    expect(s.openPanel).toBe(true);
    expect(s.drawerTab).toBe('explore');
  });

  it('dragging the panel wide hides the drawer instead of closing the panel mid-drag', () => {
    setViewport(1280);
    render(<Harness />);
    act(() => useControlCenterUI.getState().openGroup('encode'));
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    act(() => useControlCenterUI.getState().setPanelWidth(860));
    const s = useControlCenterUI.getState();
    expect(s.openPanel).toBe(true);
    expect(s.drawerTab).toBeNull();
  });

  it('a window resize that stops them fitting closes the settings panel', () => {
    setViewport(1024);
    render(<Harness />);
    act(() => useControlCenterUI.getState().openGroup('encode'));
    act(() => useControlCenterUI.getState().openDrawer('explore'));
    setViewport(800);
    act(() => { window.dispatchEvent(new Event('resize')); });
    const s = useControlCenterUI.getState();
    expect(s.openPanel).toBe(false);
    expect(s.drawerTab).toBe('explore');
  });
});
