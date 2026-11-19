import { describe, it, expect, vi, beforeEach } from 'vitest';

const graph = vi.hoisted(() => ({
  nodes: [
    { id: 1, label: 'Smart Contract', metadata: { domain: 'blockchain' } },
    { id: 2, label: 'Swarm Robotics', metadata: { domain: 'robotics' } },
    { id: 3, label: 'Ledger', metadata: { domain: 'blockchain' } },
  ],
  edges: [{ id: 'e1', source: 1, target: 2 }, { id: 'e2', source: 3, target: 1 }],
}));
vi.mock('../../../graph/managers/graphDataManager', () => ({
  graphDataManager: { getGraphData: async () => graph },
}));
vi.mock('../../../../services/api/UnifiedApiClient', () => ({ unifiedApiClient: { post: vi.fn() } }));
const writeEmphasis = vi.hoisted(() => vi.fn());
vi.mock('../../../graph/explore/filterRegistry', () => ({ writeEmphasis, EMPHASIS_PREFIX: 'emph:' }));

import { runInsightFollowUp } from '../insightRunner';
import { planInsightIntent } from '../insightIntents';

beforeEach(() => writeEmphasis.mockClear());

describe('runInsightFollowUp', () => {
  it('bridges halo the cross-edge endpoints and say how many edges cross', async () => {
    const plan = planInsightIntent('what connects blockchain and robotics?')!;
    const answer = await runInsightFollowUp(plan);
    expect(writeEmphasis.mock.calls[0][2]).toEqual(['1', '2']);
    expect(answer).toMatch(/1 edges? cross/);
    expect(answer).toContain('Swarm Robotics');
  });
  it('bridges say so when nothing crosses', async () => {
    const plan = planInsightIntent('what connects blockchain and ai?')!;
    expect(await runInsightFollowUp(plan)).toMatch(/no edges cross/i);
  });
  it('anomalies answer from a zero-result response, wrapped or not', async () => {
    const plan = planInsightIntent('find the outliers')!;
    expect(await runInsightFollowUp(plan, { anomalies: [], total: 0 })).toMatch(/no outliers/i);
    expect(await runInsightFollowUp(plan, { data: { anomalies: [], total: 0 } })).toMatch(/no outliers/i);
  });
  it('hierarchy without a concept writes no emphasis and returns no extra line', async () => {
    const plan = planInsightIntent('show the backbone of the ontology')!;
    expect(await runInsightFollowUp(plan)).toBeUndefined();
    expect(writeEmphasis.mock.calls[0][2]).toBeNull();
  });
});
