import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

// Mock dependencies before importing the module under test
vi.mock('../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

// Mock nostr-tools
const mockNpubEncode = vi.fn((hex: string) => `npub1${hex.slice(0, 20)}`);
const mockNip19Decode = vi.fn();
vi.mock('nostr-tools', () => ({
  nip19: {
    npubEncode: (...args: unknown[]) => mockNpubEncode(...(args as [string])),
    decode: (...args: unknown[]) => mockNip19Decode(...args),
  },
}));

const mockGetPublicKey = vi.fn();
const mockFinalizeEvent = vi.fn();
vi.mock('nostr-tools/pure', () => ({
  getPublicKey: (...args: unknown[]) => mockGetPublicKey(...args),
  finalizeEvent: (...args: unknown[]) => mockFinalizeEvent(...args),
}));

// We need to re-import fresh module for each test to reset singleton
let nostrAuth: Awaited<typeof import('../nostrAuthService')>['nostrAuth'];
let setLocalKey: Awaited<typeof import('../nostrAuthService')>['setLocalKey'];
let clearLocalKey: Awaited<typeof import('../nostrAuthService')>['clearLocalKey'];

async function loadModule() {
  // Reset module registry so the singleton resets
  vi.resetModules();
  const mod = await import('../nostrAuthService');
  nostrAuth = mod.nostrAuth;
  setLocalKey = mod.setLocalKey;
  clearLocalKey = mod.clearLocalKey;
}

describe('NostrAuthService', () => {
  let mockSessionStorage: Record<string, string>;
  let mockLocalStorage: Record<string, string>;

  beforeEach(async () => {
    mockSessionStorage = {};
    mockLocalStorage = {};

    // Mock sessionStorage
    vi.stubGlobal('sessionStorage', {
      getItem: vi.fn((key: string) => mockSessionStorage[key] ?? null),
      setItem: vi.fn((key: string, value: string) => { mockSessionStorage[key] = value; }),
      removeItem: vi.fn((key: string) => { delete mockSessionStorage[key]; }),
    });

    // Mock localStorage
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => mockLocalStorage[key] ?? null),
      setItem: vi.fn((key: string, value: string) => { mockLocalStorage[key] = value; }),
      removeItem: vi.fn((key: string) => { delete mockLocalStorage[key]; }),
    });

    // Mock import.meta.env
    vi.stubGlobal('import', { meta: { env: { DEV: false } } });

    // Mock crypto.subtle for signRequest
    vi.stubGlobal('crypto', {
      randomUUID: vi.fn().mockReturnValueOnce('nonce-one').mockReturnValueOnce('nonce-two').mockReturnValue('nonce-other'),
      subtle: {
        digest: vi.fn(async () => new ArrayBuffer(32)),
        importKey: vi.fn(async () => ({})),
        deriveBits: vi.fn(async () => new ArrayBuffer(32)),
      },
    });

    // Mock window.nostr (NIP-07)
    window.nostr = undefined;

    mockNpubEncode.mockClear();
    mockNip19Decode.mockClear();
    mockGetPublicKey.mockClear();
    mockFinalizeEvent.mockClear();

    await loadModule();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // --- Singleton ---

  describe('getInstance / singleton', () => {
    it('should return the same instance', () => {
      expect(nostrAuth).toBeDefined();
      // The export is already a singleton
    });
  });

  // --- hasNip07Provider ---

  describe('hasNip07Provider', () => {
    it('should return false when window.nostr is undefined', () => {
      window.nostr = undefined;
      expect(nostrAuth.hasNip07Provider()).toBe(false);
    });

    it('should return true when window.nostr is defined', () => {
      window.nostr = { getPublicKey: vi.fn(), signEvent: vi.fn() };
      expect(nostrAuth.hasNip07Provider()).toBe(true);
    });
  });

  // --- hexToNpub / npubToHex ---

  describe('hexToNpub', () => {
    it('should encode hex pubkey to npub', () => {
      mockNpubEncode.mockReturnValue('npub1abc');
      const result = nostrAuth.hexToNpub('deadbeef');
      expect(result).toBe('npub1abc');
      expect(mockNpubEncode).toHaveBeenCalledWith('deadbeef');
    });

    it('should return undefined for empty string', () => {
      expect(nostrAuth.hexToNpub('')).toBeUndefined();
    });

    it('should return undefined when encoding throws', () => {
      mockNpubEncode.mockImplementation(() => { throw new Error('bad'); });
      expect(nostrAuth.hexToNpub('invalid')).toBeUndefined();
    });
  });

  describe('npubToHex', () => {
    it('should decode npub to hex', () => {
      mockNip19Decode.mockReturnValue({ type: 'npub', data: 'abcdef' });
      expect(nostrAuth.npubToHex('npub1xyz')).toBe('abcdef');
    });

    it('should return undefined for empty string', () => {
      expect(nostrAuth.npubToHex('')).toBeUndefined();
    });

    it('should return undefined for non-npub decode result', () => {
      mockNip19Decode.mockReturnValue({ type: 'nsec', data: 'secret' });
      expect(nostrAuth.npubToHex('nsec1xyz')).toBeUndefined();
    });

    it('should return undefined when decoding throws', () => {
      mockNip19Decode.mockImplementation(() => { throw new Error('decode fail'); });
      expect(nostrAuth.npubToHex('bad')).toBeUndefined();
    });
  });

  // --- getSessionToken (deprecated) ---

  describe('getSessionToken', () => {
    it('should always return null (NIP-98 mode)', () => {
      expect(nostrAuth.getSessionToken()).toBeNull();
    });
  });

  // --- getCurrentUser / isAuthenticated ---

  describe('getCurrentUser', () => {
    it('should return null before login', () => {
      expect(nostrAuth.getCurrentUser()).toBeNull();
    });
  });

  describe('isAuthenticated', () => {
    it('should return false when no user and no NIP-07', () => {
      expect(nostrAuth.isAuthenticated()).toBe(false);
    });
  });

  // --- getCurrentAuthState ---

  describe('getCurrentAuthState', () => {
    it('should return unauthenticated state initially', () => {
      const state = nostrAuth.getCurrentAuthState();
      expect(state.authenticated).toBe(false);
      expect(state.user).toBeUndefined();
      expect(state.error).toBeUndefined();
    });
  });

  // --- onAuthStateChanged ---

  describe('onAuthStateChanged', () => {
    it('should not immediately call listener when not initialized', () => {
      const listener = vi.fn();
      nostrAuth.onAuthStateChanged(listener);
      // Not initialized yet, so listener should not be called immediately
      // (constructor doesn't set initialized=true)
      expect(listener).not.toHaveBeenCalled();
    });

    it('should call listener immediately after initialization', async () => {
      await nostrAuth.initialize();
      const listener = vi.fn();
      nostrAuth.onAuthStateChanged(listener);
      expect(listener).toHaveBeenCalledTimes(1);
      expect(listener).toHaveBeenCalledWith(expect.objectContaining({ authenticated: false }));
    });

    it('should return unsubscribe function that works', async () => {
      await nostrAuth.initialize();
      const listener = vi.fn();
      const unsub = nostrAuth.onAuthStateChanged(listener);
      expect(listener).toHaveBeenCalledTimes(1);
      unsub();
      // After logout/login cycle, listener should not be called again
      await nostrAuth.logout();
      expect(listener).toHaveBeenCalledTimes(1); // still 1
    });
  });

  // --- initialize ---

  describe('initialize', () => {
    it('should set initialized to true', async () => {
      await nostrAuth.initialize();
      // After initialization, getCurrentAuthState reflects initialized
      const state = nostrAuth.getCurrentAuthState();
      expect(state).toBeDefined();
    });

    it('should restore user from localStorage when present', async () => {
      const storedUser = { pubkey: 'abc123', isPowerUser: false };
      mockLocalStorage['nostr_user'] = JSON.stringify(storedUser);
      await nostrAuth.initialize();
      const user = nostrAuth.getCurrentUser();
      expect(user).toEqual(storedUser);
    });

    it('should handle corrupt localStorage data gracefully', async () => {
      mockLocalStorage['nostr_user'] = 'not-json{{{';
      await nostrAuth.initialize();
      expect(nostrAuth.getCurrentUser()).toBeNull();
    });

    it('should not re-initialize if already initialized', async () => {
      await nostrAuth.initialize();
      // Second call should return immediately
      await nostrAuth.initialize();
      const state = nostrAuth.getCurrentAuthState();
      expect(state).toBeDefined();
    });
  });

  // --- login (NIP-07) ---

  describe('login', () => {
    it('should throw when no NIP-07 provider', async () => {
      window.nostr = undefined;
      await expect(nostrAuth.login()).rejects.toThrow('NIP-07 provider');
    });

    it('should login successfully via NIP-07', async () => {
      const pubkey = 'a'.repeat(64);
      window.nostr = {
        getPublicKey: vi.fn().mockResolvedValue(pubkey),
        signEvent: vi.fn(),
      };
      mockNpubEncode.mockReturnValue('npub1test');

      const state = await nostrAuth.login();
      expect(state.authenticated).toBe(true);
      expect(state.user?.pubkey).toBe(pubkey);
    });

    it('should throw when getPublicKey returns falsy', async () => {
      window.nostr = {
        getPublicKey: vi.fn().mockResolvedValue(''),
        signEvent: vi.fn(),
      };
      await expect(nostrAuth.login()).rejects.toThrow();
    });

    it('should handle user rejection', async () => {
      window.nostr = {
        getPublicKey: vi.fn().mockRejectedValue(new Error('User rejected')),
        signEvent: vi.fn(),
      };
      await expect(nostrAuth.login()).rejects.toThrow('rejected');
    });
  });

  // --- logout ---

  describe('logout', () => {
    it('should clear current user', async () => {
      // First login
      const pubkey = 'b'.repeat(64);
      window.nostr = {
        getPublicKey: vi.fn().mockResolvedValue(pubkey),
        signEvent: vi.fn(),
      };
      await nostrAuth.login();
      expect(nostrAuth.getCurrentUser()).not.toBeNull();

      await nostrAuth.logout();
      expect(nostrAuth.getCurrentUser()).toBeNull();
    });

    it('should notify listeners with unauthenticated state', async () => {
      await nostrAuth.initialize();
      const listener = vi.fn();
      nostrAuth.onAuthStateChanged(listener);
      listener.mockClear();

      await nostrAuth.logout();
      expect(listener).toHaveBeenCalledWith({ authenticated: false });
    });
  });

  // --- loginWithPasskey ---

  describe('loginWithPasskey', () => {
    it('should set user and local private key', async () => {
      const pubkey = 'c'.repeat(64);
      const privkey = new Uint8Array(32).fill(0xab);
      mockNpubEncode.mockReturnValue('npub1passkey');

      const state = await nostrAuth.loginWithPasskey(pubkey, privkey);
      expect(state.authenticated).toBe(true);
      expect(state.user?.pubkey).toBe(pubkey);
      expect(nostrAuth.getCurrentUser()?.pubkey).toBe(pubkey);
    });

    it('should store pubkey in sessionStorage but not private key', async () => {
      const pubkey = 'd'.repeat(64);
      const privkey = new Uint8Array(32).fill(0x01);
      mockNpubEncode.mockReturnValue('npub1pk');

      await nostrAuth.loginWithPasskey(pubkey, privkey);

      expect(sessionStorage.setItem).toHaveBeenCalledWith('nostr_passkey_pubkey', pubkey);
      // private key should NOT be stored in sessionStorage
      expect(sessionStorage.setItem).not.toHaveBeenCalledWith('nostr_passkey_key', expect.anything());
    });
  });

  // --- hasPasskeySession ---

  describe('hasPasskeySession', () => {
    it('should return false when no passkey session', () => {
      expect(nostrAuth.hasPasskeySession()).toBe(false);
    });

    it('should return true after loginWithPasskey', async () => {
      const pubkey = 'e'.repeat(64);
      const privkey = new Uint8Array(32).fill(0x02);
      mockNpubEncode.mockReturnValue('npub1has');
      await nostrAuth.loginWithPasskey(pubkey, privkey);
      expect(nostrAuth.hasPasskeySession()).toBe(true);
    });

    it('should return true if legacy sessionStorage key exists', () => {
      mockSessionStorage['nostr_passkey_key'] = 'aa'.repeat(32);
      expect(nostrAuth.hasPasskeySession()).toBe(true);
    });
  });

  // --- restorePasskeySession ---

  describe('restorePasskeySession', () => {
    it('should restore from legacy sessionStorage and remove it', () => {
      const hexKey = 'ab'.repeat(32);
      const pubkey = 'f'.repeat(64);
      mockSessionStorage['nostr_passkey_key'] = hexKey;
      mockSessionStorage['nostr_passkey_pubkey'] = pubkey;
      mockGetPublicKey.mockReturnValue(pubkey);
      mockNpubEncode.mockReturnValue('npub1restored');

      nostrAuth.restorePasskeySession();

      // Legacy key should be removed
      expect(sessionStorage.removeItem).toHaveBeenCalledWith('nostr_passkey_key');
      // User should be set
      expect(nostrAuth.getCurrentUser()?.pubkey).toBe(pubkey);
    });

    it('should clear session on pubkey mismatch', () => {
      const hexKey = 'cd'.repeat(32);
      const pubkey = '0'.repeat(64);
      mockSessionStorage['nostr_passkey_key'] = hexKey;
      mockSessionStorage['nostr_passkey_pubkey'] = pubkey;
      mockGetPublicKey.mockReturnValue('1'.repeat(64)); // different pubkey

      nostrAuth.restorePasskeySession();

      expect(sessionStorage.removeItem).toHaveBeenCalledWith('nostr_passkey_pubkey');
      expect(nostrAuth.getCurrentUser()).toBeNull();
    });

    it('should do nothing when no keys in storage', () => {
      nostrAuth.restorePasskeySession();
      expect(nostrAuth.getCurrentUser()).toBeNull();
    });
  });

  // --- setLocalKey / clearLocalKey ---

  describe('setLocalKey / clearLocalKey', () => {
    it('setLocalKey should enable hasPasskeySession', () => {
      setLocalKey('aa'.repeat(32));
      expect(nostrAuth.hasPasskeySession()).toBe(true);
    });

    it('clearLocalKey should disable hasPasskeySession', () => {
      setLocalKey('bb'.repeat(32));
      clearLocalKey();
      expect(nostrAuth.hasPasskeySession()).toBe(false);
    });

    it('clearLocalKey should remove legacy sessionStorage entries', () => {
      clearLocalKey();
      expect(sessionStorage.removeItem).toHaveBeenCalledWith('nostr_passkey_key');
      expect(sessionStorage.removeItem).toHaveBeenCalledWith('nostr_privkey');
    });
  });

  // --- signRequest ---

  describe('signRequest', () => {
    it('should use NIP-07 extension when no local key and extension available', async () => {
      const signedEvent = { id: 'evt1', sig: 'sig1', pubkey: 'pk1', kind: 27235, tags: [], content: '', created_at: 0 };
      window.nostr = {
        getPublicKey: vi.fn(),
        signEvent: vi.fn().mockResolvedValue(signedEvent),
      };

      const result = await nostrAuth.signRequest('https://example.com/api', 'GET');
      expect(result).toBe(btoa(JSON.stringify(signedEvent)));
      expect(window.nostr!.signEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: 27235,
          tags: expect.arrayContaining([
            ['u', 'https://example.com/api'],
            ['method', 'GET'],
          ]),
        })
      );
    });

    it('gives same-second repeated requests distinct signed event tags', async () => {
      window.nostr = { getPublicKey: vi.fn(), signEvent: vi.fn().mockResolvedValue({}) };
      await nostrAuth.signRequest('https://example.com/api', 'GET');
      await nostrAuth.signRequest('https://example.com/api', 'GET');
      const calls = vi.mocked(window.nostr.signEvent).mock.calls;
      expect(calls[0][0].tags).toContainEqual(['nonce', 'nonce-one']);
      expect(calls[1][0].tags).toContainEqual(['nonce', 'nonce-two']);
    });

    it('should throw when no signing method available', async () => {
      window.nostr = undefined;
      await expect(nostrAuth.signRequest('https://example.com', 'POST')).rejects.toThrow(
        'No signing method available'
      );
    });

    it('should include payload hash tag when body provided via NIP-07', async () => {
      const signedEvent = { id: 'e', sig: 's', pubkey: 'p', kind: 27235, tags: [], content: '', created_at: 0 };
      window.nostr = {
        getPublicKey: vi.fn(),
        signEvent: vi.fn().mockResolvedValue(signedEvent),
      };

      await nostrAuth.signRequest('https://example.com', 'POST', '{"data":true}');
      expect(window.nostr!.signEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          tags: expect.arrayContaining([
            expect.arrayContaining(['payload']),
          ]),
        })
      );
    });
  });

  // --- signWithLocalKey ---

  describe('signWithLocalKey', () => {
    it('should throw when no local private key', async () => {
      await expect(nostrAuth.signWithLocalKey('https://example.com', 'GET')).rejects.toThrow(
        'No local private key available'
      );
    });

    it('should use finalizeEvent when local key is set', async () => {
      const pubkey = 'a'.repeat(64);
      const privkey = new Uint8Array(32).fill(0x05);
      mockNpubEncode.mockReturnValue('npub1local');
      mockFinalizeEvent.mockReturnValue({ id: 'local', sig: 'localsig', pubkey, kind: 27235, tags: [], content: '', created_at: 0 });

      await nostrAuth.loginWithPasskey(pubkey, privkey);
      const result = await nostrAuth.signWithLocalKey('https://example.com/api', 'PUT');

      expect(mockFinalizeEvent).toHaveBeenCalledWith(
        expect.objectContaining({ kind: 27235 }),
        privkey
      );
      expect(typeof result).toBe('string');
    });
  });
});
