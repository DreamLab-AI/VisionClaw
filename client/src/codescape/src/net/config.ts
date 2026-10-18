/**
 * Server addressing. Same origin by default (under `/codescape` when served
 * through the VisionClaw port); `?server=http://host:4100` overrides for development.
 * `?demo=1` runs without a server.
 */

/** First path segments that mark Codescape as served under a shared-port prefix (VisionClaw nginx strips it). */
export const PATH_PREFIXES: readonly string[] = ['codescape'];

/**
 * The path prefix this page is served under: `'/codescape'` when the first
 * path segment is one of `segments`, otherwise `''` (served at the root).
 * Splits on segment boundaries, so `/codescapex` is not a match.
 */
export function detectPrefix(pathname: string, segments: readonly string[] = PATH_PREFIXES): string {
  const first = (pathname.startsWith('/') ? pathname : `/${pathname}`).split('/')[1] ?? '';
  return first && segments.includes(first) ? `/${first}` : '';
}

export interface Endpoints {
  /** HTTP base without trailing slash ('' = same origin root, '/codescape' under the shared port). */
  http: string;
  positions: string;
  events: string;
  voice: string;
  /** Human-readable target for the "connecting to …" state. */
  label: string;
  /** Prefix the page itself is served under, independent of `?server=` ('' = served at the root). */
  pagePrefix: string;
}

export function resolveEndpoints(loc: { protocol: string; host: string; search: string; pathname?: string }): Endpoints {
  const params = new URLSearchParams(loc.search);
  const override = params.get('server');
  const pagePrefix = detectPrefix(loc.pathname ?? '/');
  let httpBase: string;
  let wsBase: string;
  let label: string;
  if (override) {
    const u = new URL(override.includes('://') ? override : `http://${override}`);
    const p = u.pathname.replace(/\/+$/, '');
    httpBase = `${u.protocol}//${u.host}${p}`;
    wsBase = `${u.protocol === 'https:' ? 'wss:' : 'ws:'}//${u.host}${p}`;
    label = httpBase;
  } else {
    httpBase = pagePrefix;
    wsBase = `${loc.protocol === 'https:' ? 'wss:' : 'ws:'}//${loc.host}${pagePrefix}`;
    label = `${loc.protocol}//${loc.host}${pagePrefix}`;
  }
  return {
    http: httpBase,
    positions: `${wsBase}/ws/positions`,
    events: `${wsBase}/ws/events`,
    voice: `${wsBase}/ws/voice`,
    label,
    pagePrefix,
  };
}

export function isDemo(search: string): boolean {
  const v = new URLSearchParams(search).get('demo');
  return v !== null && v !== '0' && v !== 'false';
}

/** Reconnect delay: exponential from 500 ms, capped at 15 s, with ±20% jitter from `rand` in [0,1). */
export function backoffDelay(attempt: number, rand: number = Math.random()): number {
  const base = Math.min(15_000, 500 * Math.pow(2, Math.max(0, attempt)));
  return Math.round(base * (0.8 + 0.4 * rand));
}

/** Append query parameters (skipping empty values) to an absolute ws/http URL. */
export function withQuery(url: string, params: Record<string, string | null | undefined>): string {
  const u = new URL(url);
  for (const [k, v] of Object.entries(params)) if (v) u.searchParams.set(k, v);
  return u.toString();
}

/** A random per-page session key linking /ws/events, /ws/positions and /ws/voice server-side. */
export function newSessionKey(rand: () => number = Math.random): string {
  const c = (globalThis as { crypto?: Crypto }).crypto;
  if (c?.randomUUID) return c.randomUUID();
  return Array.from({ length: 4 }, () => Math.floor(rand() * 0x1_0000_0000).toString(16).padStart(8, '0')).join('-');
}
