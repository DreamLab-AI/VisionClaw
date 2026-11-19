/**
  * Tally of NIP-98 signatures (exposed on window only in dev). Every signed request (the API client,
 * the global axios interceptor, memory-cloud, LDP, WebSocket handshakes) goes
 * through `nostrAuth.signRequest`, and with a NIP-07 signer such as Podkey each
 * call is one SIGN_EVENT prompt. In the browser console:
 *
 *   __signStats()        // { total, byPath: { 'GET /api/settings/all': 3, ... } }
 *   __signStats(true)    // same, then reset (use it to measure a 30 s window)
 */
export interface SignStats {
  total: number;
  byPath: Record<string, number>;
}

let total = 0;
let byPath: Record<string, number> = {};

/** Count one signature. Query strings are dropped so polls of one path add up. */
export function recordSignature(method: string, url: string): void {
  let path = url;
  try {
    path = new URL(url, 'http://local').pathname;
  } catch {
    /* keep the raw string */
  }
  const key = `${method.toUpperCase()} ${path}`;
  total++;
  byPath[key] = (byPath[key] ?? 0) + 1;
}

export function getSignStats(reset = false): SignStats {
  const snapshot = { total, byPath: { ...byPath } };
  if (reset) {
    total = 0;
    byPath = {};
  }
  return snapshot;
}
