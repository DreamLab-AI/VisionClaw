/**
 * The desktop overview layer (ADR-2179..2183): screen-space edges and node
 * sprites over one GPU node source.
 *
 * - Owns the NodePositionTexture and refreshes it after the edge pass
 *   (priority −0.4) from the drawn positions and the highlight mask; it
 *   uploads only when either changed, so a settled graph does no per-node or
 *   per-edge CPU work.
 * - Overview edges (ADR-2181): the renderable edge plan (useEdgeBufferComputation),
 *   capacity from `graphView.overviewEdges.capacity` via
 *   resolveOverviewEdgeCapacity, the heaviest edges kept when truncated, and
 *   {capacity, drawn, total, truncated} published for the in-canvas notice.
 * - Overview sprites (ADR-2182): OverviewNodes mirrors the knowledge gem mesh.
 *
 * Exposes the position texture through OverviewTextureContext so the inferred
 * edges draw on the same path.
 */
import React, { createContext, useContext, useEffect, useMemo } from 'react';
import { clearOverviewFaults, setOverviewPositions } from './overviewLive';
import { useFrame } from '@react-three/fiber';
import { useSettingsStore } from '../../../store/settingsStore';
import { DEFAULT_EDGES_SETTINGS, DEFAULT_GRAPH_VIEW } from '../../../api/settings/defaults';
import type { GraphViewSettings } from '../../settings/config/settings';
import { highlightMask } from '../state/highlightMask';
import { resolveOverviewEdgeCapacity } from '../state/overviewEdgeCapacity';
import type { EdgePlan } from '../hooks/edgeBufferPlan';
import type { GemNodesHandle } from '../components/GemNodes';
import { NodePositionTexture } from './nodePositionTexture';
import { selectDrawnEdges, overviewEdgeOpacity } from './edgeStyle';
import { publishOverviewEdgeReport, resetOverviewEdgeReport } from './overviewEdgeStore';
import { OverviewEdgeLines, type OverviewEdgeData, type OverviewEdgeStyle } from './OverviewEdgeLines';
import { OverviewNodes } from './OverviewNodes';

export const OverviewTextureContext = createContext<NodePositionTexture | null>(null);
export const useOverviewPositionTexture = () => useContext(OverviewTextureContext);

/** Resolve the stored graph-view settings with the shipped defaults under them. */
export function resolveGraphView(gv: Partial<GraphViewSettings> | undefined): GraphViewSettings {
  const d = DEFAULT_GRAPH_VIEW as unknown as GraphViewSettings;
  return {
    fitTarget: gv?.fitTarget ?? d.fitTarget,
    overviewEdges: { ...d.overviewEdges, ...(gv?.overviewEdges ?? {}) },
    nodes: { ...d.nodes, ...(gv?.nodes ?? {}) },
    explore: { ...d.explore, ...(gv?.explore ?? {}) },
    labels: { ...d.labels, ...(gv?.labels ?? {}) },
  };
}

interface Props {
  plan: EdgePlan | null;
  nodePositionsRef: React.MutableRefObject<Float32Array | null>;
  nodeCount: number;
  knowledgeGemRef: React.RefObject<GemNodesHandle | null>;
  children?: React.ReactNode;
}

export const OverviewLayer: React.FC<Props> = ({ plan, nodePositionsRef, nodeCount, knowledgeGemRef, children }) => {
  const stored = useSettingsStore((s) => s.get<GraphViewSettings>('visualisation.graphView'));
  const gv = useMemo(() => resolveGraphView(stored), [stored]);
  const nodeSize = useSettingsStore((s) => s.get<number>('visualisation.graphs.knowledge.nodes.nodeSize')) ?? 1.0;
  const background = useSettingsStore((s) => s.get<string>('visualisation.rendering.backgroundColor')) ?? '#000000';

  const positions = useMemo(() => new NodePositionTexture(), []);
  useEffect(() => () => positions.dispose(), [positions]);

  useFrame(() => {
    const p = nodePositionsRef.current;
    if (!p || nodeCount === 0) return;
    positions.update(p, nodeCount, highlightMask.getMask(), highlightMask.version);
    // GraphManager publishes nodePositionsRef only once the stream has delivered
    // non-zero positions, so from here the texture holds the real layout.
    setOverviewPositions(true);
  }, -0.4);
  useEffect(() => () => setOverviewPositions(false), []);

  // ADR-2181 D4: capacity from the data, heaviest first when the budget is short.
  const oe = gv.overviewEdges;
  const edgeData = useMemo<OverviewEdgeData | null>(() => {
    if (!plan || !oe.enabled) return null;
    const cap = resolveOverviewEdgeCapacity(oe.capacity, plan.count);
    const pick = cap.truncated ? selectDrawnEdges(plan.weights, plan.count, cap.capacity) : null;
    return { count: cap.drawn, src: plan.srcIdx, tgt: plan.tgtIdx, colors: plan.colors, weights: plan.weights, pick };
  }, [plan, oe.enabled, oe.capacity]);

  useEffect(() => {
    if (!plan || !oe.enabled) { resetOverviewEdgeReport(); return; }
    const cap = resolveOverviewEdgeCapacity(oe.capacity, plan.count);
    publishOverviewEdgeReport({ capacity: cap.capacity, drawn: cap.drawn, total: plan.count, truncated: cap.truncated });
  }, [plan, oe.enabled, oe.capacity]);
  useEffect(() => () => { resetOverviewEdgeReport(); clearOverviewFaults(); }, []);

  // ADR-2183 D3: the scene blends in linear space on both backends, so the
  // setting is used as is.
  const greyOpacity = gv.explore.greyoutOpacity;
  // The one Edge Opacity control (Look › Edges, and the chat) drives the
  // overview lines as well as the close-zoom tubes.
  const edgeOpacitySetting = useSettingsStore((s) => s.get<number>('visualisation.graphs.knowledge.edges.opacity'));
  const edgeOpacity = overviewEdgeOpacity(edgeOpacitySetting, DEFAULT_EDGES_SETTINGS.opacity);
  const edgeStyle = useMemo<OverviewEdgeStyle>(() => ({
    widthPx: oe.widthPx,
    maxWidthPx: oe.maxWidthPx,
    fadeNearPx: oe.fadeNearPx,
    fadeFarPx: oe.fadeFarPx,
    minOpacity: oe.minOpacity,
    opacity: edgeOpacity,
    greyoutOpacity: greyOpacity,
  }), [oe.widthPx, oe.maxWidthPx, oe.fadeNearPx, oe.fadeFarPx, oe.minOpacity, edgeOpacity, greyOpacity]);

  const nodeStyle = useMemo(() => ({
    spriteSizePx: gv.nodes.spriteSizePx,
    nodeSize,
    greyoutOpacity: greyOpacity,
    background,
    halo: gv.nodes.bloomScope !== 'off',
  }), [gv.nodes.spriteSizePx, gv.nodes.bloomScope, nodeSize, greyOpacity, background]);

  return (
    <OverviewTextureContext.Provider value={positions}>
      <OverviewEdgeLines positions={positions} data={edgeData} style={edgeStyle} visible={oe.enabled} publishLive />
      <OverviewNodes positions={positions} sourceRef={knowledgeGemRef} style={nodeStyle} />
      {children}
    </OverviewTextureContext.Provider>
  );
};
