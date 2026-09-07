import { beforeEach, describe, expect, it, vi } from 'vitest';
const fetcher = vi.hoisted(() => vi.fn());
vi.mock('../contextLoader', () => ({ fetchWithAuth: fetcher, getOntologyUrl: () => '/solid/public/ontology', logger: { debug: vi.fn(), info: vi.fn(), error: vi.fn() } }));
vi.mock('../../../../../utils/clientDebugState', () => ({ debugState: { isEnabled: () => false } }));
vi.mock('../../../../../utils/loggerConfig', () => ({ createErrorMetadata: (e: unknown) => String(e) }));
import { fetchJsonLd, fetchTurtle, makeSchemaCache } from '../schemaParser';
const a = 'a'.repeat(32), b = 'b'.repeat(32);
const metrics = () => ({ fetchCount: 0, cacheHitCount: 0, lastFetchDurationMs: 0 });
const response = (value: unknown) => ({ ok: true, json: async () => value, text: async () => value });
beforeEach(() => fetcher.mockReset());
describe('immutable generation reads', () => {
  it('pins JSON-LD and Turtle to one manifest even if the publisher advances', async () => {
    fetcher.mockResolvedValueOnce(response({ 'visionflow:generation': a }))
      .mockResolvedValueOnce(response({ '@context': {}, '@graph': [] }))
      .mockResolvedValueOnce(response('old-turtle'))
      .mockResolvedValueOnce(response({ 'visionflow:generation': b }))
      .mockResolvedValueOnce(response({ '@context': {}, '@graph': [{ '@id': 'new' }] }));
    const cache = makeSchemaCache(), m = metrics();
    await fetchJsonLd(cache, m);
    expect(await fetchTurtle(cache, m)).toBe('old-turtle');
    expect(fetcher.mock.calls.map(c => c[0])).toEqual([
      '/solid/public/ontology/index.jsonld', `/solid/public/ontology/@${a}/ontology.jsonld`, `/solid/public/ontology/@${a}/visionflow.ttl`,
    ]);
    await fetchJsonLd(cache, m, { skipCache: true });
    expect(cache.generation).toBe(b);
    expect(cache.turtle).toBeNull();
  });
  it('fails closed on missing or traversal-shaped generation identities', async () => {
    for (const generation of [undefined, '../private', 'a'.repeat(31)]) {
      fetcher.mockResolvedValueOnce(response({ 'visionflow:generation': generation }));
      await expect(fetchJsonLd(makeSchemaCache(), metrics())).rejects.toThrow('valid atomic generation');
    }
    expect(fetcher).toHaveBeenCalledTimes(3);
  });
});
