import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

import { UnifiedApiClient } from '../UnifiedApiClient';

const ok = () =>
  new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } });

describe('UnifiedApiClient URL building', () => {
  let fetchMock: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchMock = vi.fn(async () => ok());
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('never produces /api/api when a caller repeats the base prefix', async () => {
    const c = new UnifiedApiClient('/api');
    await c.get('/api/admin/rbac/whoami');
    expect(fetchMock.mock.calls[0][0]).toBe('/api/admin/rbac/whoami');
  });

  it('leaves a path that merely starts with /api-like text alone', async () => {
    const c = new UnifiedApiClient('/api');
    await c.get('/apiary/x');
    expect(fetchMock.mock.calls[0][0]).toBe('/api/apiary/x');
  });
});

const sources = import.meta.glob(
  ['../../../**/*.{ts,tsx}', '!../../../**/*.test.*', '!../../../**/__tests__/**'],
  { query: '?raw', import: 'default', eager: true },
) as Record<string, string>;

describe('call sites', () => {
  it('no source file hands unifiedApiClient a path that starts with /api/', () => {
    const re = /unifiedApiClient\.\w+(?:<[^()]*>)?\(\s*[`'"]\/api\//;
    expect(Object.keys(sources).length).toBeGreaterThan(100);
    const offenders = Object.keys(sources).filter((f) => re.test(sources[f]));
    expect(offenders).toEqual([]);
  });
});
