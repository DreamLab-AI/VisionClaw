import { describe, expect, it, vi } from 'vitest';
import { loadVariantBytes, SplatFetchError, SplatIntegrityError, type SplatCacheDeps } from '../splatCache';
import { BAD_BYTES as BAD, BAD_SHA, GOOD_BYTES as GOOD, GOOD_SHA, variantFor } from './fixtures';

/** An in-memory CacheStorage that records writes and deletes. */
function fakeCaches(initial: Record<string, Uint8Array> = {}) {
  const store = new Map<string, Uint8Array>(Object.entries(initial));
  const cache = {
    match: vi.fn(async (key: string) => {
      const v = store.get(key);
      return v ? new Response(v.slice().buffer) : undefined;
    }),
    put: vi.fn(async (key: string, res: Response) => {
      store.set(key, new Uint8Array(await res.arrayBuffer()));
    }),
    delete: vi.fn(async (key: string) => store.delete(key)),
  };
  const caches = { open: vi.fn(async () => cache) } as unknown as CacheStorage;
  return { store, cache, caches };
}

function fakeFetch(bytes: Uint8Array, status = 200) {
  return vi.fn(async () => new Response(bytes.slice().buffer, { status }));
}

/** The real SubtleCrypto digest is used throughout, so GOOD_SHA and BAD_SHA are checked too. */
function deps(over: Partial<SplatCacheDeps>): SplatCacheDeps {
  return { ...over };
}

describe('loadVariantBytes', () => {
  it('has fixtures that really hash as declared', async () => {
    const hex = async (b: Uint8Array) =>
      [...new Uint8Array(await crypto.subtle.digest('SHA-256', b))].map((x) => x.toString(16).padStart(2, '0')).join('');
    expect(await hex(GOOD)).toBe(GOOD_SHA);
    expect(await hex(BAD)).toBe(BAD_SHA);
  });

  it('fetches, verifies, caches by address and returns the bytes', async () => {
    const v = variantFor('desktop');
    const { store, caches } = fakeCaches();
    const fetchMock = fakeFetch(GOOD);
    const out = await loadVariantBytes(v, deps({ caches, fetch: fetchMock as unknown as typeof fetch }));
    expect(new Uint8Array(out)).toEqual(GOOD);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe(v.url);
    expect([...store.keys()]).toEqual([`/__splat-cache__/${v.address}`]);
  });

  it('refuses a hash mismatch: throws once, deletes the entry, never fetches again', async () => {
    const v = variantFor('desktop');
    const { store, cache, caches } = fakeCaches();
    const fetchMock = fakeFetch(BAD);
    await expect(loadVariantBytes(v, deps({ caches, fetch: fetchMock as unknown as typeof fetch }))).rejects.toBeInstanceOf(
      SplatIntegrityError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(cache.delete).toHaveBeenCalledWith(`/__splat-cache__/${v.address}`);
    expect(cache.put).not.toHaveBeenCalled();
    expect(store.size).toBe(0);
  });

  it('carries the expected and actual hashes on the refusal', async () => {
    const v = variantFor('desktop');
    const err = await loadVariantBytes(v, deps({ caches: fakeCaches().caches, fetch: fakeFetch(BAD) as unknown as typeof fetch })).catch(
      (e) => e,
    );
    expect(err).toBeInstanceOf(SplatIntegrityError);
    expect(err.expected).toBe(v.sha256);
    expect(err.actual).toBe(BAD_SHA);
  });

  it('re-verifies a cache hit and serves it without touching the network', async () => {
    const v = variantFor('desktop');
    const { cache, caches } = fakeCaches({ [`/__splat-cache__/${v.address}`]: GOOD });
    const fetchMock = fakeFetch(GOOD);
    const digestSpy = vi.spyOn(crypto.subtle, 'digest');
    const out = await loadVariantBytes(v, { caches, fetch: fetchMock as unknown as typeof fetch });
    expect(new Uint8Array(out)).toEqual(GOOD);
    expect(digestSpy).toHaveBeenCalledTimes(1);
    digestSpy.mockRestore();
    expect(cache.match).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('discards a corrupt cache hit, then fetches once and verifies that', async () => {
    const v = variantFor('desktop');
    const key = `/__splat-cache__/${v.address}`;
    const { store, cache, caches } = fakeCaches({ [key]: BAD });
    const fetchMock = fakeFetch(GOOD);
    const out = await loadVariantBytes(v, deps({ caches, fetch: fetchMock as unknown as typeof fetch }));
    expect(new Uint8Array(out)).toEqual(GOOD);
    expect(cache.delete).toHaveBeenCalledWith(key);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(store.get(key)).toEqual(GOOD);
  });

  it('refuses when both the cache and the server hold wrong bytes, with one fetch', async () => {
    const v = variantFor('desktop');
    const key = `/__splat-cache__/${v.address}`;
    const { store, caches } = fakeCaches({ [key]: BAD });
    const fetchMock = fakeFetch(BAD);
    await expect(loadVariantBytes(v, deps({ caches, fetch: fetchMock as unknown as typeof fetch }))).rejects.toBeInstanceOf(
      SplatIntegrityError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(store.has(key)).toBe(false);
  });

  it('works without Cache Storage', async () => {
    const v = variantFor('desktop');
    const out = await loadVariantBytes(v, deps({ caches: undefined, fetch: fakeFetch(GOOD) as unknown as typeof fetch }));
    expect(new Uint8Array(out)).toEqual(GOOD);
  });

  it('reports an HTTP failure as a fetch error, not an integrity refusal', async () => {
    const v = variantFor('desktop');
    await expect(
      loadVariantBytes(v, deps({ caches: fakeCaches().caches, fetch: fakeFetch(GOOD, 404) as unknown as typeof fetch })),
    ).rejects.toBeInstanceOf(SplatFetchError);
  });
});
