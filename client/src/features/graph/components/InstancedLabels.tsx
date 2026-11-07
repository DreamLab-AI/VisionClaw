import React, { useRef, useMemo } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { createGlyphAtlas, type GlyphAtlasResult } from '../../../rendering/text/GlyphAtlas';
import { layoutText, layoutTextInline, type GlyphInstance } from '../../../rendering/text/textLayout';
import { createTextUniforms, createTslTextMaterial } from '../../../rendering/text/createTextMaterial';
import { debounceFault, onPipelineError, onRenderFault } from '../../../rendering/renderFaults';
import { isOverviewFaulted, markOverviewFault } from '../overview/overviewLive';
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { GraphVisualMode } from '../hooks/useGraphVisualState';
import { computeNodeScale } from '../utils/nodeScaling';
import type { GraphTypeVisualsSettings } from '../../settings/config/settings';

import { getDomainColor } from '../utils/domainColors';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GraphViewSettings } from '../../settings/config/settings';
import { DEFAULT_GRAPH_VIEW } from '../../../api/settings/defaults';
import { LabelBudgetRunner, worldPerPx, labelMaxWidth } from '../overview/labelRunner';
import { clusterLabelText } from '../overview/clusterLabels';
import { glyphCapacityFor, type LabelBudgets } from '../overview/labelBudget';

/** ADR-2186 label settings, defaults under the stored values. */
function useLabelBudgets(): LabelBudgets & { fontPx: number; clusterMinDistance: number } {
  const l = useSettingsStore(s => s.get<GraphViewSettings['labels']>('visualisation.graphView.labels'));
  // ADR-2186 D2: cluster labels at overview zoom = beyond the gem LOD distance.
  const lod = useSettingsStore(s => s.get<number>('visualisation.graphView.nodes.gemLodDistance'));
  const d = DEFAULT_GRAPH_VIEW.labels;
  const clusterMinDistance = lod ?? DEFAULT_GRAPH_VIEW.nodes.gemLodDistance;
  return React.useMemo(() => ({
    budget: l?.budget ?? d.budget,
    dynamicBudget: l?.dynamicBudget ?? d.dynamicBudget,
    selectedBudget: l?.selectedBudget ?? d.selectedBudget,
    fontPx: l?.fontPx ?? d.fontPx,
    clusterMinDistance,
  }), [l?.budget, l?.dynamicBudget, l?.selectedBudget, l?.fontPx, d.budget, d.dynamicBudget, d.selectedBudget, d.fontPx, clusterMinDistance]);
}

/** Cluster labels: a touch larger than node titles, a cool tint (ADR-2186 D2). */
const CLUSTER_FONT_SCALE = 1.25;
const CLUSTER_LABEL_COLOR = '#9fd3ff';

/** The cluster labels of the runner's last selection, as label lines + world centroid. */
function forEachClusterLabel(
  runner: LabelBudgetRunner, nodes: readonly GraphNode[], fontSize: number,
  fn: (line: LabelLine, x: number, y: number, z: number, sx: number, sy: number) => void,
): void {
  const sel = runner.selection;
  const c = runner.clusters;
  for (let k = 0; k < sel.clusterCount; k++) {
    const j = sel.clusters[k];
    const top = nodes[c.top[j]];
    const name = top ? String(top.label || top.id) : `Cluster ${c.id[j]}`;
    fn({ text: clusterLabelText(name, c.members[j]), color: CLUSTER_LABEL_COLOR, fontSize: fontSize * CLUSTER_FONT_SCALE },
      c.x[j], c.y[j], c.z[j], c.sx[j], c.sy[j]);
  }
}

/** Font size factor of the title line relative to the metadata lines' base. */
const META_SCALE = 0.8;

const getQualityStars = (quality?: number | string): string => {
  if (quality === undefined || quality === null) return '';
  const score = typeof quality === 'string' ? parseFloat(quality) : quality;
  if (isNaN(score)) return '';
  const normalized = score <= 1 ? score * 5 : Math.min(score, 5);
  const filled = Math.round(normalized);
  return '\u2605'.repeat(filled) + '\u2606'.repeat(5 - filled);
};

