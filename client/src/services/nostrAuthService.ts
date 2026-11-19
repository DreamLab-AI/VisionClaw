import { createLogger } from '../utils/loggerConfig';
import { recordSignature } from './signStats';
import { createErrorMetadata } from '../utils/loggerConfig';
import { nip19 } from 'nostr-tools';
import { getPublicKey, finalizeEvent, generateSecretKey } from 'nostr-tools/pure';
import type {} from '../types/nip07';

const logger = createLogger('NostrAuthService');

// --- Module-scoped key storage ---
// Private key held in memory only, never persisted to sessionStorage.
// This reduces the attack surface vs sessionStorage (which is queryable by
// any same-origin JS).  nostr-tools still needs raw bytes for signing, so
// this is the best we can do without a native secp256k1 WebCrypto curve.
let _localKeyHex: string | null = null;

/**
 * Store a hex-encoded Nostr private key in the module-scoped closure.
 * Call this instead of writing the key to sessionStorage.
 */
export function setLocalKey(hexKey: string): void {
  _localKeyHex = hexKey;
}

/**
 * Wipe the module-scoped private key and remove any legacy sessionStorage
 * entries that may still exist from older code paths.
 */
export function clearLocalKey(): void {
  if (_localKeyHex) {
    _localKeyHex = '';
  }
  _localKeyHex = null;
  try {
    sessionStorage.removeItem('nostr_passkey_key');
    sessionStorage.removeItem('nostr_privkey');
  } catch { /* sessionStorage may be unavailable */ }
}

// Clear key material when the tab / window is closed
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    if (_localKeyHex) {
      _localKeyHex = '';
    }
    _localKeyHex = null;
  });
}

// --- NIP-07 Extension Detection ---
// Adapted from nip07-awaiter (https://github.com/penpenpng/nip07-awaiter)
// Uses dual-strategy: Object.defineProperty setter hook (instant) + heuristic
// polling (fallback), racing against a caller-supplied timeout.

/** Type guard: does the value look like a NIP-07 provider? */
function isNip07Provider(value: unknown): value is NonNullable<typeof window.nostr> {
  if (!value || typeof value !== 'object') return false;
  const obj = value as Record<string, unknown>;
  return typeof obj.getPublicKey === 'function' && typeof obj.signEvent === 'function';
}

/**
 * Wait for a NIP-07 extension to set window.nostr.
 * Resolves with true if detected within timeoutMs, false otherwise.
 *
 * Strategy 1 (instant): Object.defineProperty setter hook intercepts the
 *   assignment the moment the extension writes window.nostr.
 * Strategy 2 (fallback): Heuristic polling - 10ms for first 1s, 100ms for
 *   1-5s, 1s thereafter.
 */
function waitForNip07(timeoutMs: number): Promise<boolean> {
  if (isNip07Provider(window.nostr)) return Promise.resolve(true);

  const controller = new AbortController();
  const cleanup = () => controller.abort();
  const promises: Promise<boolean>[] = [];

  // Timeout: resolves false after timeoutMs
  promises.push(
    new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => resolve(false), timeoutMs);
      controller.signal.addEventListener('abort', () => clearTimeout(timer));
    })
  );

  // Heuristic polling (always works)
  promises.push(
    new Promise<boolean>((resolve) => {
      let elapsed = 0;
      let timer: ReturnType<typeof setTimeout> | undefined;

      const check = () => {
        if (isNip07Provider(window.nostr)) {
          resolve(true);
          return;
        }
        let interval: number;
        if (elapsed < 1000) interval = 10;
        else if (elapsed < 5000) interval = 100;
        else interval = 1000;
        elapsed += interval;
        timer = setTimeout(check, interval);
      };

      check();
      controller.signal.addEventListener('abort', () => {
        if (timer !== undefined) clearTimeout(timer);
      });
    })
  );

  // Object.defineProperty setter hook (instant detection, not always installable)
  const descriptor = Object.getOwnPropertyDescriptor(window, 'nostr');
  if (!descriptor || descriptor.configurable) {
    promises.push(
      new Promise<boolean>((resolve) => {
        let current = window.nostr;
        Object.defineProperty(window, 'nostr', {
          configurable: true,
          get: () => current,
          set: (value) => {
            current = value;
            if (isNip07Provider(value)) resolve(true);
          },
        });
        // Restore normal property on teardown
        controller.signal.addEventListener('abort', () => {
          try {
            Object.defineProperty(window, 'nostr', {
              configurable: true,
              writable: true,
              enumerable: true,
              value: current,
            });
          } catch { /* best effort */ }
        });
      })
    );
  }

  return Promise.race(promises).then((result) => {
    cleanup();
    return result;
  });
}

