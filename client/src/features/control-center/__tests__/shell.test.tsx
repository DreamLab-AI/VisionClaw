/**
 * Shell smoke test (jsdom). Verifies the WP2 composition:
 *  - the dock renders at rest,
 *  - hotkey '4' opens the Layout section (ADR-2189) and its 82 controls appear in the DOM,
 *  - opening a group triggers ensureLoaded(group.loadPaths),
 *  - Esc closes the panel,
 *  - a `controlcenter:reveal` event opens the group and focuses the target control.
 *
 * Network is fully mocked: settingsApi is stubbed and the store's ensureLoaded is
 * spied to a resolved no-op, so no fetch is attempted. Exhaustive per-field DOM
 * coverage is the browser-automation phase (WP-plan task #7), not this test.
 */

import React from 'react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, act, cleanup } from '@testing-library/react';
import { ControlCenter } from '../ControlCenter';
import { useControlCenterUI } from '../state/useControlCenterUI';
import { useSettingsStore } from '../../../store/settingsStore';
import { REGISTRY } from '../registry/settingsRegistry';

// settingsApi.getSettingsByPaths is the only network surface the ensure/hydration
// path can reach; stub it so nothing hits the wire even if a spy is bypassed.
vi.mock('../../../api/settingsApi', () => ({
  settingsApi: {
    getSettingsByPaths: vi.fn(async () => ({})),
    updateSettingsByPaths: vi.fn(async () => ({})),
    getAllSettings: vi.fn(async () => ({})),
  },
}));

const LAYOUT = REGISTRY.find((g) => g.id === 'layout')!;
const REPEL_K_TESTID = 'setting-visualisation.graphs.knowledge.physics.repelK';

/**
 * Count the CANONICAL field controls only. WP1's SettingSlider/NostrAuthControl
 * attach secondary testids (`…-readout`, `…-container`) that also start with
 * `setting-`; those are not fields, so exclude them to get the one-per-field
 * count (§9.1: the canonical field testid is exactly `setting-{path}`).
 */
function countSettings(): number {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-testid^="setting-"]'))
    .map((el) => el.getAttribute('data-testid') ?? '')
    .filter((id) => !id.endsWith('-readout') && !id.endsWith('-container') && !id.endsWith('-scope')).length;
}

let ensureSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  useControlCenterUI.setState({
    drawerTab: null,
    previousDrawerTab: null,
    shortcutSheetOpen: false,
    openPanel: false,
    activeGroup: null,
    dockCollapsed: false,
    echoPulseEnabled: false,
  });
  ensureSpy = vi
    .spyOn(useSettingsStore.getState(), 'ensureLoaded')
    .mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('ControlCenter shell', () => {
  it('renders the dock at rest with no settings surfaced', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    expect(screen.getByTestId('control-center-dock')).toBeInTheDocument();
    expect(screen.getByTestId('macro-bar')).toBeInTheDocument();
    // The status surface is no longer part of ControlCenter's composition — it
    // mounts as a sibling badge in MainLayout (see StatusSurface).
    // At rest the panel body is not rendered → zero setting controls in the DOM.
    expect(countSettings()).toBe(0);
  });

  it('hotkey "4" opens Layout and renders its 82 controls, ensureLoaded fired for its paths', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);

    act(() => {
      fireEvent.keyDown(window, { key: '4' });
    });

    await waitFor(() => expect(countSettings()).toBe(LAYOUT.fields.length));
    expect(LAYOUT.fields.length).toBe(82); // + the two ontology-constraint keys (ADR-2192)
    expect(useControlCenterUI.getState().activeGroup).toBe('layout');
    expect(ensureSpy).toHaveBeenCalledWith(LAYOUT.loadPaths);
  });

  it('Esc closes the open panel', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);

    act(() => {
      fireEvent.keyDown(window, { key: '4' });
    });
    await waitFor(() => expect(countSettings()).toBe(82));

    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    await waitFor(() => expect(countSettings()).toBe(0));
    expect(useControlCenterUI.getState().openPanel).toBe(false);
  });

  it('reveal event opens the target group and focuses the control', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);

    act(() => {
      window.dispatchEvent(
        new CustomEvent('controlcenter:reveal', {
          detail: { group: 'layout', testid: REPEL_K_TESTID },
        }),
      );
    });

    await waitFor(() => {
      expect(document.querySelector(`[data-testid="${REPEL_K_TESTID}"]`)).not.toBeNull();
    });
    expect(ensureSpy).toHaveBeenCalledWith(LAYOUT.loadPaths);

    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe(REPEL_K_TESTID);
    });
  });

  it('every advertised hotkey opens its section (ADR-2189: no dead key)', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    const advertised = Array.from(document.querySelectorAll<HTMLElement>('[data-testid^="dock-group-"]'));
    expect(advertised).toHaveLength(7);
    for (const btn of advertised) {
      const key = btn.getAttribute('aria-keyshortcuts')!;
      expect(btn.getAttribute('aria-label')).toContain(`(${key})`);
      act(() => {
        fireEvent.keyDown(window, { key });
      });
      expect(useControlCenterUI.getState().activeGroup).toBe(btn.getAttribute('data-testid')!.replace('dock-group-', ''));
    }
  });

  it('"?" opens the shortcut sheet, which lists the seven section keys', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    act(() => {
      fireEvent.keyDown(window, { key: '?' });
    });
    const sheet = await screen.findByTestId('shortcut-sheet');
    for (const g of REGISTRY) expect(sheet.textContent).toContain(`Open ${g.label}`);
  });

  it('the palette command "show-keyboard-shortcuts" also opens the sheet', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    act(() => {
      window.dispatchEvent(new CustomEvent('show-keyboard-shortcuts'));
    });
    expect(await screen.findByTestId('shortcut-sheet')).toBeInTheDocument();
  });

  it('"/" opens the explore panel drawer, and Esc closes it', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    act(() => {
      fireEvent.keyDown(window, { key: '/' });
    });
    expect(useControlCenterUI.getState().drawerTab).toBe('explore');
    expect(screen.getByTestId('right-drawer')).toBeVisible();
    expect(screen.getByTestId('explore-panel')).toBeInTheDocument();
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(useControlCenterUI.getState().drawerTab).toBeNull();
  });

  it('a collapsed dock is inert (out of the tab order), not only aria-hidden', () => {
    useControlCenterUI.setState({ dockCollapsed: true });
    render(<ControlCenter showStats={false} enableBloom={false} />);
    const body = document.getElementById('control-center-dock')!;
    expect(body.hasAttribute('inert')).toBe(true);
  });

  it('dials announce what they set (aria-valuetext) and mark the shared ones', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    const spread = screen.getByTestId('macro-density');
    expect(spread.getAttribute('aria-label')).toBe('Spread');
    expect(spread.getAttribute('aria-valuetext')).toMatch(/repulsion .*Changes the layout for everyone/);
    expect(screen.getByTestId('macro-luminosity').getAttribute('aria-valuetext')).toMatch(/Saved on the server for every viewer/);
    expect(screen.getByTestId('macro-luminosity').getAttribute('aria-label')).toBe('Glow');
    expect(screen.getByTestId('macro-focus').getAttribute('aria-label')).toBe('Labels');
    expect(screen.getByTestId('macro-focus').getAttribute('aria-valuetext')).toMatch(/labels/);
    expect(screen.queryByTestId('macro-luminosity-shared')).toBeNull();
    expect(screen.getByTestId('macro-density-shared')).toBeInTheDocument();
  });

  it('Fit asks the camera for the stored scope; Legend and Explore are wired to the lane components', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    const onFit = vi.fn();
    window.addEventListener('visionclaw:camera-fit', onFit);
    fireEvent.click(screen.getByTestId('dock-fit'));
    window.removeEventListener('visionclaw:camera-fit', onFit);
    expect(onFit).toHaveBeenCalledTimes(1);
    // no target in the event = the stored graphView.fitTarget (ADR-2180)
    expect((onFit.mock.calls[0][0] as CustomEvent).detail).toEqual({});
    // the legend host is always mounted (GraphLegend installs the encoder)
    expect(screen.getByTestId('legend-slot')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('dock-explore'));
    expect(screen.getByTestId('explore-panel')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('right-drawer-close'));
    expect(screen.getByTestId('right-drawer')).not.toBeVisible();
    expect(screen.getByTestId('dock-legend')).toBeInTheDocument();
  });


  it('Esc inside the drawer closes only the drawer tab, once, and leaves the settings panel open', () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);
    act(() => {
      useControlCenterUI.getState().openGroup('encode');
      useControlCenterUI.getState().openDrawer('explore');
    });
    act(() => {
      window.dispatchEvent(new CustomEvent('visionclaw:node-selected', {
        detail: { nodeId: '1', label: 'Loom', connectionCount: 0, neighbors: [] },
      }));
    });
    expect(useControlCenterUI.getState().drawerTab).toBe('node');
    const nodeTab = screen.getByRole('tab', { name: 'Node' });
    nodeTab.focus();
    act(() => {
      fireEvent.keyDown(nodeTab, { key: 'Escape' });
    });
    // node tab closed back to explore — not closed twice, panel untouched
    expect(useControlCenterUI.getState().drawerTab).toBe('explore');
    expect(useControlCenterUI.getState().openPanel).toBe(true);
    // Esc with focus outside the drawer still closes the settings panel first
    act(() => {
      fireEvent.keyDown(document.body, { key: 'Escape' });
    });
    expect(useControlCenterUI.getState().openPanel).toBe(false);
    expect(useControlCenterUI.getState().drawerTab).toBe('explore');
  });
});

