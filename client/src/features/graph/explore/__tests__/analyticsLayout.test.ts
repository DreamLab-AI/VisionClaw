import { describe, it, expect } from 'vitest';
import * as local from '../analyticsLayout';
import * as store from '../../../analytics/store/nodeAnalyticsStore';

describe('analytics layout re-declaration', () => {
  it('matches nodeAnalyticsStore', () => {
    for (const k of Object.keys(local) as (keyof typeof local)[]) expect(local[k], k).toBe(store[k]);
  });
});