const getRecencyText = (lastModified?: string | number | Date): string => {
  if (!lastModified) return '';
  const modDate = lastModified instanceof Date ? lastModified : new Date(lastModified);
  if (isNaN(modDate.getTime())) return '';
  const diffMs = Date.now() - modDate.getTime();
  if (diffMs < 0) return 'Updated just now';
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 1) return 'Updated just now';
  if (minutes < 60) return `Updated ${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Updated ${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `Updated ${days}d ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `Updated ${months}mo ago`;
  return `Updated ${Math.floor(months / 12)}y ago`;
};

const getRecencyColor = (lastModified?: string | number | Date): string => {
  if (!lastModified) return '#666666';
  const modDate = lastModified instanceof Date ? lastModified : new Date(lastModified);
  if (isNaN(modDate.getTime())) return '#666666';
  const diffDays = (Date.now() - modDate.getTime()) / 86400000;
  if (diffDays < 1) return '#4FC3F7';
  if (diffDays < 7) return '#81C784';
  if (diffDays < 30) return '#FFD54F';
  if (diffDays < 90) return '#FFB74D';
  return '#90A4AE';
};

const ONTOLOGY_DEPTH_HEX = ['#FF6B6B', '#FFD93D', '#4ECDC4', '#AA96DA', '#95E1D3'];
const getOntologyDepthHex = (depth: number): string =>
  ONTOLOGY_DEPTH_HEX[Math.min(depth, ONTOLOGY_DEPTH_HEX.length - 1)];

const getOntologyCategory = (node: GraphNode): 'class' | 'property' | 'instance' => {
  const meta = node.metadata ?? {};
  const role = meta.role ?? meta.type ?? '';
  const rawNodeType = (node as unknown as { nodeType?: string }).nodeType;
  if (role === 'property' || rawNodeType === 'property') return 'property';
  if (role === 'instance' || rawNodeType === 'instance') return 'instance';
  return 'class';
};

const ONTOLOGY_CATEGORY_DISPLAY: Record<string, string> = {
  class: '\u25C9 Class', property: '\u25C7 Property', instance: '\u25CB Instance',
};

const AGENT_STATUS_HEX: Record<string, string> = {
  active: '#2ECC71', busy: '#F39C12', idle: '#95A5A6', error: '#E74C3C', queen: '#FFD700',
};
const getAgentStatusHex = (status?: string): string =>
  AGENT_STATUS_HEX[status ?? 'idle'] ?? '#95A5A6';

// --- End metadata overlay helpers ---

interface LabelLine {
  text: string;
  color: string;
  fontSize: number;
}

/** Minimal hierarchy node shape */
interface HierarchyNodeLike {
  depth?: number;
  childIds: string[];
}

interface SSSPResult {
  sourceNodeId: string;
  distances?: Record<string, number>;
  normalizedDistances?: Record<string, number>;
}

export interface InstancedLabelsProps {
  nodes: GraphNode[];
  nodeIdToIndexMap: Map<string, number>;
  nodePositionsRef?: React.MutableRefObject<Float32Array | null>;
  labelPositionsRef: React.MutableRefObject<Array<{ x: number; y: number; z: number }>>;
  settings: Record<string, unknown> | undefined;
  graphMode: GraphVisualMode;
  perNodeVisualModeMap: Map<string, GraphVisualMode>;
  connectionCountMap: Map<string, number>;
  hierarchyMap: Map<string, HierarchyNodeLike>;
  graphTypeVisuals: GraphTypeVisualsSettings | undefined;
  ssspResult: SSSPResult | null;
}

/** Hard ceiling of the glyph buffer; the live size is glyphCapacityFor(budgets) (ADR-2186 D1). */
const MAX_GLYPHS = 32768;
const _tempVec3 = new THREE.Vector3();
const _tempColor = new THREE.Color();


