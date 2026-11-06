/**
 * InferredEdges — edges from the inferred named graph
 * (`urn:ngm:graph:ontology:inferred`), drawn on the overview edge path
 * (ADR-2181 Consequences: "inferred edges move onto the same path, with a dash
 * or colour distinction"): screen-space quads whose endpoints come from the
 * overview node position texture on the GPU, dashed in screen pixels and
 * tinted amber (ontosphere's amber-dashed convention, PRD-018 WS-2, ADR-099 D4).
 *
 *  - It does NOT touch the glass-tube pipeline or the GraphManager edge hot loop.
 *  - No per-frame CPU work: the endpoint pairs are written once per data change.
 *  - Gated by the `showInferred` toggle. Empty inferred set → renders nothing.
 *  - Greyed endpoints (ADR-2183) drop the edge to greyoutOpacity like any edge.
 *
 * Must be mounted inside OverviewLayer (it reads the position texture from
 * OverviewTextureContext); outside it, it renders nothing.
 */

import React, { useMemo } from 'react';
import { useInferredEdgesStore } from '../../ontology/store/useInferredEdgesStore';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GraphViewSettings } from '../../settings/config/settings';
import { useOverviewPositionTexture, resolveGraphView } from '../overview/OverviewLayer';
import { OverviewEdgeLines, type OverviewEdgeData, type OverviewEdgeStyle } from '../overview/OverviewEdgeLines';

/** Differentiated style — amber, dashed. Matches ADR-099 D4 convention. */
export const INFERRED_COLOR = '#ffb000';
/** Dash and gap, CSS px (screen-space now, so the pattern holds at every zoom). */
export const INFERRED_DASH_PX = 6;
export const INFERRED_GAP_PX = 4;
const LINE_OPACITY = 0.85;

interface InferredEdgesProps {
  /** Map from string node id to its render index (the position texture's index). */
  nodeIdToIndexMap: Map<string, number>;
}

/** Resolve inferred (source, target) ids to render-index pairs; unmapped endpoints are dropped. */
export function resolveInferredPairs(
  edges: ReadonlyArray<{ sourceId: string | number; targetId: string | number }>,
  nodeIdToIndexMap: ReadonlyMap<string, number>,
): { src: Int32Array; tgt: Int32Array; count: number } {
  const src = new Int32Array(edges.length);
  const tgt = new Int32Array(edges.length);
  let n = 0;
  for (const e of edges) {
    const si = nodeIdToIndexMap.get(String(e.sourceId));
    const ti = nodeIdToIndexMap.get(String(e.targetId));
    if (si === undefined || ti === undefined) continue;
    src[n] = si; tgt[n] = ti; n++;
  }
  return { src, tgt, count: n };
}

export const InferredEdges: React.FC<InferredEdgesProps> = ({ nodeIdToIndexMap }) => {
  const positions = useOverviewPositionTexture();
  const showInferred = useInferredEdgesStore((s) => s.showInferred);
  const inferredEdges = useInferredEdgesStore((s) => s.inferredEdges);
  const stored = useSettingsStore((s) => s.get<GraphViewSettings>('visualisation.graphView'));
  const gv = useMemo(() => resolveGraphView(stored), [stored]);

  const data = useMemo<OverviewEdgeData | null>(() => {
    const r = resolveInferredPairs(inferredEdges, nodeIdToIndexMap);
    return r.count > 0 ? { count: r.count, src: r.src, tgt: r.tgt } : null;
  }, [inferredEdges, nodeIdToIndexMap]);

  const greyOpacity = gv.explore.greyoutOpacity;
  const style = useMemo<OverviewEdgeStyle>(() => ({
    widthPx: Math.max(1.5, gv.overviewEdges.widthPx),
    maxWidthPx: Math.max(1.5, gv.overviewEdges.widthPx),
    // inferred edges are sparse and semantic: no length fade
    fadeNearPx: 1e9,
    fadeFarPx: 1e9,
    minOpacity: 1,
    opacity: LINE_OPACITY,
    greyoutOpacity: greyOpacity,
    dashed: true,
    dashPx: INFERRED_DASH_PX,
    gapPx: INFERRED_GAP_PX,
    tint: INFERRED_COLOR,
  }), [gv.overviewEdges.widthPx, greyOpacity]);

  if (!positions || !data) return null;
  return (
    <OverviewEdgeLines
      positions={positions}
      data={data}
      style={style}
      visible={showInferred}
      renderOrder={4}
      name="inferred-edges"
    />
  );
};

export default InferredEdges;
