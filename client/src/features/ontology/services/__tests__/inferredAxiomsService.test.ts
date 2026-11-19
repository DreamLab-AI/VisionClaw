import { describe, it, expect, vi, beforeEach } from 'vitest';

const get = vi.hoisted(() => vi.fn());
vi.mock('@/services/api/UnifiedApiClient', () => ({ unifiedApiClient: { get } }));
vi.mock('@/utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), debug: vi.fn(), warn: vi.fn(), error: vi.fn() }),
}));

import { fetchReasoningReport, INFERRED_PAGE_SIZE, INFERRED_MAX_PAGES } from '../inferredAxiomsService';

const NGM = 'https://narrativegoldmine.com/ns/v1#';

/** Reified records for axioms [from, from+n). */
function page(from: number, n: number, total: number, truncated: boolean) {
  const triples: Array<{ s: string; p: string; o: string }> = [];
  for (let i = from; i < from + n; i++) {
    const s = `urn:ngm:axiom:${i}`;
    triples.push({ s, p: `${NGM}subject`, o: `urn:c:${i}` });
    triples.push({ s, p: `${NGM}object`, o: `urn:c:${i + 1}` });
    triples.push({ s, p: `${NGM}axiomType`, o: 'SubClassOf' });
  }
  return { data: { namedGraph: 'g', runId: 'r', triples, total, truncated } };
}

describe('fetchReasoningReport', () => {
  beforeEach(() => {
    get.mockReset();
  });

  it('reports the server total, not the size of the first page', async () => {
    get.mockResolvedValueOnce(page(0, INFERRED_PAGE_SIZE, 1200, true));
    get.mockResolvedValueOnce(page(INFERRED_PAGE_SIZE, INFERRED_PAGE_SIZE, 1200, true));
    get.mockResolvedValueOnce(page(2 * INFERRED_PAGE_SIZE, 200, 1200, false));
    const report = await fetchReasoningReport();
    expect(get).toHaveBeenCalledTimes(3);
    expect(get.mock.calls[1][0]).toContain(`offset=${INFERRED_PAGE_SIZE}`);
    expect(report.triples).toHaveLength(1200);
    expect(report.count).toBe(1200);
    expect(report.partial).toBe(false);
  });

  it('says so when the page ceiling stops the fetch short of the total', async () => {
    const total = INFERRED_PAGE_SIZE * INFERRED_MAX_PAGES + 700;
    get.mockImplementation(async (url: string) => {
      const offset = Number(/offset=(\d+)/.exec(url)![1]);
      return page(offset, INFERRED_PAGE_SIZE, total, true);
    });
    const report = await fetchReasoningReport();
    expect(get).toHaveBeenCalledTimes(INFERRED_MAX_PAGES);
    expect(report.count).toBe(total);
    expect(report.triples.length).toBe(INFERRED_PAGE_SIZE * INFERRED_MAX_PAGES);
    expect(report.partial).toBe(true);
  });

  it('degrades to the empty report when the endpoint is unavailable', async () => {
    get.mockImplementation(async () => {
      throw new Error("404");
    });
    const report = await fetchReasoningReport();
    expect(report.count).toBe(0);
  });
});
