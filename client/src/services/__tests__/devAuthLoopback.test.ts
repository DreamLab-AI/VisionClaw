import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

const FORUM_PUBKEY = 'b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a';
const DEV_TOKEN = ['dev', 'session', 'token'].join('-');

import { nostrAuth } from '../nostrAuthService';
import { computeAuthHeaders } from '../api/authInterceptor';

function setHost(hostname: string) {
  vi.stubGlobal('location', {
    hostname,
    origin: `http://${hostname}:3001`,
    href: `http://${hostname}:3001/`,
    protocol: 'http:',
    host: `${hostname}:3001`,
  });
}

function seedUser() {
  (nostrAuth as any).currentUser = { pubkey: FORUM_PUBKEY, npub: 'npub-test', isPowerUser: true };
  (nostrAuth as any).localPrivateKey = null;
}

describe('dev-token mode is loopback-only', () => {
  beforeEach(() => {
    vi.stubEnv('DEV', true);
    vi.stubEnv('VITE_DEV_MODE_AUTH', 'true');
    (window as any).nostr = {
      getPublicKey: vi.fn().mockResolvedValue(FORUM_PUBKEY),
      signEvent: vi.fn(async (e: any) => ({ ...e, pubkey: FORUM_PUBKEY, id: 'id', sig: 'sig' })),
    };
    seedUser();
  });
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    delete (window as any).nostr;
    (nostrAuth as any).currentUser = null;
    (nostrAuth as any).initialized = false;
  });

  it.each(['192.168.2.132', 'example.com', '10.0.0.5'])('isDevMode() is false on %s', (host) => {
    setHost(host);
    expect(nostrAuth.isDevMode()).toBe(false);
    expect(nostrAuth.getCurrentAuthState().authenticated).toBe(true); // via NIP-07, not dev bypass
  });

  it.each(['localhost', '127.0.0.1', '[::1]', '::1'])('isDevMode() is true on %s', (host) => {
    setHost(host);
    expect(nostrAuth.isDevMode()).toBe(true);
  });

  it('off loopback, computeAuthHeaders signs NIP-98 via window.nostr and never sends the dev token', async () => {
    setHost('192.168.2.132');
    const url = 'http://192.168.2.132:3001/api/settings';
    const headers = await computeAuthHeaders(url, 'PUT', '{"a":1}');
    expect(JSON.stringify(headers)).not.toContain(DEV_TOKEN);
    expect(headers.Authorization).toMatch(/^Nostr /);
    const ev = JSON.parse(atob(headers.Authorization.slice('Nostr '.length)));
    expect(ev.kind).toBe(27235);
    expect(ev.pubkey).toBe(FORUM_PUBKEY);
    expect(ev.tags).toContainEqual(['u', url]);
    expect(ev.tags).toContainEqual(['method', 'PUT']);
  });

  it('off loopback, initialize() does not auto-login', async () => {
    setHost('192.168.2.132');
    (nostrAuth as any).currentUser = null;
    (nostrAuth as any).initialized = false;
    sessionStorage.clear();
    localStorage.clear();
    await nostrAuth.initialize();
    expect(nostrAuth.getCurrentUser()).toBeNull();
    expect(sessionStorage.getItem('ephemeral_session_pubkey')).toBeNull();
  });

  it('off loopback, manual dev login is unavailable and rejected', async () => {
    setHost('192.168.2.132');
    expect(nostrAuth.isDevLoginAvailable()).toBe(false);
    await expect(nostrAuth.devLogin()).rejects.toThrow();
  });

  it('on loopback, dev-token behaviour is unchanged', async () => {
    setHost('localhost');
    const headers = await computeAuthHeaders('http://localhost:3001/api/x', 'GET');
    expect(headers.Authorization).toBe(`Bearer ${DEV_TOKEN}`);
    expect(headers['X-Nostr-Pubkey']).toBe(FORUM_PUBKEY);
  });
});

describe('dev token literal', () => {
  it('appears in exactly one non-test source file (the helper)', () => {
    const sources = import.meta.glob(
      ['../../**/*.{ts,tsx}', '!../../**/__tests__/**', '!../../**/*.{test,spec}.{ts,tsx}'],
      { query: '?raw', import: 'default', eager: true },
    ) as Record<string, string>;
    const hits = Object.entries(sources)
      .filter(([, src]) => src.includes(DEV_TOKEN))
      .map(([path]) => path);
    expect(hits.length).toBe(1);
    expect(hits[0]).toMatch(/nostrAuthService\.ts$/);
  });
});
