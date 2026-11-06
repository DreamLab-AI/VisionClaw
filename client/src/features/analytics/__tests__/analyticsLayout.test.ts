/**
 * VC-56 parallel layout: the analytics buffer layout is declared once
 * (analyticsLayout.ts). Pinned here: the module imports nothing, so the
 * node-table worker can load it, and the store writes each field at its
 * declared offset.
 */
import { describe, it, expect } from 'vitest';
import * as L from '../analyticsLayout';
import layoutSource from '../analyticsLayout.ts?raw';
import { nodeAnalyticsStore } from '../store/nodeAnalyticsStore';
import { useSettingsStore } from '../../../store/settingsStore';
import type { BinaryNodeData } from '../../../types/binaryProtocol';

describe('analytics buffer layout (one declaration)', () => {
  it('imports nothing, so a worker can load it without window', () => {
    expect(layoutSource).toContain('ANALYTICS_STRIDE');
    expect(layoutSource).not.toMatch(/^\s*import\b/m);
  });

  it('the store writes every field at its declared offset', () => {
    useSettingsStore.setState({ settings: { visualisation: {} } } as never);
    nodeAnalyticsStore.clear();
    nodeAnalyticsStore.ingest([{ nodeId: 7, clusterId: 3, anomalyScore: 0.25, communityId: 5, centrality: 0.75, ssspDistance: 2 } as unknown as BinaryNodeData]);
    const buf = nodeAnalyticsStore.getIndexedBuffer(new Map([['7', 1]]))!;
    const b = 1 * L.ANALYTICS_STRIDE;
    expect(buf.length).toBe(2 * L.ANALYTICS_STRIDE);
    expect(buf[b + L.ANALYTICS_CLUSTER_OFFSET]).toBe(3);
    expect(buf[b + L.ANALYTICS_ANOMALY_OFFSET]).toBeCloseTo(0.25);
    expect(buf[b + L.ANALYTICS_COMMUNITY_OFFSET]).toBe(5);
    expect(buf[b + L.ANALYTICS_CENTRALITY_OFFSET]).toBeCloseTo(0.75);
    expect(buf[b + L.ANALYTICS_SSSP_OFFSET]).toBe(2);
    nodeAnalyticsStore.clear();
  });
});
