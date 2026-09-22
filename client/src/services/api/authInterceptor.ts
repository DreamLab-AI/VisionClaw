import { nostrAuth } from '../nostrAuthService';
import { createLogger } from '../../utils/loggerConfig';
import type { RequestConfig } from './UnifiedApiClient';
import { v4 as uuidv4 } from 'uuid';

const logger = createLogger('AuthInterceptor');

export function generateRequestId(): string {
  return uuidv4();
}

/**
 * ADR-06 §D1 + resolution T2: skipAuth is INFORMATIONAL on the client side.
 *
 * The server decides whether to honour `Bearer dev-session-token`. In a release
 * build (compiled without `--features dev-auth`) the token-acceptance branch
 * is physically absent from the binary, so any request carrying the dev token
 * will receive a `401 Unauthorized`. We surface that mismatch via console.warn
 * so dev-mode users running against a production server see why their requests
 * are failing.
 *
 * Detection heuristic: a 401 response on a request that carried the dev token
 * with no other auth scheme. The actual decision is the server's; this warning
 * does not affect the request.
 */
let releaseModeWarned = false;
function warnIfServerInReleaseMode(status: number, sentDevToken: boolean): void {
  if (releaseModeWarned) return;
  if (status === 401 && sentDevToken) {
    releaseModeWarned = true;
    console.warn(
      '[AuthInterceptor] Server returned 401 on Bearer dev-session-token. ' +
      'The server is likely a release build compiled WITHOUT `--features dev-auth` ' +
      '(ADR-06 §D1). `?skipAuth=true` has no effect against a release build. ' +
      'Rebuild the server with `./scripts/launch.sh up dev` to enable the dev-auth gate.'
    );
  }
}

/**
 * Single source of truth for the dev-mode-vs-NIP-98 auth header branch.
 *
 * Returns `{}` when not authenticated. When authenticated in dev mode,
 * returns the `Bearer dev-session-token` header (see the release-mode note
 * above `warnIfServerInReleaseMode`) plus `X-Nostr-Pubkey` when a pubkey is
 * known. Otherwise signs the request per NIP-98 and returns an
 * `Authorization: Nostr <token>` header. NIP-07 extensions like Podkey may
 * also intercept, but their retry-on-401 approach is unreliable for
 * PUT/POST mutations, so we always sign ourselves.
 */
export async function computeAuthHeaders(
  fullUrl: string,
  method: string,
  body?: string,
): Promise<Record<string, string>> {
  if (!nostrAuth.isAuthenticated()) {
    return {};
  }

  const user = nostrAuth.getCurrentUser();

  if (nostrAuth.isDevMode()) {
    const headers: Record<string, string> = {
      Authorization: 'Bearer dev-session-token',
    };
    if (user?.pubkey) {
      headers['X-Nostr-Pubkey'] = user.pubkey;
    }
    return headers;
  }

  if (!user?.pubkey) {
    return {};
  }

  const token = await nostrAuth.signRequest(fullUrl, method, body);
  return { Authorization: `Nostr ${token}` };
}

export async function authRequestInterceptor(config: RequestConfig, url: string): Promise<RequestConfig> {
  const finalConfig = { ...config };

  if (!finalConfig.headers) {
    finalConfig.headers = {} as Record<string, string>;
  }

  const headers = finalConfig.headers as Record<string, string>;

  const requestId = generateRequestId();
  headers['X-Request-ID'] = requestId;

  if (nostrAuth.isAuthenticated()) {
    const fullUrl = new URL(url, window.location.origin).href;
    const method = (finalConfig.method || 'GET').toUpperCase();
    const body = typeof finalConfig.body === 'string' ? finalConfig.body : undefined;
    const devMode = nostrAuth.isDevMode();

    try {
      Object.assign(headers, await computeAuthHeaders(fullUrl, method, body));
      if (devMode) {
        logger.debug(`[${requestId}] Dev-mode auth headers for ${url}`);
      } else {
        logger.debug(`[${requestId}] NIP-98 signed request for ${method} ${url}`);
      }
    } catch (e) {
      logger.error(`[${requestId}] NIP-98 signing failed:`, e);
    }
  } else {
    logger.debug(`[${requestId}] No auth headers (not authenticated) for ${url}`);
  }

  finalConfig.headers = headers;
  return finalConfig;
}

/**
 * Response interceptor that detects a release-mode server rejecting the dev
 * token, and emits a one-shot console.warn so dev users immediately understand
 * why `?skipAuth=true` is not working.
 */
export async function authResponseInterceptor<T>(
  response: { status: number; headers?: Record<string, string> } & T,
  config?: RequestConfig,
): Promise<typeof response> {
  const headers = (config?.headers || {}) as Record<string, string>;
  const sentDevToken = headers['Authorization'] === 'Bearer dev-session-token';
  warnIfServerInReleaseMode(response.status, sentDevToken);
  return response;
}

export function initializeAuthInterceptor(apiClient: any): void {
  apiClient.setInterceptors({
    onRequest: authRequestInterceptor,
    onResponse: authResponseInterceptor,
  });

  logger.info('Authentication interceptor initialized for UnifiedApiClient');
}

export function setupAuthStateListener(): void {
  nostrAuth.onAuthStateChanged((state) => {
    if (state.authenticated) {
      logger.info('Authentication state changed: User authenticated', {
        pubkey: state.user?.pubkey?.slice(0, 8) + '...',
      });
    } else {
      logger.info('Authentication state changed: User logged out');
    }
  });
}
