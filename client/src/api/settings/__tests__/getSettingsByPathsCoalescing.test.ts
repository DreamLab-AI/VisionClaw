import { describe, it, expect, vi, beforeEach } from 'vitest';

const { get } = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock('axios', () => ({
  default: { get, put: vi.fn(), post: vi.fn(), interceptors: { request: { use: vi.fn() } } },
}));
vi.mock('../../../services/nostrAuthService', () => ({ nostrAuth: { isAuthenticated: () => false } }));
vi.mock('../../../services/api/authInterceptor', () => ({ computeAuthHeaders: vi.fn(async () => ({})) }));
vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

import { getSettingsByPaths, getSettingByPath, invalidateSettingsCache } from '../endpoints';

describe('getSettingsByPaths', () => {
  beforeEach(() => {
    get.mockReset();
    invalidateSettingsCache();
  });

  it('issues one /api/settings/all (one signature) for concurrent per-section reads', async () => {
    let finish!: () => void;
    get.mockImplementation(() => new Promise((res) => { finish = () => res({ data: {} }); }));
    const reads = [
      getSettingByPath('visualisation.graphs'),
      getSettingsByPaths(['system.debug']),
      getSettingsByPaths(['nodeFilter']),
      getSettingByPath('visualisation.rendering'),
    ];
    await new Promise((r) => setTimeout(r, 0));
    finish();
    await Promise.all(reads);
    expect(get).toHaveBeenCalledTimes(1);
  });

  it('invalidateSettingsCache drops a request still in flight', async () => {
    const finishers: Array<() => void> = [];
    get.mockImplementation(() => new Promise((res) => { finishers.push(() => res({ data: {} })); }));
    const first = getSettingsByPaths(['a']);
    await new Promise((r) => setTimeout(r, 0));
    invalidateSettingsCache();
    const second = getSettingsByPaths(['a']);
    await new Promise((r) => setTimeout(r, 0));
    expect(get).toHaveBeenCalledTimes(2);
    finishers.forEach((f) => f());
    await Promise.all([first, second]);
  });
});
