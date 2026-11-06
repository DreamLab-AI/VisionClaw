// A settings change the server did not get is shown on the status chip, not only logged.
import React from 'react';
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup, act } from '@testing-library/react';
import { StatusSurface } from '../StatusSurface';
import { useSettingsSyncStatus } from '../../../../store/settingsSyncStatus';

vi.mock('../../../bots/contexts/BotsDataContext', () => ({ useBotsDataOptional: () => null }));
vi.mock('../StatusFlyout', () => ({ StatusFlyout: () => <div data-testid="status-flyout" /> }));

beforeEach(() => useSettingsSyncStatus.getState().clear());
afterEach(() => cleanup());

describe('StatusSurface: unsaved settings notice', () => {
  it('shows nothing when every change reached the server', () => {
    render(<StatusSurface />);
    expect(screen.queryByTestId('status-unsaved-settings')).toBeNull();
  });

  it('shows how many changes are not on the server, and why', () => {
    render(<StatusSurface />);
    act(() => {
      useSettingsSyncStatus.getState().markUnsaved(['visualisation.glow.intensity'], 'failed');
      useSettingsSyncStatus.getState().markUnsaved(['visualisation.graphs.knowledge.edges.opacity'], 'sync-off');
    });
    const notice = screen.getByTestId('status-unsaved-settings');
    expect(notice.textContent).toContain('2');
    expect(notice.getAttribute('title')).toMatch(/1 failed to save to the server/);
    expect(notice.getAttribute('title')).toMatch(/1 kept in this browser only \(sync is off\)/);
    expect(screen.getByTestId('status-surface').getAttribute('aria-label')).toMatch(/2 settings not saved/);
  });

  it('clears once the change is saved', () => {
    render(<StatusSurface />);
    act(() => useSettingsSyncStatus.getState().markUnsaved(['visualisation.glow.intensity'], 'failed'));
    act(() => useSettingsSyncStatus.getState().markSaved(['visualisation.glow.intensity']));
    expect(screen.queryByTestId('status-unsaved-settings')).toBeNull();
  });
});