// Build label lines for a node (pure function, extracted from GraphManager NodeLabels useMemo)
function buildLabelLines(
  node: GraphNode,
  mode: GraphVisualMode,
  labelText: string,
  textColor: string,
  fontSize: number,
  metaFontSize: number,
  showMetadata: boolean,
  connectionCountMap: Map<string, number>,
  hierarchyMap: Map<string, HierarchyNodeLike>,
  ssspResult: SSSPResult | null,
): LabelLine[] {
  const lines: LabelLine[] = [];

  // SSSP distance overlay
  if (ssspResult && ssspResult.distances) {
    const dist = ssspResult.distances[node.id];
    let distanceInfo: string;
    if (node.id === ssspResult.sourceNodeId) distanceInfo = 'Source (0)';
    else if (dist === undefined || !isFinite(dist)) distanceInfo = 'Unreachable';
    else distanceInfo = `Distance: ${dist.toFixed(2)}`;

    const dColor = node.id === ssspResult.sourceNodeId ? '#00FFFF'
      : (!isFinite(ssspResult.distances[node.id] || 0) ? '#666666' : '#FFFF00');
    lines.push({ text: labelText, color: textColor, fontSize });
    lines.push({ text: distanceInfo, color: dColor, fontSize: fontSize * 0.7 });
    return lines;
  }

  if (mode === 'knowledge_graph') {
    const sourceDomain = node.metadata?.source_domain ?? '';
    const domainColor = getDomainColor(sourceDomain);
    const qualityStars = getQualityStars(node.metadata?.quality ?? node.metadata?.quality_score);
    const connectionCount = connectionCountMap.get(node.id) ?? 0;
    const recencyField = node.metadata?.lastModified ?? node.metadata?.last_modified ?? node.metadata?.updated_at;
    const recencyText = getRecencyText(recencyField);
    const recencyColor = getRecencyColor(recencyField);

    const line2Parts: string[] = [];
    if (sourceDomain) line2Parts.push(`\u25CF ${sourceDomain}`);
    if (qualityStars) line2Parts.push(qualityStars);
    const line2 = line2Parts.join('  ');
    const line3 = `\u27E8${connectionCount} link${connectionCount !== 1 ? 's' : ''}\u27E9`;

    lines.push({ text: labelText, color: sourceDomain ? domainColor : textColor, fontSize });
    if (showMetadata && line2) lines.push({ text: line2, color: sourceDomain ? domainColor : '#B0BEC5', fontSize: metaFontSize });
    if (showMetadata) lines.push({ text: line3, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
    if (showMetadata && recencyText) lines.push({ text: recencyText, color: recencyColor, fontSize: metaFontSize * 0.85 });
  } else if (mode === 'ontology') {
    const depth = node.metadata?.hierarchyDepth ?? node.metadata?.depth ?? 0;
    const instanceCount = node.metadata?.instanceCount ?? 0;
    const category = getOntologyCategory(node);
    const categoryDisplay = ONTOLOGY_CATEGORY_DISPLAY[category];
    const depthColor = getOntologyDepthHex(depth);
    const violations = node.metadata?.violations ?? 0;
    const depthLine = `\u21B3 Depth ${depth} \u00B7 ${instanceCount} instance${instanceCount !== 1 ? 's' : ''}`;
    const constraintLine = violations > 0
      ? `\u26A0 ${violations} violation${violations !== 1 ? 's' : ''}`
      : (node.metadata?.constraintValid !== undefined ? '\u2713 Valid' : '');
    const constraintColor = violations > 0 ? '#F39C12' : '#2ECC71';

    lines.push({ text: labelText, color: depthColor, fontSize });
    if (showMetadata) lines.push({ text: depthLine, color: depthColor, fontSize: metaFontSize });
    if (showMetadata) lines.push({ text: categoryDisplay, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
    if (showMetadata && constraintLine) lines.push({ text: constraintLine, color: constraintColor, fontSize: metaFontSize * 0.85 });
  } else if (mode === 'agent') {
    const agentType = (node.metadata?.agentType ?? node.metadata?.type ?? 'unknown').toUpperCase();
    const status = node.metadata?.status ?? 'idle';
    const statusColor = getAgentStatusHex(status);
    const health = node.metadata?.health ?? 100;
    const tokenRate = node.metadata?.tokenRate ?? 0;
    const activeTasks = node.metadata?.tasksActive ?? node.metadata?.tasks ?? 0;
    const statusLabel = status.charAt(0).toUpperCase() + status.slice(1);
    const agentLine2 = `\u25CF ${statusLabel}  \u2665 ${health}%`;
    const agentLine3 = `\u26A1 ${tokenRate} tok/min \u00B7 ${activeTasks} task${activeTasks !== 1 ? 's' : ''}`;

    lines.push({ text: agentType, color: statusColor, fontSize });
    if (showMetadata) lines.push({ text: agentLine2, color: statusColor, fontSize: metaFontSize });
    if (showMetadata) lines.push({ text: agentLine3, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
  } else {
    lines.push({ text: labelText, color: textColor, fontSize });
  }

  return lines;
}

// ===== Instanced glyph labels =====
// ADR-2186 D4: labels are instanced glyph quads with one TSL material
// (createTslTextMaterial), compiled by WebGPURenderer for either backend. There
// is no DOM label layer: while the material builds no labels draw, and a
// failed build or faulting draw hides them and reports a `labels` fault on the
// canvas notice (overviewLive, OverviewEdgeNotice).


/**
 * Phase 6 (ADR-04 D7 / T4): the wrapper forwards through an intermediate
 * `InstancedLabelsProps`-typed object literal. A missing field in the
 * literal is a TypeScript compile error (the object literal is annotated
 * with the full prop type, so any missing required field fails strict
 * type-checking). This prevents the historical class of "forgot to forward
 * nodePositionsRef" bugs that caused labels to lag behind SAB positions.
 */
export const InstancedLabels: React.FC<InstancedLabelsProps> = (props) => {
  // Build a fully-typed forwarding object. TypeScript enforces that every
  // required InstancedLabelsProps field is present on this literal.
  const forwardedProps: InstancedLabelsProps = {
    nodes: props.nodes,
    nodeIdToIndexMap: props.nodeIdToIndexMap,
    nodePositionsRef: props.nodePositionsRef,
    labelPositionsRef: props.labelPositionsRef,
    settings: props.settings,
    graphMode: props.graphMode,
    perNodeVisualModeMap: props.perNodeVisualModeMap,
    connectionCountMap: props.connectionCountMap,
    hierarchyMap: props.hierarchyMap,
    graphTypeVisuals: props.graphTypeVisuals,
    ssspResult: props.ssspResult,
  };

  return <InstancedLabelsGlyph {...forwardedProps} />;
};

// The wrapper `InstancedLabels` builds a typed forwarding object so missing
// props are compile errors (ADR-04 D7). The runtime guard below (for
// `nodePositionsRef` absent) remains as a developer sanity check that emits a
// single warn.
type GlyphProps = InstancedLabelsProps;

const InstancedLabelsGlyph: React.FC<GlyphProps> = ({
  nodes, nodeIdToIndexMap, nodePositionsRef, labelPositionsRef,
  settings, graphMode, perNodeVisualModeMap, connectionCountMap,
  hierarchyMap, graphTypeVisuals, ssspResult,
}) => {
  const { camera, size } = useThree();
  const meshRef = useRef<THREE.Mesh>(null);
  const budgets = useLabelBudgets();
  const runnerRef = useRef(new LabelBudgetRunner());
  // ADR-2186 D1: the glyph buffer is sized from the budget, not MAX_GLYPHS.
  const glyphCap = Math.min(MAX_GLYPHS, glyphCapacityFor(budgets));
  const frameCountRef = useRef(0);
  const diagLoggedRef = useRef(false);
  const prevCameraRef = useRef({
    x: 0, y: 0, z: 0,
    qx: 0, qy: 0, qz: 0, qw: 1,
  });
  const motionStateRef = useRef({ lastFastTime: 0 });

  // Per-node glyph tracking: for each visible node, store the glyph index range
  // and the Y offset so positions can be patched every frame without full layout rebuild.
  const nodeGlyphMapRef = useRef<Array<{
    nodeId: string;
    physicsIndex: number;     // index into labelPositionsRef
    glyphStart: number;       // first glyph index in the attribute buffers
    glyphCount: number;       // number of glyphs for this node
    labelOffsetY: number;     // Y offset above node center
  }>>([]);

  // Create atlas, material, and geometry once
  const { geometry, uniforms, atlas } = useMemo(() => {
    const a = createGlyphAtlas();
    // Fresh uniforms; the TSL material binds to this geometry and is built below.
    const uniforms = createTextUniforms(a.texture);

    // Unit quad: two triangles, 4 vertices with positions in 0..1
    const geo = new THREE.InstancedBufferGeometry();
    const quadPositions = new Float32Array([
      0, 0, 0,  1, 0, 0,  1, 1, 0,  0, 1, 0,
    ]);
    const quadIndices = new Uint16Array([0, 1, 2, 0, 2, 3]);
    geo.setIndex(new THREE.BufferAttribute(quadIndices, 1));
    geo.setAttribute('position', new THREE.BufferAttribute(quadPositions, 3));

    // Pre-allocate instanced attributes for MAX_GLYPHS
    const aLabelPos = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap * 3), 3);
    const aLocalOffset = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap * 2), 2);
    const aScale = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap * 2), 2);
    const aUVRect = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap * 4), 4);
    const aColor = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap * 3), 3);
    const aOpacity = new THREE.InstancedBufferAttribute(new Float32Array(glyphCap), 1);

    aLabelPos.setUsage(THREE.DynamicDrawUsage);
    aLocalOffset.setUsage(THREE.DynamicDrawUsage);
    aScale.setUsage(THREE.DynamicDrawUsage);
    aUVRect.setUsage(THREE.DynamicDrawUsage);
    aColor.setUsage(THREE.DynamicDrawUsage);
    aOpacity.setUsage(THREE.DynamicDrawUsage);

    geo.setAttribute('aLabelPos', aLabelPos);
    geo.setAttribute('aLocalOffset', aLocalOffset);
    geo.setAttribute('aScale', aScale);
    geo.setAttribute('aUVRect', aUVRect);
    geo.setAttribute('aColor', aColor);
    geo.setAttribute('aOpacity', aOpacity);

    geo.instanceCount = 0;

    return { geometry: geo, uniforms, atlas: a };
  }, [glyphCap]);

  // Dispose on unmount
  React.useEffect(() => {
    return () => {
      geometry.dispose();
    };
  }, [geometry]);

  // ADR-2186 D4: the glyph material for the live renderer. WebGL uses the
  // GLSL one at once; WebGPU builds the TSL twin, and reports 'failed' (the
  // DOM fallback takes over) if it cannot.
  // A TSL material is paired with the geometry it binds: after a glyphCap
  // change the new geometry never draws with the previous material.
  const [tslBuilt, setTslBuilt] = React.useState<{ material: THREE.Material; geometry: THREE.InstancedBufferGeometry } | null>(null);
  const material = tslBuilt && tslBuilt.geometry === geometry ? tslBuilt.material : null;
  React.useEffect(() => {
    let cancelled = false;
    let built: THREE.Material | null = null;
    createTslTextMaterial(uniforms, geometry).then((m) => {
      if (cancelled) { m.dispose(); return; }
      built = m;
      setTslBuilt({ material: m, geometry });
    }).catch((err) => {
      if (cancelled) return;
      console.warn('[InstancedLabels] glyph material failed to build; labels hidden:', err);
      markOverviewFault('labels', err instanceof Error ? err.message : String(err));
    });
    return () => { cancelled = true; built?.dispose(); setTslBuilt(null); };
  }, [uniforms, geometry]);

  // A draw that throws (renderObject guard) or a shader that fails on the GPU
  // hides the labels and reports the fault.
  React.useEffect(() => {
    const mesh = meshRef.current;
    if (!mesh || !material) return;
    const fault = (err: unknown) => {
      mesh.visible = false;
      console.warn('[InstancedLabels] glyph draw faulted; labels hidden:', err);
      markOverviewFault('labels', err instanceof Error ? err.message : String(err));
    };
    onRenderFault(mesh, debounceFault(fault));
    onPipelineError(material, fault);
    return () => { onRenderFault(mesh, null); onPipelineError(material, null); };
  }, [material]);

  // Priority 0 (default): runs after GraphManager (-2) and GemNodes (-1),
  // ensuring nodePositionsRef.current is populated for this frame.
  useFrame(() => {
    const mesh = meshRef.current;
    if (!mesh) return;
    if (isOverviewFaulted('labels')) { mesh.visible = false; return; }

    // --- Camera uniforms (always needed for billboard orientation) ---
    camera.updateMatrixWorld();
    uniforms.uCamRight.value.setFromMatrixColumn(camera.matrixWorld, 0);
    uniforms.uCamUp.value.setFromMatrixColumn(camera.matrixWorld, 1);

    // --- Camera motion detection: hide labels during fast movement ---
    frameCountRef.current++;
    const prev = prevCameraRef.current;
    const cdx = camera.position.x - prev.x;
    const cdy = camera.position.y - prev.y;
    const cdz = camera.position.z - prev.z;
    const posDelta = Math.sqrt(cdx * cdx + cdy * cdy + cdz * cdz);
    const rotDot = Math.abs(
      camera.quaternion.x * prev.qx + camera.quaternion.y * prev.qy +
      camera.quaternion.z * prev.qz + camera.quaternion.w * prev.qw
    );
    const rotDelta = 1.0 - rotDot;
    prev.x = camera.position.x; prev.y = camera.position.y; prev.z = camera.position.z;
    prev.qx = camera.quaternion.x; prev.qy = camera.quaternion.y;
    prev.qz = camera.quaternion.z; prev.qw = camera.quaternion.w;
    const cameraMovingFast = posDelta > 0.5 || rotDelta > 0.001;

    const now = performance.now();
    if (cameraMovingFast) motionStateRef.current.lastFastTime = now;
    // ADR-2186 D5: label selection at ~4 Hz and on camera stop, never per frame.
    const labelDue = runnerRef.current.due(now, cameraMovingFast || now - motionStateRef.current.lastFastTime < 150);
    if (cameraMovingFast) {
      // Hide all labels immediately — node spheres keep rendering at full framerate
      geometry.instanceCount = 0;
      nodeGlyphMapRef.current = [];
      return;
    }
    // Debounce: wait 150ms of stillness before rebuilding labels
    if (now - motionStateRef.current.lastFastTime < 150) return;

    // --- EVERY STILL FRAME: patch label world positions from the drawn array ---
    const labelPosAttr = geometry.getAttribute('aLabelPos') as THREE.InstancedBufferAttribute;
    const labelPosArr = labelPosAttr.array as Float32Array;
    const nodeMap = nodeGlyphMapRef.current;

    const rawPositions = nodePositionsRef?.current;
    // Phase 6 (ADR-04 D7): single warn if the runtime fallback fires —
    // means upstream forgot to forward nodePositionsRef. Should never happen
    // in practice because the wrapper enforces it at compile time.
    if (!rawPositions && !diagLoggedRef.current && nodeMap.length > 0) {
      console.warn(
        '[InstancedLabels] nodePositionsRef absent — falling back to labelPositionsRef. ' +
        'This is a bug upstream: the parent should always forward nodePositionsRef.'
      );
      diagLoggedRef.current = true;
    }
    if (nodeMap.length > 0 && rawPositions && rawPositions.length > 0) {
      for (const entry of nodeMap) {
        if (entry.physicsIndex < 0) continue;
        const pi3 = entry.physicsIndex * 3;
        if (pi3 + 2 >= rawPositions.length) continue;
        const wx = rawPositions[pi3];
        const wy = rawPositions[pi3 + 1] + entry.labelOffsetY;
        const wz = rawPositions[pi3 + 2];
        for (let g = entry.glyphStart; g < entry.glyphStart + entry.glyphCount; g++) {
          const i3 = g * 3;
          labelPosArr[i3] = wx;
          labelPosArr[i3 + 1] = wy;
          labelPosArr[i3 + 2] = wz;
        }
      }
      labelPosAttr.needsUpdate = true;
    } else if (nodeMap.length > 0) {
      const currentLabelPositions = labelPositionsRef.current;
      if (currentLabelPositions.length > 0) {
        for (const entry of nodeMap) {
          const pos = currentLabelPositions[entry.physicsIndex];
          if (!pos) continue;
          const wx = pos.x;
          const wy = pos.y + entry.labelOffsetY;
          const wz = pos.z;
          for (let g = entry.glyphStart; g < entry.glyphStart + entry.glyphCount; g++) {
            const i3 = g * 3;
            labelPosArr[i3] = wx;
            labelPosArr[i3 + 1] = wy;
            labelPosArr[i3 + 2] = wz;
          }
        }
        labelPosAttr.needsUpdate = true;
      }
    }

    // ADR-2186 D5: the layout runs with the label selection (LABEL_REFRESH_MS,
    // ≈4 Hz, and on camera stop); between refreshes only positions are patched.
    if (!labelDue) return;

    // Reuse rawPositions captured above (same drawn-array snapshot for consistency)
    const rawPositionsForLayout = rawPositions;
    const currentLabelPositions = labelPositionsRef.current;

    const labelSettings = (settings as any)?.visualisation?.graphs?.knowledge?.labels ?? (settings as any)?.visualisation?.labels;
    if (!labelSettings?.enableLabels || nodes.length === 0) {
      geometry.instanceCount = 0;
      nodeGlyphMapRef.current = [];
      return;
    }

    const nodeSettingsObj = (settings as any)?.visualisation?.graphs?.knowledge?.nodes ?? (settings as any)?.visualisation?.nodes;
    const nodeSize = nodeSettingsObj?.nodeSize ?? 0.5;
    const LABEL_DISTANCE_THRESHOLD = labelSettings?.labelDistanceThreshold ?? 1200;
    const METADATA_DISTANCE_THRESHOLD = LABEL_DISTANCE_THRESHOLD * 0.6;
    const metadataEnabled = labelSettings?.showMetadata !== false;
    const textColor = labelSettings.textColor || '#ffffff';
    const textPadding = labelSettings.textPadding ?? 0.3;
    const maxWidth = labelSettings.maxLabelWidth ?? 5.0;

    // Get attribute arrays for direct writing
    const localOffArr = (geometry.getAttribute('aLocalOffset') as THREE.InstancedBufferAttribute).array as Float32Array;
    const scaleArr = (geometry.getAttribute('aScale') as THREE.InstancedBufferAttribute).array as Float32Array;
    const uvRectArr = (geometry.getAttribute('aUVRect') as THREE.InstancedBufferAttribute).array as Float32Array;
    const colorArr = (geometry.getAttribute('aColor') as THREE.InstancedBufferAttribute).array as Float32Array;
    const opacityArr = (geometry.getAttribute('aOpacity') as THREE.InstancedBufferAttribute).array as Float32Array;

    let glyphIdx = 0;
    let visibleNodeCount = 0;
    const newNodeMap: typeof nodeGlyphMapRef.current = [];

    // ADR-2186: priority-ranked, budgeted selection (selection / overview /
    // dynamic tiers, greyed nodes excluded, screen collisions removed) —
    // the same selection the WebGPU path draws.
    const runner = runnerRef.current;
    const sel = runner.run(now, nodes, nodeIdToIndexMap, connectionCountMap, rawPositionsForLayout, camera, size, budgets, budgets.fontPx, budgets.clusterMinDistance);

    for (let k = 0; k < sel.count; k++) {
      const nodeIdx = sel.nodes[k];
      const node = nodes[nodeIdx];
      if (glyphIdx >= glyphCap - 200) break; // Reserve headroom

      const originalIndex = runner.renderIndexOf(nodeIdx);
      // Prefer the drawn-positions array (same frame as GemNodes), fall back to labelPositionsRef
      let px: number, py: number, pz: number;
      if (rawPositionsForLayout && originalIndex !== -1 && originalIndex * 3 + 2 < rawPositionsForLayout.length) {
        const pi3 = originalIndex * 3;
        px = rawPositionsForLayout[pi3];
        py = rawPositionsForLayout[pi3 + 1];
        pz = rawPositionsForLayout[pi3 + 2];
      } else {
        const physicsPos = originalIndex !== -1 ? currentLabelPositions[originalIndex] : undefined;
        const fallback = physicsPos || node.position || { x: 0, y: 0, z: 0 };
        px = fallback.x; py = fallback.y; pz = fallback.z;
      }

      _tempVec3.set(px, py, pz);
      const distanceToCamera = _tempVec3.distanceTo(camera.position);
      // Labels are sized in CSS px (graphView.labels.fontPx) at this node's depth.
      const fontSize = budgets.fontPx * worldPerPx(camera, distanceToCamera, size.height);
      const metaFontSize = fontSize * META_SCALE;
      const opacity = 1;
      const showMetadata = metadataEnabled && distanceToCamera <= METADATA_DISTANCE_THRESHOLD;
      const nodeLabelVisualMode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
      const scale = computeNodeScale(node, connectionCountMap, nodeLabelVisualMode, hierarchyMap, graphTypeVisuals);
      const labelOffsetY = scale * nodeSize + textPadding * fontSize;

      const labelText = node.label && node.label.length > 40
        ? node.label.substring(0, 37) + '...' : (node.label || node.id);

      const lines = buildLabelLines(
        node, nodeLabelVisualMode, labelText, textColor, fontSize, metaFontSize,
        showMetadata, connectionCountMap, hierarchyMap, ssspResult,
      );

      // Zero-alloc layout: write glyphs directly into attribute buffers
      const glyphStart = glyphIdx;
      const glyphCount = layoutTextInline(
        lines, atlas, labelMaxWidth(maxWidth, fontSize),
        localOffArr, scaleArr, uvRectArr, colorArr, opacityArr, labelPosArr,
        px, py + labelOffsetY, pz,
        opacity, glyphIdx, glyphCap,
        _tempColor as unknown as { r: number; g: number; b: number; set(c: string): void },
      );

      if (glyphCount === 0) continue;
      glyphIdx += glyphCount;
      visibleNodeCount++;

      // Record this node's glyph range for fast per-frame position patching
      newNodeMap.push({
        nodeId: String(node.id),
        physicsIndex: originalIndex,
        glyphStart,
        glyphCount,
        labelOffsetY,
      });
    }

    // ADR-2186 D2: cluster labels at their centroids, sized in px at their depth.
    // Not position-patched per frame (no single node): they refresh with the
    // selection at the label cadence.
    forEachClusterLabel(runner, nodes, 1, (line, cx, cy, cz) => {
      if (glyphIdx >= glyphCap - 200) return;
      _tempVec3.set(cx, cy, cz);
      const fs = budgets.fontPx * worldPerPx(camera, _tempVec3.distanceTo(camera.position), size.height);
      const glyphStart = glyphIdx;
      const glyphCount = layoutTextInline(
        [{ ...line, fontSize: line.fontSize * fs }], atlas, labelMaxWidth(maxWidth, line.fontSize * fs),
        localOffArr, scaleArr, uvRectArr, colorArr, opacityArr, labelPosArr,
        cx, cy, cz, 1, glyphIdx, glyphCap,
        _tempColor as unknown as { r: number; g: number; b: number; set(c: string): void },
      );
      if (glyphCount === 0) return;
      glyphIdx += glyphCount;
      newNodeMap.push({ nodeId: `cluster:${line.text}`, physicsIndex: -1, glyphStart, glyphCount, labelOffsetY: 0 });
    });

    nodeGlyphMapRef.current = newNodeMap;

    // Diagnostic log (rate-limited to 1 Hz to track declutter effectiveness)
    const _now = performance.now();
    const _last = (diagLoggedRef as any).lastLogMs as number | undefined;
    if (glyphIdx > 0 && (_last === undefined || _now - _last > 1000)) {
      (diagLoggedRef as any).lastLogMs = _now;
      console.debug('[InstancedLabels] layout:', {
        totalNodes: nodes.length,
        labelled: visibleNodeCount,
        budget: budgets,
        priority: runner.prioritySource,
        glyphCount: glyphIdx,
        glyphCapacity: glyphCap,
      });
    }

    // Update instance count and mark all buffers dirty
    geometry.instanceCount = glyphIdx;

    labelPosAttr.needsUpdate = true;
    (geometry.getAttribute('aLocalOffset') as THREE.InstancedBufferAttribute).needsUpdate = true;
    (geometry.getAttribute('aScale') as THREE.InstancedBufferAttribute).needsUpdate = true;
    (geometry.getAttribute('aUVRect') as THREE.InstancedBufferAttribute).needsUpdate = true;
    (geometry.getAttribute('aColor') as THREE.InstancedBufferAttribute).needsUpdate = true;
    (geometry.getAttribute('aOpacity') as THREE.InstancedBufferAttribute).needsUpdate = true;
  });

  return (
    material ? <mesh ref={meshRef} geometry={geometry} material={material} frustumCulled={false} renderOrder={10} /> : null
  );
};
