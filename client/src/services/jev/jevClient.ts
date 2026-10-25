/**
 * Jev option — browser side (VC-06-S1-01/02, VC-34-S1-01/02).
 *
 * The browser never holds a vendor key and never picks a destination: it posts
 * plain command/palette data to the authenticated `/api/jev/*` routes and the
 * server builds the typed questions and resolves the endpoint.
 *
 * Fail-open contract: every function here resolves to `null` (never throws,
 * never blocks past `JEV_DEADLINE_MS`) on every failure — flag off, transport
 * error, timeout, non-2xx, malformed body, or `used:false`. A caller treats
 * `null` as "take the pre-change path". Nothing is logged with request content.
 */
import { unifiedApiClient } from '../api/UnifiedApiClient';

/** Interactive deadline, matching the server's ≤ 4 s ceiling. */
export const JEV_DEADLINE_MS = 4000;

export type JevItemSlug =
  | 'settings-patch'
  | 'action-class'
  | 'command-intent'
  | 'palette-search'
  | 'edge-meaning'
  | 'nlq-plan'
  | 'voice-intent';

export interface JevStatus {
  enabled: Partial<Record<JevItemSlug, boolean>>;
  available: boolean;
}

export interface JevSuggestion {
  id: string;
  confidence: number;
}

export interface CommandUnderstanding {
  intent?: JevSuggestion | null;
  actionClass?: JevSuggestion | null;
  setting?: JevSuggestion | null;
  valueMissing?: number | null;
}

export interface PaletteEntry {
  id: string;
  title: string;
  description?: string;
  category?: string;
}

export interface PaletteRanking {
  ranked: Array<{ id: string; score: number }>;
  noMatch: boolean;
}

export interface SettingCandidate {
  path: string;
  label: string;
  type: string;
  min?: number;
  max?: number;
  current?: unknown;
  description?: string;
}

/** Minimal transport seam; tests inject a fake. */
export interface JevApi {
  get: (url: string, opts?: { timeout?: number; retries?: number }) => Promise<{ data?: unknown } | unknown>;
  post: (url: string, body: unknown, opts?: { timeout?: number; retries?: number }) => Promise<{ data?: unknown } | unknown>;
}

const defaultApi: JevApi = {
  get: (url, opts) => unifiedApiClient.get(url, { ...opts, retries: 0 }),
  post: (url, body, opts) => unifiedApiClient.post(url, body, { ...opts, retries: 0 }),
};

function unwrap(res: unknown): Record<string, unknown> | null {
  if (res && typeof res === 'object') {
    const r = res as Record<string, unknown>;
    // ApiResponse envelope or the bare body.
    const body = r.data && typeof r.data === 'object' && !('used' in r) && !('enabled' in r) ? r.data : r;
    if (body && typeof body === 'object') return body as Record<string, unknown>;
  }
  return null;
}

async function withDeadline<T>(work: Promise<T>): Promise<T | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), JEV_DEADLINE_MS);
  });
  try {
    return await Promise.race([work.catch(() => null), deadline]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

let statusPromise: Promise<JevStatus | null> | null = null;

/** Test hook: forget the cached status. */
export function resetJevStatusCache(): void {
  statusPromise = null;
}

/** Session-cached status. `null` ⇒ Jev unavailable (treat everything as off). */
export function getJevStatus(api: JevApi = defaultApi): Promise<JevStatus | null> {
  if (!statusPromise) {
    statusPromise = withDeadline(api.get('/jev/status', { timeout: JEV_DEADLINE_MS })).then((res) => {
      const b = res ? unwrap(res) : null;
      if (!b || typeof b.enabled !== 'object' || b.enabled === null) {
        statusPromise = null; // do not pin a transient failure for the session
        return null;
      }
      return { enabled: b.enabled as JevStatus['enabled'], available: b.available === true };
    });
  }
  return statusPromise;
}

/** Whether an item is on AND an endpoint resolves server-side. Never throws. */
export async function isJevEnabled(item: JevItemSlug, api: JevApi = defaultApi): Promise<boolean> {
  const s = await getJevStatus(api);
  return !!s && s.available && s.enabled[item] === true;
}

const isSuggestion = (v: unknown): v is JevSuggestion =>
  !!v && typeof v === 'object' &&
  typeof (v as JevSuggestion).id === 'string' &&
  typeof (v as JevSuggestion).confidence === 'number';

/** Ask the server to understand a command. `null` ⇒ keep the existing path. */
export async function jevUnderstandCommand(
  command: string,
  candidates: SettingCandidate[],
  api: JevApi = defaultApi,
): Promise<CommandUnderstanding | null> {
  if (!(await isJevEnabled('settings-patch', api)) &&
      !(await isJevEnabled('action-class', api)) &&
      !(await isJevEnabled('command-intent', api))) {
    return null;
  }
  const res = await withDeadline(api.post('/jev/command', { command, candidates }, { timeout: JEV_DEADLINE_MS }));
  const b = res ? unwrap(res) : null;
  if (!b || b.used !== true) return null;
  const out: CommandUnderstanding = {
    intent: isSuggestion(b.intent) ? b.intent : null,
    actionClass: isSuggestion(b.actionClass) ? b.actionClass : null,
    setting: isSuggestion(b.setting) ? b.setting : null,
    valueMissing: typeof b.valueMissing === 'number' ? b.valueMissing : null,
  };
  return out.intent || out.actionClass || out.setting ? out : null;
}

/** Ask the server to rank palette commands. `null` ⇒ keep lexical results only. */
export async function jevRankPalette(
  query: string,
  commands: PaletteEntry[],
  api: JevApi = defaultApi,
): Promise<PaletteRanking | null> {
  if (!query.trim() || commands.length === 0) return null;
  if (!(await isJevEnabled('palette-search', api))) return null;
  const res = await withDeadline(api.post('/jev/palette', { query, commands }, { timeout: JEV_DEADLINE_MS }));
  const b = res ? unwrap(res) : null;
  if (!b || b.used !== true || !Array.isArray(b.ranked)) return null;
  const ranked: PaletteRanking['ranked'] = [];
  for (const r of b.ranked) {
    if (r && typeof r === 'object' && typeof (r as { id: unknown }).id === 'string' && typeof (r as { score: unknown }).score === 'number') {
      ranked.push({ id: (r as { id: string }).id, score: (r as { score: number }).score });
    } else {
      return null; // malformed ⇒ fall back entirely
    }
  }
  return { ranked, noMatch: b.noMatch === true };
}

/**
 * Merge Jev's ranking into the lexical result without disturbing it: the
 * lexical list is returned untouched; suggestions are only those ids that map
 * to a currently-enabled registry command not already shown. Pure.
 */
export function semanticSuggestions<T extends { id: string; enabled?: boolean }>(
  lexical: T[],
  ranking: PaletteRanking | null,
  allEnabled: T[],
  max = 3,
): T[] {
  if (!ranking || ranking.noMatch) return [];
  const shown = new Set(lexical.map((c) => c.id));
  const byId = new Map(allEnabled.filter((c) => c.enabled !== false).map((c) => [c.id, c] as const));
  const out: T[] = [];
  for (const r of ranking.ranked) {
    const c = byId.get(r.id);
    if (c && !shown.has(c.id)) out.push(c);
    if (out.length >= max) break;
  }
  return out;
}
