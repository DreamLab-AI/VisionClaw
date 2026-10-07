import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('@/services/api/authInterceptor', () => ({
  computeAuthHeaders: vi.fn(async (url: string, method: string) => ({
    Authorization: `Nostr signed:${method}:${url}`,
  })),
}));

import {
  fetchSnapshot,
  fetchVectors,
  postQuery,
  fetchHealth,
  MemoryCloudApiError,
  decodeVectors,
  resolveApiUrl,
} from '../api';
import { computeAuthHeaders } from '@/services/api/authInterceptor';
import type { MemoryCloudSnapshot, MemoryCloudQueryResponse, MemoryCloudHealth } from '../types';

const DIM = 4;

function snapshot(id: string, count = 2): MemoryCloudSnapshot {
  return {
    version: 1,
    snapshotId: id,
    generatedAt: 1_700_000_000_000,
    dim: DIM,
    count,
    positions: Array.from({ length: count * 3 }, (_, i) => i),
    metadata: Array.from({ length: count }, (_, i) => ({
      id: `id-${i}`,
      key: `key-${i}`,
      namespace: i % 2 ? 'patterns' : 'project-state',
      sourceType: 'memory',
      updatedAt: 1_700_000_000_000 + i,
    })),
    namespaces: ['patterns', 'project-state'],
    sourceTypes: ['memory'],
    strata: [],
    excludedNamespaces: [],
    vectorsUrl: `/api/memory-cloud/vectors?snapshot=${id}`,
  };
}

/** count unit rows, little-endian */
function unitRows(count: number, dim = DIM): ArrayBuffer {
  const buf = new ArrayBuffer(count * dim * 4);
  const dv = new DataView(buf);
  for (let r = 0; r < count; r++) dv.setFloat32((r * dim + (r % dim)) * 4, 1, true);
  return buf;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function binResponse(buf: ArrayBuffer, status = 200): Response {
  return new Response(buf, { status, headers: { 'content-type': 'application/octet-stream' } });
}

const fetchMock = vi.fn();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('resolveApiUrl', () => {
  it('resolves absolute paths against the page origin', () => {
    expect(resolveApiUrl('/api/memory-cloud')).toBe(`${window.location.origin}/api/memory-cloud`);
  });
  it('resolves bare relative vector URLs under /api/memory-cloud/', () => {
    expect(resolveApiUrl('vectors?snapshot=a')).toBe(
      `${window.location.origin}/api/memory-cloud/vectors?snapshot=a`,
    );
  });
});

describe('fetchSnapshot', () => {
  it('returns a validated snapshot and signs the request', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(snapshot('s1')));
    const s = await fetchSnapshot();
    expect(s.snapshotId).toBe('s1');
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(`${window.location.origin}/api/memory-cloud`);
    expect((init.headers as Record<string, string>).Authorization).toBe(
      `Nostr signed:GET:${window.location.origin}/api/memory-cloud`,
    );
    expect(computeAuthHeaders).toHaveBeenCalled();
  });

  it('rejects a snapshot whose positions do not match count', async () => {
    const bad = snapshot('s1');
    bad.positions = bad.positions.slice(1);
    fetchMock.mockResolvedValueOnce(jsonResponse(bad));
    await expect(fetchSnapshot()).rejects.toMatchObject({ kind: 'invalid' });
  });

  it('maps HTTP failures to a typed error carrying the status', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ error: 'sidecar unreachable' }, 503));
    const err = await fetchSnapshot().catch((e) => e);
    expect(err).toBeInstanceOf(MemoryCloudApiError);
    expect(err.kind).toBe('http');
    expect(err.status).toBe(503);
    expect(err.message).toContain('sidecar unreachable');
  });

  it('maps a rejected fetch to a network error', async () => {
    fetchMock.mockRejectedValueOnce(new TypeError('Failed to fetch'));
    await expect(fetchSnapshot()).rejects.toMatchObject({ kind: 'network' });
  });
});

