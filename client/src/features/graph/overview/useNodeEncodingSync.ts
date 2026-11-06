/**
 * Keeps the shared node encoding (state/nodeEncoding.ts, ADR-2184 D4) current.
 *
 * ONE caller encodes the whole delivered node set (graphData.nodes) at the
 * shared render index, so the three population meshes never zero each other's
 * slots. It re-encodes only when an input changes: the node list, the index
 * map, the connection counts, the colour scheme / base colour / anomaly
 * overlay / node size / per-type visuals, the analytics buffer, or an explicit
 * encoder change (`nodeEncoding.setEncoder` bumps `encoderVersion` and
 * notifies), or an explicit `requestNodeReencode()` for a change the encoder
 * object does not show (e.g. a column's data changed under the same encoder).
 * Mounted from GraphManager; its useFrame runs at -1.5, after GraphManager's
 * -2 pass and before GemNodes (-1) reads the buffers.
 */
import { useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { useSettingsStore } from '../../../store/settingsStore';
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { GraphTypeVisualsSettings, QualityGatesSettings } from '../../settings/config/settings';
import { nodeEncoding, type EncodingContext } from '../state/nodeEncoding';
import { sharedNodeIndexResolver } from '../state/nodeIndex';
import { nodeAnalyticsStore } from '../../analytics/store/nodeAnalyticsStore';
import { ANALYTICS_STRIDE, ANALYTICS_CENTRALITY_OFFSET } from '../../analytics/analyticsLayout';

let reencodeEpoch = 0;

/** Ask for a re-encode on the next frame when the encoder is unchanged but its inputs moved. */
export function requestNodeReencode(): void {
  reencodeEpoch++;
}

/** Analytics refresh cadence, frames (≈0.5 s at 60 fps, as GemNodes). */
const ANALYTICS_EVERY = 30;

export interface NodeEncodingSyncInputs {
  nodes: readonly GraphNode[];
  nodeIdToIndexMap: ReadonlyMap<string, number>;
  connectionCountMap: ReadonlyMap<string, number>;
  hierarchyMap?: Map<string, unknown>;
}

/** Comparable snapshot of every encodeAll input. */
interface Key {
  nodes: unknown; map: unknown; cc: unknown; hier: unknown; gtv: unknown; analytics: unknown;
  scheme: unknown; base: unknown; anomalies: unknown; nodeSize: unknown; epoch: number; encoder: number;
}

export function useNodeEncodingSync({ nodes, nodeIdToIndexMap, connectionCountMap, hierarchyMap }: NodeEncodingSyncInputs): void {
  const colorScheme = useSettingsStore((s) => s.get<string>('visualisation.graphs.knowledge.nodes.colorScheme')) ?? 'type';
  const baseNodeColor = useSettingsStore((s) => s.get<string>('visualisation.graphs.knowledge.nodes.baseColor'));
  const nodeSize = useSettingsStore((s) => s.get<number>('visualisation.graphs.knowledge.nodes.nodeSize')) ?? 1.0;
  const qualityGates = useSettingsStore((s) => s.get<QualityGatesSettings>('qualityGates'));
  const graphTypeVisuals = useSettingsStore((s) => s.get<GraphTypeVisualsSettings>('visualisation.graphTypeVisuals'));

  const analyticsRef = useRef<Float32Array | null>(null);
  const maxCentralityRef = useRef(0);
  const frameRef = useRef(0);
  const lastRef = useRef<Key | null>(null);
  const ctxRef = useRef<EncodingContext>({ connectionCountMap });

  useFrame(() => {
    // The encoding resolves indices through the published shared map; encode only
    // once GraphManager has published this map, so slots match nodeIdToIndexMap.
    if (sharedNodeIndexResolver() !== nodeIdToIndexMap) return;

    if (frameRef.current++ % ANALYTICS_EVERY === 0) {
      const buf = nodeAnalyticsStore.getIndexedBuffer(nodeIdToIndexMap as Map<string, number>);
      if (buf !== analyticsRef.current) {
        analyticsRef.current = buf;
        let maxC = 0;
        if (buf) for (let b = ANALYTICS_CENTRALITY_OFFSET; b < buf.length; b += ANALYTICS_STRIDE) if (buf[b] > maxC) maxC = buf[b];
        maxCentralityRef.current = maxC;
      }
    }

    const showAnomalies = qualityGates?.showAnomalies === true;
    const prev = lastRef.current;
    if (
      prev && prev.nodes === nodes && prev.map === nodeIdToIndexMap && prev.cc === connectionCountMap &&
      prev.hier === hierarchyMap && prev.gtv === graphTypeVisuals && prev.analytics === analyticsRef.current &&
      prev.scheme === colorScheme && prev.base === baseNodeColor && prev.anomalies === showAnomalies &&
      prev.nodeSize === nodeSize && prev.epoch === reencodeEpoch && prev.encoder === nodeEncoding.encoderVersion
    ) return;
    lastRef.current = {
      nodes, map: nodeIdToIndexMap, cc: connectionCountMap, hier: hierarchyMap, gtv: graphTypeVisuals,
      analytics: analyticsRef.current, scheme: colorScheme, base: baseNodeColor, anomalies: showAnomalies,
      nodeSize, epoch: reencodeEpoch, encoder: nodeEncoding.encoderVersion,
    };
    const ctx = ctxRef.current;
    ctx.connectionCountMap = connectionCountMap;
    ctx.colorScheme = colorScheme;
    ctx.baseNodeColor = baseNodeColor ?? null;
    ctx.analytics = analyticsRef.current;
    ctx.maxCentrality = maxCentralityRef.current;
    ctx.showAnomalies = showAnomalies;
    ctx.nodeSize = nodeSize;
    ctx.graphMode = 'knowledge_graph';
    ctx.hierarchyMap = hierarchyMap;
    ctx.graphTypeVisuals = graphTypeVisuals;
    nodeEncoding.encodeAll(nodes, ctx);
  }, -1.5);
}
