import { unifiedApiClient } from '../../services/api/UnifiedApiClient';
import { createLogger } from '../../utils/loggerConfig';
import type { SplatVariant } from './types';

const logger = createLogger('SplatCache');

/** Cache Storage bucket for verified splat blobs, keyed by content address. */
export const SPLAT_CACHE_NAME = 'visionclaw-splats-v1';

/**
 * The bytes do not hash to the manifest's SHA-256. This is a refusal, not a
 * retry: the entry is deleted and the blob is never fetched a second time
 * (ADR-2170 decision 10).
 */
export class SplatIntegrityError extends Error {
  constructor(
    readonly address: string,
    readonly expected: string,
    readonly actual: string,
  ) {
    super(`splat ${address} refused: sha256 ${actual} does not match manifest ${expected}`);
    this.name = 'SplatIntegrityError';
  }
}

/** The blob could not be fetched, or the browser cannot verify it. */
export class SplatFetchError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'SplatFetchError';
  }
}

/** Seams for tests; the defaults are the browser's own. */
export interface SplatCacheDeps {
  caches?: CacheStorage | undefined;
  fetch?: typeof fetch;
  digest?: (data: ArrayBuffer) => Promise<ArrayBuffer>;
}

function cacheKey(address: string): string {
  // A synthetic same-origin URL: Cache Storage keys are Requests.
  return `/__splat-cache__/${address}`;
}

function toHex(buf: ArrayBuffer): string {
  let out = '';
  for (const b of new Uint8Array(buf)) out += b.toString(16).padStart(2, '0');
  return out;
}

async function sha256Hex(data: ArrayBuffer, deps: SplatCacheDeps): Promise<string> {
  const digest = deps.digest ?? (globalThis.crypto?.subtle ? (d: ArrayBuffer) => crypto.subtle.digest('SHA-256', d) : null);
  // Without SubtleCrypto (an insecure context) nothing can be verified, so nothing is used.
  if (!digest) throw new SplatFetchError('crypto.subtle is unavailable: cannot verify splat integrity');
  return toHex(await digest(data));
}

async function openCache(deps: SplatCacheDeps): Promise<Cache | null> {
  const storage = 'caches' in deps ? deps.caches : globalThis.caches;
  if (!storage) return null;
  try {
    return await storage.open(SPLAT_CACHE_NAME);
  } catch (e) {
    logger.warn('Cache Storage unavailable; splat blobs will not persist', e);
    return null;
  }
}

async function verify(bytes: ArrayBuffer, variant: SplatVariant, deps: SplatCacheDeps): Promise<void> {
  const actual = await sha256Hex(bytes, deps);
  if (actual !== variant.sha256.toLowerCase()) {
    throw new SplatIntegrityError(variant.address, variant.sha256, actual);
  }
}

/**
 * The verified bytes of one variant.
 *
 * A cache hit is re-verified before use. Any mismatch, from the cache or from
 * the server, is a refusal: the entry is deleted, `SplatIntegrityError` is
 * thrown and nothing is fetched again within this call. A corrupt cache entry
 * therefore heals on the next explicit open, which finds no entry and fetches.
 */
export async function loadVariantBytes(variant: SplatVariant, deps: SplatCacheDeps = {}): Promise<ArrayBuffer> {
  const cache = await openCache(deps);
  const key = cacheKey(variant.address);

  if (cache) {
    const hit = await cache.match(key);
    if (hit) {
      const bytes = await hit.arrayBuffer();
      try {
        await verify(bytes, variant, deps);
        return bytes;
      } catch (e) {
        if (e instanceof SplatIntegrityError) await cache.delete(key);
        throw e;
      }
    }
  }

  const doFetch = deps.fetch ?? fetch;
  const token = unifiedApiClient.getAuthToken();
  const response = await doFetch(variant.url, {
    credentials: 'same-origin',
    headers: token ? { Authorization: `Bearer ${token}` } : undefined,
  });
  if (!response.ok) throw new SplatFetchError(`splat ${variant.address}: HTTP ${response.status}`);
  const bytes = await response.arrayBuffer();

  try {
    await verify(bytes, variant, deps);
  } catch (e) {
    if (cache) await cache.delete(key);
    throw e;
  }

  if (cache) {
    try {
      await cache.put(key, new Response(bytes.slice(0), { headers: { 'Content-Type': 'application/octet-stream' } }));
    } catch (e) {
      logger.warn(`could not cache splat ${variant.address}`, e);
    }
  }
  return bytes;
}
