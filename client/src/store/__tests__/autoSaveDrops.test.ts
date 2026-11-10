// A settings change that does not reach the server must be visible to the user and
// remembered across a reload, never only logged at debug level.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const mockUpdateSettingsByPaths = vi.fn();
vi.mock('../../api/settingsApi', () => ({
  settingsApi: { updateSettingsByPaths: (...a: unknown[]) => mockUpdateSettingsByPaths(...a) },
}));
const mockAddFailedUpdate = vi.fn();
vi.mock('../settingsRetryManager', () => ({
  settingsRetryManager: { addFailedUpdate: (...a: unknown[]) => mockAddFailedUpdate(...a) },
}));

const mockReloadFromServer = vi.fn();
vi.mock('../settingsStore', () => ({
  useSettingsStore: { getState: () => ({ reloadFromServer: (...a: unknown[]) => mockReloadFromServer(...a) }) },
}));

import { AutoSaveManager, httpStatus } from '../autoSaveManager';
import { useSettingsSyncStatus, isUnsyncedPath } from '../settingsSyncStatus';

const GLOW = 'visualisation.glow.intensity';
const OPACITY = 'visualisation.graphs.knowledge.edges.opacity';

describe('autoSaveManager: changes that do not reach the server', () => {
  let m: AutoSaveManager;
  beforeEach(() => {
    vi.useFakeTimers();
    mockUpdateSettingsByPaths.mockReset().mockResolvedValue(undefined);
    mockAddFailedUpdate.mockReset();
    useSettingsSyncStatus.getState().clear();
    m = new AutoSaveManager();
  });
  afterEach(() => vi.useRealTimers());

  it('a change made before initialisation is held, flagged, and sent once initialised', async () => {
    m.queueChange(GLOW, 0.5);
    expect(useSettingsSyncStatus.getState().unsaved[GLOW]).toBe('not-ready');
    m.setInitialized(true);
    await vi.runAllTimersAsync();
    expect(mockUpdateSettingsByPaths).toHaveBeenCalledWith([{ path: GLOW, value: 0.5 }]);
    expect(useSettingsSyncStatus.getState().unsaved[GLOW]).toBeUndefined();
  });

  it('a change made while sync is off is flagged as local-only', async () => {
    m.setInitialized(true);
    m.setSyncEnabled(false);
    m.queueChange(GLOW, 0.5);
    await vi.runAllTimersAsync();
    expect(mockUpdateSettingsByPaths).not.toHaveBeenCalled();
    expect(useSettingsSyncStatus.getState().unsaved[GLOW]).toBe('sync-off');
  });

  it('the batched entry point (updateSettings) honours sync-off too', async () => {
    m.setInitialized(true);
    m.setSyncEnabled(false);
    m.queueChanges(new Map([[GLOW, 0.5], [OPACITY, 0.1]]));
    await vi.runAllTimersAsync();
    expect(mockUpdateSettingsByPaths).not.toHaveBeenCalled();
    expect(useSettingsSyncStatus.getState().unsaved).toEqual({ [GLOW]: 'sync-off', [OPACITY]: 'sync-off' });
  });

  it('a write the server refuses (400) is reverted to the server value, not retried (ADR-2192)', async () => {
    const STRENGTH = 'visualisation.graphs.knowledge.physics.ontologyConstraintStrength';
    m.setInitialized(true);
    mockReloadFromServer.mockReset().mockResolvedValue(undefined);
    mockUpdateSettingsByPaths.mockRejectedValueOnce(
      Object.assign(new Error('Request failed with status code 400'), { response: { status: 400 } }),
    );
    m.queueChange(STRENGTH, 1.5);
    await m.forceFlush();
    expect(mockReloadFromServer).toHaveBeenCalledWith([STRENGTH]);
    expect(mockAddFailedUpdate).not.toHaveBeenCalled();
    expect(useSettingsSyncStatus.getState().unsaved[STRENGTH]).toBe('rejected');
    expect(httpStatus({ response: { status: 500 } })).toBe(500);
    expect(httpStatus(new Error('x'))).toBeUndefined();
  });

  it('a failed write is flagged until a later write of that path succeeds', async () => {
    m.setInitialized(true);
    mockUpdateSettingsByPaths.mockRejectedValueOnce(new Error('HTTP 500'));
    m.queueChange(OPACITY, 0.1);
    await vi.runAllTimersAsync();
    expect(mockAddFailedUpdate).toHaveBeenCalledWith(OPACITY, 0.1, 'HTTP 500');
    expect(useSettingsSyncStatus.getState().unsaved[OPACITY]).toBe('failed');
    m.queueChange(OPACITY, 0.12);
    await vi.runAllTimersAsync();
    expect(useSettingsSyncStatus.getState().unsaved[OPACITY]).toBeUndefined();
  });

  it('browser-only paths (no server bucket) are not reported: they are saved where they belong', async () => {
    m.setInitialized(true);
    m.setSyncEnabled(false);
    m.queueChange('provenance.showAttribution', true);
    expect(useSettingsSyncStatus.getState().unsaved).toEqual({});
  });

  it('isUnsyncedPath matches the path, its ancestors and its descendants', () => {
    useSettingsSyncStatus.getState().markUnsaved([OPACITY], 'failed');
    expect(isUnsyncedPath(OPACITY)).toBe(true);
    expect(isUnsyncedPath('visualisation.graphs.knowledge.edges')).toBe(true);
    expect(isUnsyncedPath(`${OPACITY}.x`)).toBe(true);
    expect(isUnsyncedPath('visualisation.graphs.knowledge.edges.baseWidth')).toBe(false);
  });

  it('the unsaved set is persisted, so a reload still knows which local values are newer', () => {
    useSettingsSyncStatus.getState().markUnsaved([GLOW], 'sync-off');
    const raw = localStorage.getItem('graph-viz-settings-unsynced');
    expect(raw && JSON.parse(raw).state.unsaved).toEqual({ [GLOW]: 'sync-off' });
  });
});
