import { describe, it, expect, vi } from 'vitest';

vi.mock('../../../../services/api/UnifiedApiClient', () => ({ unifiedApiClient: { get: vi.fn() } }));

import {
  acceptLabels, planCommunityLayers, centroidOf, domainMixLine, localCommunityCounts,
  partitionSignature, stableHslFor, type CommunityLabelEntry, type CommunityLabelsResponse,
} from '../communityLabels';

const entry = (over: Partial<CommunityLabelEntry>): CommunityLabelEntry => ({
  communityId: 1, stableId: 10, size: 4, coreSize: 3, coreIds: [1, 2, 3], label: 'Agent · Team',
  terms: ['agent', 'team'], domainMix: [{ domain: 'ai', share: 0.75 }], crossDomain: false,
  exemplars: [], hasHull: true, ...over,
});
const resp = (generation: number, communities: CommunityLabelEntry[]): CommunityLabelsResponse => ({ generation, communities });

describe('planCommunityLayers', () => {
  it('draws a hull only for hasHull communities; the rest get a chip', () => {
    const labels = resp(3, [
      entry({ communityId: 1, hasHull: true }),
      entry({ communityId: 2, hasHull: false, coreIds: [], coreSize: 0, size: 5 }),
    ]);
    const plan = planCommunityLayers(labels, new Map([[1, 4], [2, 5]]));
    expect(plan.hulls.map((e) => e.communityId)).toEqual([1]);
    expect(plan.chips.map((e) => e.communityId)).toEqual([2]);
    expect(plan.stale).toBe(0);
  });

  it('never draws a hull for hasHull with an empty core', () => {
    const plan = planCommunityLayers(resp(1, [entry({ coreIds: [] })]), new Map([[1, 4]]));
    expect(plan.hulls).toHaveLength(0);
    expect(plan.chips).toHaveLength(1);
  });

  it('drops an entry whose size no longer matches the ids the client holds (stale run)', () => {
    const plan = planCommunityLayers(resp(3, [entry({ size: 4 })]), new Map([[1, 9]]));
    expect(plan.hulls).toHaveLength(0);
    expect(plan.chips).toHaveLength(0);
    expect(plan.stale).toBe(1);
  });

  it('draws nothing before any labels arrive', () => {
    expect(planCommunityLayers(null, new Map([[1, 4]]))).toEqual({ hulls: [], chips: [], stale: 0 });
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
