import { describe, it, expect } from 'vitest';
import { recordSignature, getSignStats } from '../signStats';

describe('signStats', () => {
  it('tallies per method and path, ignoring the query string, and resets on request', () => {
    getSignStats(true);
    recordSignature('get', 'http://h:3001/api/settings/all?x=1');
    recordSignature('GET', '/api/settings/all');
    recordSignature('PUT', '/api/settings/physics');
    expect(getSignStats()).toEqual({
      total: 3,
      byPath: { 'GET /api/settings/all': 2, 'PUT /api/settings/physics': 1 },
    });
    expect(getSignStats(true).total).toBe(3);
    expect(getSignStats().total).toBe(0);
  });
});
