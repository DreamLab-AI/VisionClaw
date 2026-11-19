/**
 * ADR-2205 — one NIP-07 signature per session.
 *
 * Real nostr-tools throughout (no crypto mocks): the bind request and the
 * session-signed tokens are decoded and their signatures verified. jsdom hands
 * noble a Node-realm `TextEncoder` result that fails its `instanceof Uint8Array`
 * guard, so `TextEncoder` is wrapped to return in-realm arrays.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { finalizeEvent, generateSecretKey, getPublicKey, verifyEvent } from 'nostr-tools/pure';
import { bytesToHex } from '@noble/hashes/utils';

// nostr-tools captures `new TextEncoder()` at import, so wrap it before the imports run.
vi.hoisted(() => {
  const NodeEncoder = globalThis.TextEncoder;
  (globalThis as any).TextEncoder = class {
    encoding = 'utf-8';
    encode(input?: string) { return new Uint8Array(new NodeEncoder().encode(input)); }
  };
});

vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

import { nostrAuth } from '../nostrAuthService';
import { UnifiedApiClient } from '../api/UnifiedApiClient';
import { computeAuthHeaders, initializeAuthInterceptor } from '../api/authInterceptor';

const ORIGIN = 'http://localhost:3001';
const BIND_URL = `${ORIGIN}/api/auth/nostr/session-key`;

const realSecret = generateSecretKey();
const realPub = getPublicKey(realSecret);

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

function decode(authorization: string) {
  expect(authorization.startsWith('Nostr ')).toBe(true);
  return JSON.parse(atob(authorization.slice('Nostr '.length)));
}

describe('bound session key (ADR-2205)', () => {
  let signEvent: ReturnType<typeof vi.fn>;
  let fetchMock: ReturnType<typeof vi.fn>;
  let binds: Array<{ headers: Record<string, string>; body: string }>;
  let apiBehaviour: (n: number) => Response;
  let apiCalls: number;

  beforeEach(() => {
    vi.stubGlobal('location', {
      hostname: 'localhost', origin: ORIGIN, href: `${ORIGIN}/`, protocol: 'http:', host: 'localhost:3001',
    });
    const store = (): Storage => {
      const m = new Map<string, string>();
      return {
        get length() { return m.size; },
        clear: () => m.clear(),
        getItem: (k: string) => m.get(k) ?? null,
        key: (i: number) => [...m.keys()][i] ?? null,
        removeItem: (k: string) => void m.delete(k),
        setItem: (k: string, v: string) => void m.set(k, v),
      };
    };
    vi.stubGlobal('localStorage', store());
    vi.stubGlobal('sessionStorage', store());
    vi.stubEnv('DEV', false);
    vi.stubEnv('VITE_DEV_MODE_AUTH', 'false');
    signEvent = vi.fn(async (e: any) => finalizeEvent(e, realSecret));
    (window as any).nostr = { getPublicKey: vi.fn(async () => realPub), signEvent };
    binds = [];
    apiCalls = 0;
    apiBehaviour = () => json({ ok: true });
    fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === BIND_URL) {
        binds.push({ headers: init!.headers as Record<string, string>, body: init!.body as string });
        return json({ sessionPubkey: JSON.parse(init!.body as string).sessionPubkey, expiresAt: Math.floor(Date.now() / 1000) + 600 });
      }
      apiCalls += 1;
      return apiBehaviour(apiCalls);
    });
    vi.stubGlobal('fetch', fetchMock);
    (nostrAuth as any).initialized = false;
    (nostrAuth as any).currentUser = null;
    (nostrAuth as any).localPrivateKey = null;
  });

  afterEach(async () => {
    await nostrAuth.logout();
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    delete (window as any).nostr;
  });

  it('binds once at sign-in; N later requests cause ZERO window.nostr.signEvent calls', async () => {
    await nostrAuth.login();
    expect(signEvent).toHaveBeenCalledTimes(1); // the bind itself
    expect(binds).toHaveLength(1);
    signEvent.mockClear();

    const tokens: string[] = [];
    for (let i = 0; i < 8; i++) {
      const headers = await computeAuthHeaders(`${ORIGIN}/api/settings/all?i=${i}`, 'GET');
      tokens.push(headers.Authorization);
    }
    expect(signEvent).toHaveBeenCalledTimes(0);

    const bound = JSON.parse(binds[0].body);
    const bindEvent = decode(binds[0].headers.Authorization);
    expect(bindEvent.pubkey).toBe(realPub);
    expect(bindEvent.tags.find((t: string[]) => t[0] === 'payload')).toBeTruthy();
    expect(bindEvent.tags.find((t: string[]) => t[0] === 'session')).toBeUndefined();
    expect(verifyEvent(bindEvent)).toBe(true);

    for (const t of tokens) {
      const ev = decode(t);
      expect(ev.pubkey).toBe(bound.sessionPubkey);
      expect(ev.pubkey).not.toBe(realPub);
      expect(ev.tags).toContainEqual(['session', realPub]);
      expect(verifyEvent(ev)).toBe(true);
    }
    expect(new Set(tokens).size).toBe(tokens.length);
  });

  it('never persists the session secret', async () => {
    await nostrAuth.login();
    const sk = (nostrAuth as any).sessionKey?.secret as Uint8Array | undefined;
    expect(sk).toBeInstanceOf(Uint8Array);
    const hex = bytesToHex(sk!);
    expect(JSON.stringify([...Array(localStorage.length).keys()].map((i) => localStorage.getItem(localStorage.key(i)!)))).not.toContain(hex);
    expect(JSON.stringify([...Array(sessionStorage.length).keys()].map((i) => sessionStorage.getItem(sessionStorage.key(i)!)))).not.toContain(hex);
  });

  it('uses the real signer for requests the server refuses a session key', async () => {
    await nostrAuth.login();
    signEvent.mockClear();
    await computeAuthHeaders(`${ORIGIN}/api/auth/nostr/api-keys`, 'POST', '{}');
    await computeAuthHeaders(`${ORIGIN}/api/admin/rbac/users/abc/role`, 'PUT', '{}');
    await computeAuthHeaders(`${ORIGIN}/api/auth/nostr/session-key`, 'POST', '{}');
    expect(signEvent).toHaveBeenCalledTimes(3);
    // The Solid proxy forwards the token upstream, so it needs the real signer too.
    await computeAuthHeaders(`${ORIGIN}/solid/pods/init`, 'POST', '{}');
    await computeAuthHeaders(`${ORIGIN}/api/solid/x/`, 'GET');
    await computeAuthHeaders(`${ORIGIN}/api/pay/charge`, 'POST', '{}');
    expect(signEvent).toHaveBeenCalledTimes(6);
    signEvent.mockClear();
    // Reading RBAC state stays on the session key.
    await computeAuthHeaders(`${ORIGIN}/api/admin/rbac/whoami`, 'GET');
    expect(signEvent).toHaveBeenCalledTimes(0);
  });

  it('shares one bind between concurrent first requests', async () => {
    await nostrAuth.login();
    await nostrAuth.logout();
    (nostrAuth as any).currentUser = { pubkey: realPub, npub: 'n', isPowerUser: false };
    binds.length = 0;
    await Promise.all(Array.from({ length: 6 }, (_, i) => computeAuthHeaders(`${ORIGIN}/api/x?${i}`, 'GET')));
    expect(binds).toHaveLength(1);
  });

  it('re-binds exactly once and retries exactly once on session_key_expired', async () => {
    const client = new UnifiedApiClient('/api');
    initializeAuthInterceptor(client);
    await nostrAuth.login();
    expect(binds).toHaveLength(1);
    apiBehaviour = (n) => (n === 1 ? json({ code: 'session_key_expired', error: 'session_key_expired' }, 401) : json({ ok: true }));
    const res = await client.get('/graph/data');
    expect(res.status).toBe(200);
    expect(apiCalls).toBe(2);
    expect(binds).toHaveLength(2);
    expect(binds[1].body).not.toBe(binds[0].body); // a fresh ephemeral key
  });

  it('does not loop when the retry is refused too', async () => {
    const client = new UnifiedApiClient('/api');
    initializeAuthInterceptor(client);
    await nostrAuth.login();
    apiBehaviour = () => json({ code: 'session_key_expired' }, 401);
    await expect(client.get('/graph/data')).rejects.toMatchObject({ status: 401 });
    expect(apiCalls).toBe(2);
    expect(binds).toHaveLength(2);
  });

  it('does not re-bind on an ordinary 401', async () => {
    const client = new UnifiedApiClient('/api');
    initializeAuthInterceptor(client);
    await nostrAuth.login();
    apiBehaviour = () => json({ error: 'nope' }, 401);
    await expect(client.get('/graph/data')).rejects.toMatchObject({ status: 401 });
    expect(apiCalls).toBe(1);
    expect(binds).toHaveLength(1);
  });

  it('re-binds ahead of expiry, so a socket upgrade never carries a stale key', async () => {
    await nostrAuth.login();
    (nostrAuth as any).sessionKey.expiresAt = Date.now() + 30_000; // inside the margin
    await computeAuthHeaders(`${ORIGIN}/api/x`, 'GET');
    expect(binds).toHaveLength(2);
  });

  it('logout clears the key, and the next sign-in binds a new one', async () => {
    await nostrAuth.login();
    const first = (nostrAuth as any).sessionKey.secret as Uint8Array;
    const firstCopy = new Uint8Array(first);
    expect(firstCopy.some((b) => b !== 0)).toBe(true);
    await nostrAuth.logout();
    expect((nostrAuth as any).sessionKey).toBeNull();
    expect(first.every((b) => b === 0)).toBe(true); // zeroed in place
    await nostrAuth.login();
    expect(binds).toHaveLength(2);
    expect(JSON.parse(binds[1].body).sessionPubkey).not.toBe(JSON.parse(binds[0].body).sessionPubkey);
  });

  it('falls back to ONE real signature per request when the bind is refused, without re-binding', async () => {
    fetchMock.mockImplementation(async (url: string) =>
      url === BIND_URL ? json({ error: 'no' }, 503) : json({ ok: true }));
    await nostrAuth.login(); // the one refused bind attempt (1 real signature)
    const attempts = () => fetchMock.mock.calls.filter((c) => c[0] === BIND_URL).length;
    expect(attempts()).toBe(1);
    signEvent.mockClear();
    const h1 = await computeAuthHeaders(`${ORIGIN}/api/x`, 'GET');
    const h2 = await computeAuthHeaders(`${ORIGIN}/api/y`, 'GET');
    expect(decode(h1.Authorization).pubkey).toBe(realPub);
    expect(decode(h2.Authorization).pubkey).toBe(realPub);
    expect(signEvent).toHaveBeenCalledTimes(2); // not 4: no bind retried per request
    expect(attempts()).toBe(1);
    // A later sign-in is a new session and tries again.
    await nostrAuth.logout();
    await nostrAuth.login();
    expect(attempts()).toBe(2);
  });

  it('never re-binds when the server has no bind endpoint (404/405)', async () => {
    fetchMock.mockImplementation(async (url: string) =>
      url === BIND_URL ? json({ error: 'nf' }, 404) : json({ ok: true }));
    await nostrAuth.login();
    signEvent.mockClear();
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(Date.now() + 3600_000);
    await computeAuthHeaders(`${ORIGIN}/api/x`, 'GET');
    vi.useRealTimers();
    expect(fetchMock.mock.calls.filter((c) => c[0] === BIND_URL).length).toBe(1);
    expect(signEvent).toHaveBeenCalledTimes(1);
  });

  it('does not bind on every request when the granted lifetime is shorter than the margin', async () => {
    fetchMock.mockImplementation(async (url: string, init?: RequestInit) =>
      url === BIND_URL
        ? json({ sessionPubkey: JSON.parse(init!.body as string).sessionPubkey, expiresAt: Math.floor(Date.now() / 1000) + 60 })
        : json({ ok: true }));
    await nostrAuth.login();
    signEvent.mockClear();
    await computeAuthHeaders(`${ORIGIN}/api/x`, 'GET');
    await computeAuthHeaders(`${ORIGIN}/api/y`, 'GET');
    await computeAuthHeaders(`${ORIGIN}/api/z`, 'GET');
    expect(fetchMock.mock.calls.filter((c) => c[0] === BIND_URL).length).toBe(1);
    expect(signEvent).toHaveBeenCalledTimes(0);
  });

  it('counts only real-signer signatures in the sign tally', async () => {
    const { getSignStats } = await import('../signStats');
    await nostrAuth.login();
    getSignStats(true);
    for (let i = 0; i < 5; i++) await computeAuthHeaders(`${ORIGIN}/api/x?${i}`, 'GET');
    expect(getSignStats().total).toBe(0);
    await computeAuthHeaders(`${ORIGIN}/api/auth/nostr/api-keys`, 'POST', '{}');
    expect(getSignStats().total).toBe(1);
  });

  it('leaves the loopback dev-token path alone', async () => {
    vi.stubEnv('DEV', true);
    vi.stubEnv('VITE_DEV_MODE_AUTH', 'true');
    (nostrAuth as any).currentUser = { pubkey: realPub, npub: 'n', isPowerUser: true };
    const h = await computeAuthHeaders(`${ORIGIN}/api/x`, 'GET');
    expect(h.Authorization).toBe(`Bearer ${nostrAuth.getDevSessionToken()}`);
    expect(signEvent).not.toHaveBeenCalled();
    expect(binds).toHaveLength(0);
  });
});
