/**
 * Server addressing. Same origin by default; `?server=http://host:4100`
 * overrides for development. `?demo=1` runs without a server.
 */

export interface Endpoints {
  /** HTTP base without trailing slash ('' = same origin). */
  http: string;
  positions: string;
  events: string;
  voice: string;
  /** Human-readable target for the "connecting to …" state. */
  label: string;
}

export function resolveEndpoints(loc: { protocol: string; host: string; search: string }): Endpoints {
  const params = new URLSearchParams(loc.search);
  const override = params.get('server');
  let httpBase: string;
  let wsBase: string;
  if (override) {
    const u = new URL(override.includes('://') ? override : `http://${override}`);
    httpBase = `${u.protocol}//${u.host}${u.pathname.replace(/\/+$/, '')}`;
    wsBase = `${u.protocol === 'https:' ? 'wss:' : 'ws:'}//${u.host}${u.pathname.replace(/\/+$/, '')}`;
  } else {
    httpBase = '';
    wsBase = `${loc.protocol === 'https:' ? 'wss:' : 'ws:'}//${loc.host}`;
  }
  return {
    http: httpBase,
    positions: `${wsBase}/ws/positions`,
    events: `${wsBase}/ws/events`,
    voice: `${wsBase}/ws/voice`,
    label: httpBase || `${loc.protocol}//${loc.host}`,
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