// --- Interfaces ---

// User info stored locally and used in AuthState
export interface SimpleNostrUser {
  pubkey: string;
  npub?: string;
  isPowerUser: boolean;
}

// User info returned by backend (kept for backward compat; remove in Phase 2)
export interface BackendNostrUser {
  pubkey: string;
  npub?: string;
  isPowerUser: boolean;
}

// Legacy interfaces (kept for backward compat; remove in Phase 2)
export interface AuthResponse {
  user: BackendNostrUser;
  token: string;
  expiresAt: number;
  features?: string[];
}

export interface VerifyResponse {
  valid: boolean;
  user?: BackendNostrUser;
  features?: string[];
}

export interface AuthEventPayload {
  id: string;
  pubkey: string;
  content: string;
  sig: string;
  created_at: number;
  kind: number;
  tags: string[][];
}

// State exposed to the application
export interface AuthState {
  authenticated: boolean;
  user?: SimpleNostrUser;
  error?: string;
}

type AuthStateListener = (state: AuthState) => void;

// --- Service Implementation ---

/** Kind of the proof-of-possession event a session key signs over its own bind. */
const SESSION_KEY_PROOF_KIND = 27236;
/** Lifetime requested for a bound session key; the server may grant less. */
const SESSION_KEY_REQUESTED_TTL_SECS = 3600;
/** Re-bind this long before expiry: wider than the server's 60 s NIP-98 freshness window. */
const SESSION_KEY_MARGIN_MS = 90_000;
/** After a refused bind, wait this long before trying again (per-request signing meanwhile). */
const SESSION_KEY_RETRY_COOLDOWN_MS = 5 * 60_000;

/**
 * Requests the server refuses a session key for (ADR-2205 deny list): binding
 * another key, API-key management and RBAC administration (reads of
 * `/api/admin/rbac/*` excepted), plus the Solid proxy (`/solid`, `/api/solid`;
 * it forwards the token upstream). Mirrors `session_key_denied` in
 * `src/utils/auth.rs`; these always use the real signer.
 */
export function requiresRealSigner(url: string, method: string): boolean {
  let path: string;
  try {
    path = new URL(url, 'http://placeholder.invalid').pathname;
  } catch {
    return true; // unparseable: take the safe, per-request path
  }
  const segs = path.split('/').filter(Boolean);
  const starts = (prefix: string[]) => prefix.every((p, i) => segs[i] === p);
  // Revoking is signed by the session key itself; binding needs the real signer.
  if (starts(['api', 'auth', 'nostr', 'session-key'])) return method.toUpperCase() !== 'DELETE';
  if (starts(['api', 'auth', 'nostr', 'api-keys'])) return true;
  if (starts(['api', 'admin', 'rbac']) && method.toUpperCase() !== 'GET') return true;
  // The Solid proxy forwards the token upstream, where it is verified as the
  // signer: a session key would act as an ephemeral stranger there.
  if (starts(['solid']) || starts(['api', 'solid'])) return true;
  // Payments are verified against the account's own signer.
  if (starts(['api', 'pay'])) return true;
  return false;
}

/** The one place the dev bearer literal lives (see `NostrAuthService.isDevMode`). */
const DEV_SESSION_TOKEN = 'dev-session-token';

/** True for the loopback hostnames the server treats as loopback. */
function isLoopbackHostname(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '[::1]' || hostname === '::1';
}

