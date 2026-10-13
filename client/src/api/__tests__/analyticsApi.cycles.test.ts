import { describe, it, expect, vi, beforeEach } from 'vitest';

const getData = vi.hoisted(() => vi.fn());
vi.mock('../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: { getData },
  isApiError: () => false,
}));
vi.mock('../../services/nostrAuthService', () => ({ nostrAuth: {} }));

import { analyticsAPI } from '../analyticsApi';

// The server answers with the standard envelope (`ok_json!` -> StandardResponse).
const envelope = (data: unknown) => ({ success: true, data, error: null, timestamp: 't', request_id: null });

describe('AnalyticsAPI.getCycles', () => {
  beforeEach(() => getData.mockReset());

  it('unwraps the envelope and defaults to no edgeType query', async () => {
    const report = {
      nodeCount: 5,
      edgeCount: 4,
      edgeFilter: 'subsumption',
      cyclicNodeIds: [2, 3],
      components: [[2, 3]],
    };
    getData.mockResolvedValue(envelope(report));
    await expect(analyticsAPI.getCycles()).resolves.toEqual(report);
    expect(getData).toHaveBeenCalledWith('/analytics/cycles');
  });

  it('passes an edge type through, encoded', async () => {
    getData.mockResolvedValue(envelope({ cyclicNodeIds: [], components: [] }));
    await analyticsAPI.getCycles('all');
    expect(getData).toHaveBeenCalledWith('/analytics/cycles?edgeType=all');
  });

  it('throws on an unsuccessful envelope', async () => {
    getData.mockResolvedValue({ success: false, data: null, error: 'Graph unavailable' });
    await expect(analyticsAPI.getCycles()).rejects.toThrow('Graph unavailable');
  });
});
