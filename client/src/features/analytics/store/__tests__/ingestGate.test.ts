/**
 * ADR-2189 retired qualityGates.showClusters / showCommunities. showClusters
 * defaulted on, and that alone kept analytics ingestion live for the ADR-2186
 * label budget (ranked by centrality) and the ADR-2184 explore encodings. The
 * gate now names those consumers, so retiring the toggles stops nothing.
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { nodeAnalyticsStore } from '../nodeAnalyticsStore';
import { useSettingsStore } from '../../../../store/settingsStore';
import type { BinaryNodeData } from '../../../../types/binaryProtocol';

const frame = [{ nodeId: 1, centrality: 0.5, clusterId: 2 } as unknown as BinaryNodeData];

function setSettings(patch: Record<string, unknown>) {
  useSettingsStore.setState({ settings: patch } as never);
}

beforeEach(() => nodeAnalyticsStore.clear());

describe('analytics ingestion gate without the retired cluster toggles', () => {
  it('ingests for the label budget by default (no quality gates set)', () => {
    setSettings({ visualisation: {} });
    nodeAnalyticsStore.ingest(frame);
    expect(nodeAnalyticsStore.getIndexedBuffer(new Map([['1', 0]]))).not.toBeNull();
  });

  it('ingests for an explore colour-by column even with the label budget at 0', () => {
    setSettings({ visualisation: { graphView: { labels: { budget: 0 }, explore: { colorBy: 'community' } } } });
    nodeAnalyticsStore.ingest(frame);
    expect(nodeAnalyticsStore.getIndexedBuffer(new Map([['1', 0]]))).not.toBeNull();
  });

  it('skips the scan when no consumer is on', () => {
    setSettings({ visualisation: { graphView: { labels: { budget: 0 }, explore: { colorBy: 'scheme', sizeBy: 'none' } } } });
    nodeAnalyticsStore.ingest(frame);
    expect(nodeAnalyticsStore.getIndexedBuffer(new Map([['1', 0]]))).toBeNull();
  });
});