class NostrAuthService {
  private static instance: NostrAuthService;
  private currentUser: SimpleNostrUser | null = null;
  private localPrivateKey: Uint8Array | null = null;
  /**
   * ADR-2205 bound session key: an ephemeral secp256k1 key held in memory only
   * (never storage), bound server-side to the signed-in principal by one real
   * signature. Every later request is signed with it, so a NIP-07 extension is
   * asked once per session instead of once per request.
   */
  private sessionKey: {
    secret: Uint8Array;
    pubkey: string;
    principal: string;
    expiresAt: number;
    /** Re-bind this long before `expiresAt` (capped at a quarter of a short lifetime). */
    marginMs: number;
  } | null = null;
  private bindInFlight: Promise<boolean> | null = null;
  /**
   * After a refused bind, requests sign per request (one real signature each)
   * and do not retry the bind until this time: otherwise every request would
   * cost a bind signature as well. `Infinity` = the server has no bind endpoint.
   */
  private bindSuspendedUntil = 0;
  private authStateListeners: AuthStateListener[] = [];
  private initialized = false;

  private constructor() {}

  public static getInstance(): NostrAuthService {
    if (!NostrAuthService.instance) {
      NostrAuthService.instance = new NostrAuthService();
    }
    return NostrAuthService.instance;
  }

  public hasNip07Provider(): boolean {
    return typeof window !== 'undefined' && isNip07Provider(window.nostr);
  }

  /**
   * Wait for a NIP-07 extension to become available.
   * Returns true if detected within timeoutMs, false otherwise.
   * UI components can use this to reactively show the extension button.
   *
   * SINGLETON: Only one waitForNip07 instance ever runs. Multiple callers
   * (initialize() + useNostrAuth hook) share the same promise to prevent
   * competing Object.defineProperty hooks from destroying window.nostr.
   */
  private _nip07Promise: Promise<boolean> | null = null;
  public waitForNip07Provider(timeoutMs = 5000): Promise<boolean> {
    if (!this._nip07Promise) {
      this._nip07Promise = waitForNip07(timeoutMs);
    }
    return this._nip07Promise;
  }

  /**
   * Dev-token mode: a dev build with VITE_DEV_MODE_AUTH=true AND a loopback
   * page origin. The server honours the dev bearer only from true loopback, so
   * off loopback (e.g. the LAN) a dev build behaves like production: no
   * auto-login, NIP-07/passkey sign-in and NIP-98 request signing.
   * This is the single decision point; every dev-token path goes through it.
   */
  public isDevMode(): boolean {
    return (
      import.meta.env.DEV &&
      import.meta.env.VITE_DEV_MODE_AUTH === 'true' &&
      isLoopbackHostname(window.location.hostname)
    );
  }

  /** The dev bearer token value; only ever sent when `isDevMode()` is true. */
  public getDevSessionToken(): string {
    return DEV_SESSION_TOKEN;
  }

  /**
   * Sign an HTTP request using NIP-98 (kind 27235).
   * Returns base64-encoded signed event for the Authorization header.
   * Prefers local passkey-derived key, falls back to NIP-07 extension.
   */
  public async signRequest(url: string, method: string, body?: string): Promise<string> {
    // Prefer local key (passkey-derived) over NIP-07 extension
    if (this.localPrivateKey) {
      recordSignature(method, url);
      return this.signWithLocalKey(url, method, body);
    }

    if (!this.hasNip07Provider()) {
      throw new Error('No signing method available (no passkey session or NIP-07 provider)');
    }

    // ADR-2205: requests the server refuses a session key stay on the real
    // signer; everything else rides the bound session key.
    if (!requiresRealSigner(url, method) && !this.isDevMode()) {
      if (await this.ensureSessionKey()) {
        return this.signWithSessionKey(url, method, body);
      }
      // Bind refused or unavailable: degrade to a per-request signature.
    }

    return this.signWithNip07(url, method, body);
  }

  /** Whether a live, unexpired session key is held (ADR-2205). */
  public hasSessionKey(): boolean {
    return this.sessionKey !== null && this.sessionKey.expiresAt - this.sessionKey.marginMs > Date.now();
  }

