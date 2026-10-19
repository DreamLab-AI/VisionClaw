/**
 * The signed-in user's power-user flag, as the server sees it.
 *
 * The sign-in paths in nostrAuthService set `isPowerUser: false` outside dev
 * mode and never ask the server, so power-user-only settings stayed read-only
 * for every real power user (estate register ER-26). The server's verified
 * `GET /api/admin/rbac/whoami` reports the flag for the request's own NIP-98
 * signer; the shared API client signs the request.
 */
import { unifiedApiClient } from './api/UnifiedApiClient';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('powerUserStatus');

interface WhoamiResponse {
  pubkey: string;
  role: string;
  is_power_user: boolean;
}

/**
 * Ask the server whether `pubkey` is a power user. Returns `null` when the
 * server cannot say, or when it answers for a different key than expected,
 * so a caller keeps whatever it already had.
 */
export async function fetchPowerUserStatus(pubkey: string): Promise<boolean | null> {
  try {
    const { data } = await unifiedApiClient.get<WhoamiResponse>('/api/admin/rbac/whoami');
    if (!data || typeof data.is_power_user !== 'boolean') return null;
    // An answer that does not name this key is not about this user.
    if (typeof data.pubkey !== 'string' || data.pubkey.toLowerCase() !== pubkey.toLowerCase()) {
      logger.warn('whoami did not answer for this key; ignoring it');
      return null;
    }
    return data.is_power_user;
  } catch {
    // A 401/403 or a network error: keep the flag the caller already has.
    return null;
  }
}
