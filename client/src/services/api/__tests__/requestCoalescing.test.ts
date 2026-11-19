import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

import { UnifiedApiClient } from '../UnifiedApiClient';

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

describe('UnifiedApiClient in-flight GET coalescing', () => {
  let fetchMock: ReturnType<typeof vi.fn>;
  let release: Array<() => void>;
  let signed: number;
  let identity: string;
  let client: UnifiedApiClient;

  beforeEach(() => {
    release = [];
    signed = 0;
    identity = 'pk-a';
    // fetch stays pending until released, so callers overlap.
    fetchMock = vi.fn(
      () => new Promise<Response>((res) => release.push(() => res(json({ ok: true })))),
    );
    vi.stubGlobal('fetch', fetchMock);
    client = new UnifiedApiClient('/api');
    client.setIdentityProvider(() => identity);
    client.setInterceptors({
      // Stands in for the NIP-98 interceptor: one call here is one signature.
      onRequest: async (cfg) => { signed++; return cfg; },
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  const settle = () => new Promise((r) => setTimeout(r, 0));

  it('shares one request, and so one signature, between identical concurrent GETs', async () => {
    const calls = [client.get('/x'), client.get('/x'), client.getData('/x')];
    await settle();
    release.forEach((r) => r());
    const results = await Promise.all(calls);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(signed).toBe(1);
    expect(results[0].status).toBe(200);
    expect(client.getRequestStats()).toMatchObject({ issued: 1, coalesced: 2 });
  });

  it('does not share across different URLs or identities', async () => {
    const a = client.get('/x');
    const b = client.get('/y');
    identity = 'pk-b';
    const c = client.get('/x');
    await settle();
    expect(fetchMock).toHaveBeenCalledTimes(3);
    release.forEach((r) => r());
    await Promise.all([a, b, c]);
    expect(signed).toBe(3);
  });

  it('is not a cache: a GET after the first settles signs again', async () => {
    const first = client.get('/x');
    await settle();
    release[0]();
    await first;
    const second = client.get('/x');
    await settle();
    release[1]();
    await second;
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(signed).toBe(2);
  });

  it('shares a failure with every waiter and forgets it', async () => {
    fetchMock.mockImplementationOnce(() => new Promise<Response>((res) => release.push(() => res(json({ error: 'no' }, 500)))));
    client.setRetryConfig({ maxRetries: 0 });
    const a = client.get('/x').catch((e) => e);
    const b = client.get('/x').catch((e) => e);
    await settle();
    release[0]();
    const [ea, eb] = await Promise.all([a, b]);
    expect(ea).toBe(eb);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const again = client.get('/x');
    await settle();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    release[1]();
    await again;
  });

  it('never coalesces mutations, or GETs carrying a body, signal, headers or skipInterceptors', async () => {
    const ac = new AbortController();
    const calls = [
      client.post('/x', { a: 1 }),
      client.post('/x', { a: 1 }),
      client.get('/z', { signal: ac.signal }),
      client.get('/z', { signal: ac.signal }),
      client.get('/h', { headers: { 'X-A': '1' } }),
      client.get('/h', { headers: { 'X-A': '1' } }),
      client.get('/s', { skipInterceptors: true }),
      client.get('/s', { skipInterceptors: true }),
    ];
    await settle();
    expect(fetchMock).toHaveBeenCalledTimes(8);
    release.forEach((r) => r());
    await Promise.all(calls);
  });

  it('a mutation ends sharing, so a later GET cannot receive a pre-write answer', async () => {
    const before = client.get('/x');
    await settle();
    const write = client.put('/x', { v: 2 });
    const after = client.get('/x');
    await settle();
    expect(fetchMock).toHaveBeenCalledTimes(3);
    release.forEach((r) => r());
    await Promise.all([before, write, after]);
  });
});
