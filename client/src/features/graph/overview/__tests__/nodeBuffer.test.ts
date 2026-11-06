import { describe, it, expect, beforeEach } from 'vitest';
import { syncToSharedBuffer } from '../../workers/lib/buffer-manager';
import { sharedCapacityFor } from '../../managers/graphWorkerProxy';
import {
  publishNodeBufferReport, getNodeBufferReport, resetNodeBufferReport, nodeBufferNoticeText, subscribeOverviewEdgeReport,
} from '../overviewEdgeStore';

describe('worker position buffer: no silent cap (ADR-2181 D4)', () => {
  beforeEach(() => resetNodeBufferReport());

  it('syncToSharedBuffer reports the nodes that did not fit', () => {
    const view = new Float32Array(6); // 2 nodes
    const cur = new Float32Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]); // 4 nodes
    expect(syncToSharedBuffer(view, cur)).toBe(2);
    expect(Array.from(view)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(syncToSharedBuffer(new Float32Array(12), cur)).toBe(0);
    expect(syncToSharedBuffer(null, cur)).toBe(0);
  });

  it('the SAB grows from the delivered node count, to a power of two, stride 4', () => {
    expect(sharedCapacityFor(50_000, 200_000)).toBeNull(); // fits today's buffer
    expect(sharedCapacityFor(70_000, 200_000)).toBe(131_072 * 4);
    expect(sharedCapacityFor(0, 0)).toBeNull();
  });

  it('a shortfall is published for the in-canvas notice', () => {
    let calls = 0;
    const off = subscribeOverviewEdgeReport(() => { calls++; });
    publishNodeBufferReport({ total: 70_000, drawn: 66_666, truncated: true });
    expect(getNodeBufferReport().truncated).toBe(true);
    expect(nodeBufferNoticeText(getNodeBufferReport())).toBe('66,666 of 70,000 nodes drawn (position buffer full)');
    publishNodeBufferReport({ total: 70_000, drawn: 70_000, truncated: false });
    expect(nodeBufferNoticeText(getNodeBufferReport())).toBeNull();
    expect(calls).toBe(2);
    off();
  });
});
