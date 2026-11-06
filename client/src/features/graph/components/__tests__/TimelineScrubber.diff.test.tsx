/**
 * ADR-2189 §6 merge: the scrubber's Diff checkbox and `provenance.diffMode`
 * (Explore › Timeline › Timeline diff) are one control. The checkbox writes the
 * setting through the one write path; the timeline store follows the setting,
 * whichever surface flipped it.
 */
import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react';

const h = vi.hoisted(() => ({ writes: vi.fn() }));

vi.mock('../../../control-center/state/settingWrites', async () => {
  const { useSettingsStore } = await import('../../../../store/settingsStore');
  return {
    // apply to the real store so the read side sees the write, and record it
    applySettingWrites: (writes: Array<{ path: string; value: unknown }>) => {
      h.writes(writes);
      for (const w of writes) useSettingsStore.getState().set(w.path, w.value);
    },
  };
});

import TimelineScrubber from '../TimelineScrubber';
import { useTimelineStore } from '../../../../store/timelineStore';
import { useSettingsStore } from '../../../../store/settingsStore';

beforeEach(() => {
  vi.useFakeTimers();
  // keep the network out: the loaders are not under test here
  useTimelineStore.setState({
    loadStateAt: vi.fn(async () => {}),
    loadDiff: vi.fn(async () => {}),
  } as never);
  act(() => useSettingsStore.getState().set('provenance.diffMode', false));
  h.writes.mockClear();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('TimelineScrubber diff = provenance.diffMode', () => {
  it('the checkbox writes the setting, and the store follows', () => {
    render(<TimelineScrubber />);
    const box = screen.getByRole('checkbox', { name: 'Timeline diff' });
    expect(box).not.toBeChecked();
    fireEvent.click(box);
    expect(h.writes).toHaveBeenCalledWith([{ path: 'provenance.diffMode', value: true }]);
    expect(box).toBeChecked();
    expect(useTimelineStore.getState().diffMode).toBe(true);
    act(() => { vi.runAllTimers(); });
    expect(useTimelineStore.getState().loadDiff).toHaveBeenCalled();
  });

  it('flipping the setting elsewhere (settings panel) moves the checkbox and the store', () => {
    render(<TimelineScrubber />);
    act(() => useSettingsStore.getState().set('provenance.diffMode', true));
    expect(screen.getByRole('checkbox', { name: 'Timeline diff' })).toBeChecked();
    expect(useTimelineStore.getState().diffMode).toBe(true);
    act(() => useSettingsStore.getState().set('provenance.diffMode', false));
    expect(screen.getByRole('checkbox', { name: 'Timeline diff' })).not.toBeChecked();
    expect(useTimelineStore.getState().diffMode).toBe(false);
  });
});
