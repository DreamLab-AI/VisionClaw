// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach } from 'vitest';
import {
  useNodeFilterCountStore,
  parseFilterCounts,
  formatFilterCounts,
} from '../nodeFilterCountStore';

/**
 * ADR-2192: the server answers a filter update with `filter_update_success` carrying
 * `visible_nodes` of `total_nodes`; the node-filter panel shows "Showing N of M nodes".
 */
describe('nodeFilterCountStore', () => {
  beforeEach(() => useNodeFilterCountStore.getState().reset());

  it('reads the counts from the server frame and rejects anything else', () => {
    expect(parseFilterCounts({ visible_nodes: 2500, total_nodes: 9688 })).toEqual({ visible: 2500, total: 9688 });
    expect(parseFilterCounts(undefined)).toBeNull();
    // the old reply carried no counts
    expect(parseFilterCounts({ enabled: true })).toBeNull();
    expect(parseFilterCounts({ visible_nodes: -1, total_nodes: 3 })).toBeNull();
    expect(parseFilterCounts({ visible_nodes: 1.5, total_nodes: 3 })).toBeNull();
    expect(parseFilterCounts({ visible_nodes: '2', total_nodes: 3 })).toBeNull();
  });

  it('formats N of M, all M, and the not-yet-answered state', () => {
    expect(formatFilterCounts({ visible: 2500, total: 9688 })).toBe('Showing 2,500 of 9,688 nodes');
    expect(formatFilterCounts({ visible: 9688, total: 9688 })).toBe('Showing all 9,688 nodes');
    expect(formatFilterCounts({ visible: null, total: null })).toBe('Node count arrives with the next filter update');
  });

  it('stores the last counts', () => {
    useNodeFilterCountStore.getState().setCounts(10, 20);
    expect(useNodeFilterCountStore.getState()).toMatchObject({ visible: 10, total: 20 });
  });
});
