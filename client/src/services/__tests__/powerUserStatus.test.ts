import { describe, it, expect, vi, beforeEach } from 'vitest';

const { get } = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock('../api/UnifiedApiClient', () => ({ unifiedApiClient: { get } }));

import { fetchPowerUserStatus } from '../powerUserStatus';

const KEY = 'ab'.repeat(32);

describe('fetchPowerUserStatus', () => {
  beforeEach(() => get.mockReset());

  it('reads the flag from the verified whoami endpoint', async () => {
    get.mockResolvedValue({ data: { pubkey: KEY, role: 'admin', is_power_user: true } });
    await expect(fetchPowerUserStatus(KEY)).resolves.toBe(true);
    expect(get).toHaveBeenCalledWith('/api/admin/rbac/whoami');
  });

  it('reports false for an ordinary user', async () => {
    get.mockResolvedValue({ data: { pubkey: KEY, role: 'editor', is_power_user: false } });
    await expect(fetchPowerUserStatus(KEY)).resolves.toBe(false);
  });

  it('returns null when the request fails, so the caller keeps its value', async () => {
    get.mockRejectedValueOnce(new Error('403'));
    const result = await fetchPowerUserStatus(KEY);
    expect(result).toBeNull();
  });

  it('ignores an answer about a different key', async () => {
    get.mockResolvedValue({ data: { pubkey: 'cd'.repeat(32), role: 'admin', is_power_user: true } });
    await expect(fetchPowerUserStatus(KEY)).resolves.toBeNull();
  });
});