describe('fetchVectors', () => {
  it('decodes a well-formed little-endian blob', async () => {
    const s = snapshot('s1', 3);
    fetchMock.mockResolvedValueOnce(binResponse(unitRows(3)));
    const { vectors, snapshot: used } = await fetchVectors(s);
    expect(used).toBe(s);
    expect(vectors.count).toBe(3);
    expect(vectors.dim).toBe(DIM);
    expect(vectors.data.length).toBe(3 * DIM);
    expect(vectors.data[0]).toBe(1);
    expect(vectors.data[DIM + 1]).toBe(1);
    expect(fetchMock.mock.calls[0][0]).toBe(`${window.location.origin}${s.vectorsUrl}`);
  });

  it('rejects a blob whose byte length is not count·dim·4', async () => {
    fetchMock.mockResolvedValueOnce(binResponse(unitRows(2).slice(0, 2 * DIM * 4 - 4)));
    await expect(fetchVectors(snapshot('s1', 2))).rejects.toMatchObject({ kind: 'invalid' });
  });

  it('rejects a byte-swapped (big-endian) blob via the row-norm check', async () => {
    const buf = new ArrayBuffer(2 * DIM * 4);
    const dv = new DataView(buf);
    dv.setFloat32(0, 1, false);
    dv.setFloat32((DIM + 1) * 4, 1, false);
    fetchMock.mockResolvedValueOnce(binResponse(buf));
    await expect(fetchVectors(snapshot('s1', 2))).rejects.toMatchObject({ kind: 'invalid' });
  });

  it('on 409 refetches the snapshot once and retries against it', async () => {
    const stale = snapshot('old', 2);
    const fresh = snapshot('new', 3);
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ error: 'snapshot rebuilt' }, 409))
      .mockResolvedValueOnce(jsonResponse(fresh))
      .mockResolvedValueOnce(binResponse(unitRows(3)));
    const { vectors, snapshot: used } = await fetchVectors(stale);
    expect(used.snapshotId).toBe('new');
    expect(vectors.count).toBe(3);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock.mock.calls[2][0]).toContain('snapshot=new');
  });

  it('gives up after a second 409 with a stale error', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse({}, 409))
      .mockResolvedValueOnce(jsonResponse(snapshot('new', 2)))
      .mockResolvedValueOnce(jsonResponse({}, 409));
    await expect(fetchVectors(snapshot('old', 2))).rejects.toMatchObject({ kind: 'stale', status: 409 });
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('propagates abort as a typed abort error and passes the signal to fetch', async () => {
    const ctl = new AbortController();
    fetchMock.mockImplementationOnce((_url: string, init: RequestInit) => {
      expect(init.signal).toBe(ctl.signal);
      return new Promise((_, reject) => {
        init.signal!.addEventListener('abort', () =>
          reject(new DOMException('The operation was aborted.', 'AbortError')),
        );
      });
    });
    const p = fetchVectors(snapshot('s1', 2), { signal: ctl.signal });
    ctl.abort();
    const err = await p.catch((e) => e);
    expect(err).toBeInstanceOf(MemoryCloudApiError);
    expect(err.kind).toBe('abort');
  });

  it('refuses to start when the signal is already aborted', async () => {
    const ctl = new AbortController();
    ctl.abort();
    await expect(fetchSnapshot({ signal: ctl.signal })).rejects.toMatchObject({ kind: 'abort' });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe('decodeVectors', () => {
  it('accepts zero rows', () => {
    expect(decodeVectors(new ArrayBuffer(0), 0, 384).count).toBe(0);
  });
  it('rejects non-finite values', () => {
    const buf = unitRows(1);
    new DataView(buf).setFloat32(4, Number.NaN, true);
    expect(() => decodeVectors(buf, 1, DIM)).toThrow(MemoryCloudApiError);
  });
});

describe('postQuery', () => {
  const response = (dim = DIM): MemoryCloudQueryResponse => ({
    snapshotId: 's1',
    embedModel: 'bge-small-en-v1.5',
    query: { text: 'hello', vector: Array.from({ length: dim }, (_, i) => (i === 0 ? 1 : 0)) },
    sidecar: { results: [], tookMs: 4 },
  });

  it('POSTs the body, signs it, and returns the response', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(response()));
    const r = await postQuery({ text: 'hello', k: 5, namespace: 'patterns' });
    expect(r.embedModel).toBe('bge-small-en-v1.5');
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(`${window.location.origin}/api/memory-cloud/query`);
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body)).toEqual({ text: 'hello', k: 5, namespace: 'patterns' });
    expect((init.headers as Record<string, string>)['Content-Type']).toBe('application/json');
    expect(computeAuthHeaders).toHaveBeenCalledWith(url, 'POST', init.body);
  });

  it('rejects empty text without a request', async () => {
    await expect(postQuery({ text: '   ' })).rejects.toMatchObject({ kind: 'invalid' });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('checks the query vector length when the caller knows the dimension', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(response(3)));
    await expect(postQuery({ text: 'hello' }, { expectDim: DIM })).rejects.toMatchObject({ kind: 'invalid' });
  });
});

describe('fetchHealth', () => {
  it('returns the health document', async () => {
    const h: MemoryCloudHealth = {
      snapshotId: 's1',
      generatedAt: 1,
      sidecar: { reachable: true, extensionVersion: '2.0.4', error: null },
      embedder: { url: 'http://x', model: 'bge', reachable: true },
      namespaces: [],
      recallProbe: null,
    };
    fetchMock.mockResolvedValueOnce(jsonResponse(h));
    expect(await fetchHealth()).toEqual(h);
    expect(fetchMock.mock.calls[0][0]).toBe(`${window.location.origin}/api/memory-cloud/health`);
  });
});
