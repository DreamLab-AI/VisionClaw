/**
 * Shell smoke test (jsdom). Verifies the WP2 composition:
 *  - the dock renders at rest,
 *  - hotkey '1' opens the Motion group and its 54 controls appear in the DOM,
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

const MOTION = REGISTRY[0];
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
    .filter((id) => !id.endsWith('-readout') && !id.endsWith('-container')).length;
}

let ensureSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  useControlCenterUI.setState({
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

  it('hotkey "1" opens Motion and renders its 54 controls, ensureLoaded fired for its paths', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);

    act(() => {
      fireEvent.keyDown(window, { key: '1' });
    });

    await waitFor(() => expect(countSettings()).toBe(54));
    expect(useControlCenterUI.getState().activeGroup).toBe('motion');
    expect(ensureSpy).toHaveBeenCalledWith(MOTION.loadPaths);
  });

  it('Esc closes the open panel', async () => {
    render(<ControlCenter showStats={false} enableBloom={false} />);

    act(() => {
      fireEvent.keyDown(window, { key: '1' });
    });
    await waitFor(() => expect(countSettings()).toBe(54));

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
          detail: { group: 'motion', testid: REPEL_K_TESTID },
        }),
      );
    });

    await waitFor(() => {
      expect(document.querySelector(`[data-testid="${REPEL_K_TESTID}"]`)).not.toBeNull();
    });
    expect(ensureSpy).toHaveBeenCalledWith(MOTION.loadPaths);

    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe(REPEL_K_TESTID);
    });
  });
});
