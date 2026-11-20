import { describe, it, expect, vi } from 'vitest';

const getMock = vi.hoisted(() => vi.fn());
vi.mock('../../../../services/api/UnifiedApiClient', () => ({ unifiedApiClient: { get: getMock } }));

import {
  fetchCommunityLabels, acceptLabels, planCommunityLayers, centroidOf, domainMixLine, localCommunityCounts,
  partitionSignature, stableHslFor, type CommunityLabelEntry, type CommunityLabelsResponse,
} from '../communityLabels';

const entry = (over: Partial<CommunityLabelEntry>): CommunityLabelEntry => ({
  communityId: 1, stableId: 10, size: 4, coreSize: 3, coreIds: [1, 2, 3], label: 'Agent · Team',
  terms: ['agent', 'team'], domainMix: [{ domain: 'ai', share: 0.75 }], crossDomain: false,
  exemplars: [], hasHull: true, ...over,
});
const resp = (generation: number, communities: CommunityLabelEntry[]): CommunityLabelsResponse => ({ generation, communities });

/** A probe over the nodes the client holds: `held` maps node id -> its local community. */
const probe = (held: Record<number, number>) => ({
  communityOf: (id: number) => held[id],
  counts: new Map<number, number>(Object.values(held).reduce((m, c) => m.set(c, (m.get(c) ?? 0) + 1), new Map<number, number>())),
});

describe('planCommunityLayers', () => {
  it('draws a hull only for hasHull communities; the rest get a chip', () => {
    const labels = resp(3, [
      entry({ communityId: 1, hasHull: true }),
      entry({ communityId: 2, hasHull: false, coreIds: [], coreSize: 0, size: 2, exemplars: [{ id: 20, label: 'x' }] }),
    ]);
    const plan = planCommunityLayers(labels, probe({ 1: 1, 2: 1, 3: 1, 20: 2, 21: 2 }));
    expect(plan.hulls.map((e) => e.communityId)).toEqual([1]);
    expect(plan.chips.map((e) => e.communityId)).toEqual([2]);
    expect(plan.stale).toBe(0);
  });

  it('never draws a hull for hasHull with an empty core', () => {
    const plan = planCommunityLayers(resp(1, [entry({ coreIds: [], size: 2 })]), probe({ 8: 1, 9: 1 }));
    expect(plan.hulls).toHaveLength(0);
    expect(plan.chips).toHaveLength(1);
  });

  it('drops an entry whose core ids carry other community ids locally (a different run)', () => {
    const plan = planCommunityLayers(resp(3, [entry({})]), probe({ 1: 7, 2: 7, 3: 7 }));
    expect(plan.hulls).toHaveLength(0);
    expect(plan.chips).toHaveLength(0);
    expect(plan.stale).toBe(1);
  });

  it('a type filter or node thinning does not make a fresh entry stale', () => {
    // The server size (4) counts pages the client filtered out; only 2 of 4 members are held.
    const plan = planCommunityLayers(resp(3, [entry({ size: 4, coreIds: [1, 2, 3] })]), probe({ 1: 1, 2: 1 }));
    expect(plan.hulls.map((e) => e.communityId)).toEqual([1]);
    expect(plan.stale).toBe(0);
  });

  it('a chip with no sample ids falls back to a count that cannot exceed the server size', () => {
    const chip = entry({ communityId: 2, hasHull: false, coreIds: [], coreSize: 0, size: 3, exemplars: [] });
    expect(planCommunityLayers(resp(1, [chip]), probe({ 5: 2, 6: 2 })).chips).toHaveLength(1);
    expect(planCommunityLayers(resp(1, [chip]), probe({ 5: 2, 6: 2, 7: 2, 8: 2 })).stale).toBe(1);
    expect(planCommunityLayers(resp(1, [chip]), probe({})).stale).toBe(1);
  });

  it('draws nothing before any labels arrive', () => {
    expect(planCommunityLayers(null, probe({ 1: 1 }))).toEqual({ hulls: [], chips: [], stale: 0 });
  });
});

describe('fetchCommunityLabels', () => {
  it('unwraps the server envelope {success, data} carried in the HTTP body', async () => {
    const body = resp(4, [entry({})]);
    getMock.mockResolvedValueOnce({ data: { success: true, data: body, error: null }, status: 200 });
    expect(await fetchCommunityLabels()).toEqual(body);
  });
  it('accepts a bare body and fails open on an error or a malformed reply', async () => {
    const body = resp(4, []);
    getMock.mockResolvedValueOnce({ data: body, status: 200 });
    expect(await fetchCommunityLabels()).toEqual(body);
    getMock.mockResolvedValueOnce({ data: { success: true, data: { nope: 1 } } });
    expect(await fetchCommunityLabels()).toBeNull();
    getMock.mockRejectedValueOnce(new Error('503'));
    expect(await fetchCommunityLabels()).toBeNull();
  });
});

describe('acceptLabels', () => {
  it('ignores a response older than the one applied', () => {
    const cur = resp(5, [entry({})]);
    expect(acceptLabels(cur, resp(4, []))).toBe(cur);
    const next = resp(6, []);
    expect(acceptLabels(cur, next)).toBe(next);
    expect(acceptLabels(cur, null)).toBe(cur);
    expect(acceptLabels(null, next)).toBe(next);
  });
});

describe('centroid, counts, colour, mix line', () => {
  it('centroid sits at the mean of the resolved core members only', () => {
    const pos = new Float32Array([0, 0, 0, 10, 0, 0, 100, 100, 100]);
    const idx = new Map([['1', 0], ['2', 1], ['3', 2]]);
    expect(centroidOf([1, 2], pos, (id) => idx.get(id))).toEqual([5, 0, 0]);
    expect(centroidOf([9], pos, (id) => idx.get(id))).toBeNull();
  });
  it('counts per community and a stable signature', () => {
    const ids = [1, 1, 0, 2, NaN];
    const counts = localCommunityCounts(ids.length, (i) => ids[i]);
    expect([...counts.entries()]).toEqual([[1, 2], [2, 1]]);
    expect(partitionSignature(counts)).toBe('1:2,2:1');
  });
  it('colour keys on the stable id, not the run id', () => {
    expect(stableHslFor({ stableId: 7, communityId: 1 })).toEqual(stableHslFor({ stableId: 7, communityId: 99 }));
    expect(stableHslFor({ stableId: 0, communityId: 5 })).toEqual(stableHslFor({ stableId: 5, communityId: 1 }));
  });
  it('mix line marks cross-domain', () => {
    const e = entry({ domainMix: [{ domain: 'spatial-computing', share: 0.4 }, { domain: 'ai', share: 0.3 }], crossDomain: true });
    expect(domainMixLine(e)).toBe('spatial computing 40% · ai 30% · cross-domain');
    expect(domainMixLine(entry({}))).toBe('ai 75%');
  });
});