  /**
   * Bind a session key if none is live. Concurrent callers share one bind.
   * Resolves `true` when a live key is held afterwards.
   */
  public ensureSessionKey(): Promise<boolean> {
    if (this.hasSessionKey()) return Promise.resolve(true);
    return this.bindSessionKey();
  }

  /**
   * Discard the held key and bind a fresh one (the one re-bind after a
   * `session_key_expired` 401). Concurrent callers share one bind.
   */
  public rebindSessionKey(): Promise<boolean> {
    if (this.bindInFlight) return this.bindInFlight;
    this.discardSessionKey();
    return this.bindSessionKey();
  }

  private bindSessionKey(): Promise<boolean> {
    if (this.bindInFlight) return this.bindInFlight;
    const principal = this.currentUser?.pubkey;
    if (!principal || this.localPrivateKey || !this.hasNip07Provider() || this.isDevMode()) {
      return Promise.resolve(false);
    }
    if (Date.now() < this.bindSuspendedUntil) return Promise.resolve(false);
    this.bindInFlight = (async () => {
      let secret: Uint8Array | null = null;
      try {
        secret = generateSecretKey();
        const sessionPubkey = getPublicKey(secret);
        const url = new URL('/api/auth/nostr/session-key', window.location.origin).href;
        // Proof of possession: the session key signs a statement binding itself
        // to this principal, URL and lifetime, so the server never binds a key
        // the browser does not hold (e.g. someone else's long-term key).
        const proof = finalizeEvent(
          {
            created_at: Math.floor(Date.now() / 1000),
            kind: SESSION_KEY_PROOF_KIND,
            tags: [
              ['principal', principal],
              ['u', url],
              ['expires', String(SESSION_KEY_REQUESTED_TTL_SECS)],
              ['nonce', crypto.randomUUID()],
            ],
            content: '',
          },
          secret,
        );
        const body = JSON.stringify({
          sessionPubkey,
          expiresIn: SESSION_KEY_REQUESTED_TTL_SECS,
          proof: JSON.stringify(proof),
        });
        // Raw fetch with the real signer: the bind must not recurse into
        // signRequest's session branch or the UnifiedApiClient interceptors.
        const token = await this.signWithNip07(url, 'POST', body);
        const response = await fetch(url, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', Authorization: `Nostr ${token}` },
          body,
        });
        if (!response.ok) {
          logger.warn(`Session key bind refused (HTTP ${response.status}); signing per request`);
          secret.fill(0);
          // "Not admitted" and "no endpoint" do not change within a session.
          const code = (await response.json().catch(() => ({} as { code?: string }))).code;
          this.bindSuspendedUntil =
            response.status === 404 || response.status === 405 || code === 'session_key_not_admitted'
              ? Infinity
              : Date.now() + SESSION_KEY_RETRY_COOLDOWN_MS;
          return false;
        }
        const granted = await response.json().catch(() => ({} as { expiresAt?: number }));
        const expiresAtMs = typeof granted.expiresAt === 'number' ? granted.expiresAt * 1000 : 0;
        if (!(expiresAtMs > Date.now())) {
          secret.fill(0);
          this.bindSuspendedUntil = Date.now() + SESSION_KEY_RETRY_COOLDOWN_MS;
          return false;
        }
        this.discardSessionKey();
        const marginMs = Math.min(SESSION_KEY_MARGIN_MS, (expiresAtMs - Date.now()) / 4);
        this.sessionKey = { secret, pubkey: sessionPubkey, principal, expiresAt: expiresAtMs, marginMs };
        this.bindSuspendedUntil = 0;
        return true;
      } catch (error) {
        secret?.fill(0);
        this.bindSuspendedUntil = Date.now() + SESSION_KEY_RETRY_COOLDOWN_MS;
        logger.warn('Session key bind failed; signing per request', createErrorMetadata(error));
        return false;
      }
    })().finally(() => {
      this.bindInFlight = null;
    });
    return this.bindInFlight;
  }

  /** Ask the server to drop the held key's binding, signed by that key (no extension prompt). */
  private async revokeSessionKey(): Promise<void> {
    if (!this.sessionKey || this.sessionKey.expiresAt <= Date.now()) return;
    try {
      const url = new URL('/api/auth/nostr/session-key', window.location.origin).href;
      const token = await this.signWithSessionKey(url, 'DELETE');
      await fetch(url, { method: 'DELETE', headers: { Authorization: `Nostr ${token}` } });
    } catch (error) {
      logger.warn('Session key revoke failed; it will expire on its own', createErrorMetadata(error));
    }
  }

  private discardSessionKey(): void {
    if (this.sessionKey) {
      this.sessionKey.secret.fill(0);
    }
    this.sessionKey = null;
  }

  /** Build the NIP-98 tags shared by every signer. */
  private async nip98Tags(url: string, method: string, body?: string): Promise<string[][]> {
    const tags: string[][] = [
      ['u', url],
      ['method', method.toUpperCase()],
      ['nonce', crypto.randomUUID()],
    ];

    if (body) {
      const encoder = new TextEncoder();
      const data = encoder.encode(body);
      const hashBuffer = await crypto.subtle.digest('SHA-256', data);
      const hashArray = Array.from(new Uint8Array(hashBuffer));
      const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
      tags.push(['payload', hashHex]);
    }
    return tags;
  }

  /** Sign with the bound session key; the `session` tag names the principal. */
  private async signWithSessionKey(url: string, method: string, body?: string): Promise<string> {
    const key = this.sessionKey;
    if (!key) throw new Error('No session key available');
    const tags = await this.nip98Tags(url, method, body);
    tags.push(['session', key.principal]);
    const signedEvent = finalizeEvent(
      { created_at: Math.floor(Date.now() / 1000), kind: 27235, tags, content: '' },
      key.secret,
    );
    return btoa(JSON.stringify(signedEvent));
  }

  /** Sign with the user's real NIP-07 signer (one extension round trip). */
  private async signWithNip07(url: string, method: string, body?: string): Promise<string> {
    // The sign tally measures signer prompts, so session-key signatures are not counted.
    recordSignature(method, url);
    const tags = await this.nip98Tags(url, method, body);
    const unsignedEvent = {
      created_at: Math.floor(Date.now() / 1000),
      kind: 27235,
      tags,
      content: '',
    };

    const signedEvent = await window.nostr!.signEvent(unsignedEvent);
    const eventJson = JSON.stringify(signedEvent);
    return btoa(eventJson);
  }

  /**
   * Sign a NIP-98 request using the local passkey-derived private key.
   */
  public async signWithLocalKey(url: string, method: string, body?: string): Promise<string> {
    if (!this.localPrivateKey) {
      throw new Error('No local private key available');
    }

    const tags: string[][] = [
      ['u', url],
      ['method', method.toUpperCase()],
      ['nonce', crypto.randomUUID()],
    ];

    if (body) {
      const encoder = new TextEncoder();
      const data = encoder.encode(body);
      const hashBuffer = await crypto.subtle.digest('SHA-256', data);
      const hashArray = Array.from(new Uint8Array(hashBuffer));
      const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
      tags.push(['payload', hashHex]);
    }

    const eventTemplate = {
      created_at: Math.floor(Date.now() / 1000),
      kind: 27235,
      tags,
      content: '',
    };

    const signedEvent = finalizeEvent(eventTemplate, this.localPrivateKey);
    const eventJson = JSON.stringify(signedEvent);
    return btoa(eventJson);
  }

  public async initialize(): Promise<void> {
    if (this.initialized) return;
    logger.debug('Initializing NostrAuthService...');

    // DEV MODE: Auto-login as power user with ephemeral per-tab session identity
    if (this.isDevMode()) {
      logger.info('[DEV MODE] Auto-authenticating as power user');
      // Generate a unique ephemeral pubkey per browser tab so multiple tabs
      // get isolated session identities (different physics settings, filters, etc.)
      let devPowerUserPubkey = sessionStorage.getItem('ephemeral_session_pubkey');
      if (!devPowerUserPubkey) {
        devPowerUserPubkey = import.meta.env.VITE_DEV_POWER_USER_PUBKEY
          || crypto.randomUUID().replace(/-/g, '').padEnd(64, '0');
        sessionStorage.setItem('ephemeral_session_pubkey', devPowerUserPubkey);
      }
      this.currentUser = {
        pubkey: devPowerUserPubkey,
        npub: this.hexToNpub(devPowerUserPubkey),
        isPowerUser: true,
      };
      this.initialized = true;
      this.notifyListeners(this.getCurrentAuthState());
      logger.info(`[DEV MODE] Authenticated as power user: ${devPowerUserPubkey}`);
      return;
    }

    // Restore cached user from localStorage (no server verification — NIP-98 is per-request)
    const storedUserJson = localStorage.getItem('nostr_user');
    if (storedUserJson) {
      try {
        this.currentUser = JSON.parse(storedUserJson);
        logger.info(`Restored user from localStorage: ${this.currentUser?.pubkey}`);
      } catch (e) {
        logger.error('Failed to parse stored user data:', createErrorMetadata(e));
        localStorage.removeItem('nostr_user');
      }
    } else {
      logger.info('No stored session found.');
    }

    // Restore passkey session from in-memory key (if available)
    this.restorePasskeySession();

    // Detect stale session: user in localStorage but no signing capability.
    // Use dual-strategy NIP-07 detection (Object.defineProperty + heuristic
    // polling) adapted from nip07-awaiter — resolves the instant the extension
    // sets window.nostr, with a 5s maximum wait.
    if (this.currentUser && !this.hasNip07Provider() && !this.isDevMode() && !this.localPrivateKey) {
      logger.info(
        'No signing key available on init — waiting for NIP-07 extension...'
      );
      this.waitForNip07Provider(5000).then((detected) => {
        if (detected) {
          logger.info('NIP-07 extension detected after init — session is valid.');
          this.notifyListeners(this.getCurrentAuthState());
        } else if (this.currentUser && !this.isAuthenticated()) {
          logger.warn(
            'Stale session confirmed: no NIP-07 extension after 5s. Clearing session.'
          );
          this.currentUser = null;
          localStorage.removeItem('nostr_user');
          try {
            sessionStorage.removeItem('nostr_passkey_pubkey');
          } catch { /* sessionStorage unavailable */ }
          this.notifyListeners(this.getCurrentAuthState());
        }
      });
    }

    this.initialized = true;
    this.notifyListeners(this.getCurrentAuthState());
    logger.debug('NostrAuthService initialized.');
  }

  public async login(): Promise<AuthState> {
    logger.info('Attempting NIP-07 login...');
    if (!this.hasNip07Provider()) {
      const errorMsg = 'Nostr NIP-07 provider (e.g., Alby) not found. Please install a compatible extension.';
      logger.error(errorMsg);
      this.notifyListeners({ authenticated: false, error: errorMsg });
      throw new Error(errorMsg);
    }

    try {
      const pubkey = await window.nostr!.getPublicKey();
      if (!pubkey) {
        throw new Error('Could not get public key from NIP-07 provider.');
      }
      logger.info(`Got pubkey via NIP-07: ${pubkey}`);

      this.currentUser = {
        pubkey,
        npub: this.hexToNpub(pubkey),
        isPowerUser: false, // Server determines this per-request from power user list
      };

      this.storeCurrentUser();
      // ADR-2205: one real signature now, none per request afterwards.
      this.bindSuspendedUntil = 0; // a new sign-in is a new session
      await this.ensureSessionKey();
      const newState = this.getCurrentAuthState();
      this.notifyListeners(newState);
      return newState;
    } catch (error: any) {
      let errorMessage = 'Login failed';
      if (error?.message?.includes('User rejected') || error?.message?.includes('extension rejected')) {
        errorMessage = 'Login request rejected in Nostr extension.';
      } else if (error?.message) {
        errorMessage = error.message;
      }
      const errorState: AuthState = { authenticated: false, error: errorMessage };
      this.notifyListeners(errorState);
      throw new Error(errorMessage);
    }
  }

  public async logout(): Promise<void> {
    logger.info('Logging out...');
    // ADR-2205: revoke the binding server-side while the key can still sign.
    // An exported key (XSS) is otherwise live until expiry. Failure is fine:
    // the binding then simply runs to its TTL.
    await this.revokeSessionKey();
    this.clearSession();
    this.notifyListeners({ authenticated: false });
  }

  /** @deprecated No session token in NIP-98 mode. Returns null. */
  public getSessionToken(): string | null {
    return null;
  }

  private storeCurrentUser(): void {
    if (this.currentUser) {
      localStorage.setItem('nostr_user', JSON.stringify(this.currentUser));
    } else {
      localStorage.removeItem('nostr_user');
    }
  }

  private clearSession(): void {
    this.currentUser = null;
    this.discardSessionKey();
    this.bindSuspendedUntil = 0;
    if (this.localPrivateKey) {
      this.localPrivateKey.fill(0);
    }
    this.localPrivateKey = null;
    if (_localKeyHex) {
      _localKeyHex = '';
    }
    _localKeyHex = null;
    localStorage.removeItem('nostr_user');
    // Clean up legacy key if present
    localStorage.removeItem('nostr_session_token');
    // Clear legacy sessionStorage entries
    try {
      sessionStorage.removeItem('nostr_privkey');
      sessionStorage.removeItem('nostr_passkey_pubkey');
      sessionStorage.removeItem('nostr_prf');
    } catch { /* sessionStorage may be unavailable */ }
  }

  public onAuthStateChanged(listener: AuthStateListener): () => void {
    this.authStateListeners.push(listener);
    if (this.initialized) {
      listener(this.getCurrentAuthState());
    }
    return () => {
      this.authStateListeners = this.authStateListeners.filter(l => l !== listener);
    };
  }

  private notifyListeners(state: AuthState): void {
    this.authStateListeners.forEach(listener => {
      try {
        listener(state);
      } catch (error) {
        logger.error('Error in auth state listener:', createErrorMetadata(error));
      }
    });
  }

  public getCurrentUser(): SimpleNostrUser | null {
    return this.currentUser;
  }

  public isAuthenticated(): boolean {
    return !!this.currentUser && (this.hasNip07Provider() || this.isDevMode() || this.localPrivateKey !== null);
  }

  public getCurrentAuthState(): AuthState {
    return {
      authenticated: this.isAuthenticated(),
      user: this.currentUser ? { ...this.currentUser } : undefined,
      error: undefined
    };
  }

  public hexToNpub(pubkey: string): string | undefined {
    if (!pubkey) return undefined;
    try {
      return nip19.npubEncode(pubkey);
    } catch (error) {
      logger.warn(`Failed to convert hex to npub: ${pubkey}`, createErrorMetadata(error));
      return undefined;
    }
  }

  public npubToHex(npub: string): string | undefined {
    if (!npub) return undefined;
    try {
      const decoded = nip19.decode(npub);
      if (decoded.type === 'npub') {
        return decoded.data;
      }
      throw new Error('Invalid npub format');
    } catch (error) {
      logger.warn(`Failed to convert npub to hex: ${npub}`, createErrorMetadata(error));
      return undefined;
    }
  }

  /**
   * Login using a passkey-derived private key.
   * Key is held in memory only and does not survive page reloads.
   */
  public async loginWithPasskey(pubkey: string, privateKey: Uint8Array): Promise<AuthState> {
    logger.info('Passkey login...');
    this.localPrivateKey = privateKey;

    // Keep hex form in module closure for signing
    const hexKey = Array.from(privateKey).map(b => b.toString(16).padStart(2, '0')).join('');
    _localKeyHex = hexKey;

    this.currentUser = {
      pubkey,
      npub: this.hexToNpub(pubkey),
      isPowerUser: false,
    };

    // Persist the (non-sensitive) pubkey so an in-session restorePasskeySession()
    // can verify the in-memory key derives to the same identity. The private key
    // is NEVER written to sessionStorage.
    try {
      sessionStorage.setItem('nostr_passkey_pubkey', pubkey);
    } catch { /* sessionStorage may be unavailable */ }

    // Persist user in localStorage for cross-reload
    this.storeCurrentUser();

    const newState = this.getCurrentAuthState();
    this.notifyListeners(newState);
    logger.info(`Passkey login complete: ${pubkey}`);
    return newState;
  }

  /**
   * Check if a passkey session exists. Prefers the in-memory key, but also
   * recognises a legacy sessionStorage key written by older code paths so a
   * restorePasskeySession() call can migrate it.
   */
  public hasPasskeySession(): boolean {
    if (!!_localKeyHex || this.localPrivateKey !== null) {
      return true;
    }
    try {
      return sessionStorage.getItem('nostr_passkey_key') !== null;
    } catch {
      return false;
    }
  }

  /**
   * Restore passkey session from the module-scoped in-memory key.
   * Returns early if no in-memory key is available (e.g. after page reload).
   */
  public restorePasskeySession(): void {
    try {
      // Prefer the in-memory key; fall back to a legacy sessionStorage copy
      // written by older, less secure code paths.
      let hexKey = _localKeyHex;
      let fromLegacyStorage = false;
      if (!hexKey) {
        try {
          const legacyKey = sessionStorage.getItem('nostr_passkey_key');
          if (legacyKey) {
            hexKey = legacyKey;
            fromLegacyStorage = true;
          }
        } catch { /* sessionStorage may be unavailable */ }
      }

      if (!hexKey) {
        // No key material anywhere — cannot restore session
        return;
      }

      const pubkey = sessionStorage.getItem('nostr_passkey_pubkey');
      // Clean nostr_privkey if present (older legacy path)
      sessionStorage.removeItem('nostr_privkey');

      if (hexKey && pubkey) {
        this.localPrivateKey = new Uint8Array(
          hexKey.match(/.{1,2}/g)!.map(byte => parseInt(byte, 16))
        );
        // Verify the key matches by deriving pubkey
        const derivedPubkey = getPublicKey(this.localPrivateKey);
        if (derivedPubkey !== pubkey) {
          logger.warn('Passkey session pubkey mismatch, clearing');
          this.localPrivateKey = null;
          _localKeyHex = null;
          sessionStorage.removeItem('nostr_passkey_pubkey');
          sessionStorage.removeItem('nostr_passkey_key');
          return;
        }
        // Migrate the key into memory and drop the insecure sessionStorage copy
        _localKeyHex = hexKey;
        if (fromLegacyStorage) {
          sessionStorage.removeItem('nostr_passkey_key');
        }
        // Set current user if not already set from localStorage
        if (!this.currentUser) {
          this.currentUser = {
            pubkey,
            npub: this.hexToNpub(pubkey),
            isPowerUser: false,
          };
        }
        logger.info(`Restored passkey session: ${pubkey}`);
      }
    } catch (e) {
      logger.warn('Failed to restore passkey session:', createErrorMetadata(e));
    }
  }

  /**
   * Dev mode login - bypasses NIP-07 and logs in as power user
   * Only available in development mode on local network
   */
  public async devLogin(): Promise<AuthState> {
    if (!import.meta.env.DEV) {
      throw new Error('Dev login is only available in development mode');
    }

    if (!isLoopbackHostname(window.location.hostname)) {
      throw new Error('Dev login is only available on a loopback origin');
    }

    logger.info('[DEV MODE] Manual dev login triggered');
    const devPowerUserPubkey = import.meta.env.VITE_DEV_POWER_USER_PUBKEY ||
      'bfcf20d472f0fb143b23cb5be3fa0a040d42176b71f73ca272f6912b1d62a452';

    this.currentUser = {
      pubkey: devPowerUserPubkey,
      npub: this.hexToNpub(devPowerUserPubkey),
      isPowerUser: true,
    };

    this.storeCurrentUser();
    const newState = this.getCurrentAuthState();
    this.notifyListeners(newState);
    logger.info(`[DEV MODE] Logged in as power user: ${devPowerUserPubkey}`);
    return newState;
  }

  public isDevLoginAvailable(): boolean {
    return !!import.meta.env.DEV && isLoopbackHostname(window.location.hostname);
  }
}

// Export a singleton instance
export const nostrAuth = NostrAuthService.getInstance();
