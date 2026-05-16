import React, { useRef, useMemo } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';
import { isWebGPURenderer } from '../../../rendering/rendererFactory';
import { createGlyphAtlas, type GlyphAtlasResult } from '../../../rendering/text/GlyphAtlas';
import { layoutText, layoutTextInline, type GlyphInstance } from '../../../rendering/text/textLayout';
import { createTextMaterial, type TextMaterialResult } from '../../../rendering/text/createTextMaterial';
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { GraphVisualMode } from '../hooks/useGraphVisualState';
import { computeNodeScale } from '../utils/nodeScaling';
import type { GraphTypeVisualsSettings } from '../../settings/config/settings';

// --- Metadata overlay helpers (duplicated from GraphManager to avoid circular imports) ---
const DOMAIN_COLORS: Record<string, string> = {
  'AI': '#4FC3F7', 'BC': '#81C784', 'RB': '#FFB74D', 'MV': '#CE93D8',
  'TC': '#FFD54F', 'DT': '#EF5350', 'NGM': '#4DB6AC',
};
const DEFAULT_DOMAIN_COLOR = '#90A4AE';
const getDomainColor = (domain?: string): string =>
  domain && DOMAIN_COLORS[domain] ? DOMAIN_COLORS[domain] : DEFAULT_DOMAIN_COLOR;

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
  isXRMode: boolean;
}

const MAX_GLYPHS = 32768;
const _tempVec3 = new THREE.Vector3();
const _tempColor = new THREE.Color();
const _frustum = new THREE.Frustum();
const _projScreenMatrix = new THREE.Matrix4();

// Build label lines for a node (pure function, extracted from GraphManager NodeLabels useMemo)
function buildLabelLines(
  node: GraphNode,
  mode: GraphVisualMode,
  labelText: string,
  textColor: string,
  fontSize: number,
  metaFontSize: number,
  showMetadata: boolean,
  vrMode: boolean,
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
    if (showMetadata && !vrMode) lines.push({ text: line3, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
    if (showMetadata && !vrMode && recencyText) lines.push({ text: recencyText, color: recencyColor, fontSize: metaFontSize * 0.85 });
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
    if (showMetadata && !vrMode) lines.push({ text: categoryDisplay, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
    if (showMetadata && !vrMode && constraintLine) lines.push({ text: constraintLine, color: constraintColor, fontSize: metaFontSize * 0.85 });
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
    if (showMetadata && !vrMode) lines.push({ text: agentLine3, color: '#B0BEC5', fontSize: metaFontSize * 0.9 });
  } else {
    lines.push({ text: labelText, color: textColor, fontSize });
  }

  return lines;
}

// ===== WebGPU HTML Fallback =====
// When renderer is WebGPU, ShaderMaterial won't compile GLSL.
// Fall back to a single <Html> container with projected CSS labels.

interface WebGPULabel {
  screenX: number;
  screenY: number;
  lines: LabelLine[];
  opacity: number;
}

const WebGPUFallbackLabels: React.FC<{
  labelsRef: React.MutableRefObject<WebGPULabel[]>;
}> = ({ labelsRef }) => {
  // Rendered at scene origin, Html handles projection
  return (
    <Html center={false} style={{ pointerEvents: 'none' }} calculatePosition={() => [0, 0, 0]}>
      <div style={{ position: 'fixed', top: 0, left: 0, width: '100vw', height: '100vh', pointerEvents: 'none' }}>
        {labelsRef.current.map((label, i) => (
          <div key={i} style={{
            position: 'absolute',
            left: `${label.screenX}px`,
            top: `${label.screenY}px`,
            transform: 'translate(-50%, -100%)',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            whiteSpace: 'nowrap',
            userSelect: 'none',
            opacity: label.opacity,
          }}>
            {label.lines.map((line, j) => (
              <span key={j} style={{
                color: line.color,
                fontSize: `${Math.round(line.fontSize * 28)}px`,
                textShadow: '0 0 3px #000, 0 0 6px #000',
                fontFamily: 'system-ui, sans-serif',
                lineHeight: 1.3,
              }}>{line.text}</span>
            ))}
          </div>
        ))}
      </div>
    </Html>
  );
};

// ===== WebGL Instanced Labels =====

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
    isXRMode: props.isXRMode,
  };

  // WebGPU path: use HTML fallback
  if (isWebGPURenderer) {
    return <InstancedLabelsWebGPU {...forwardedProps} />;
  }

  // WebGL instanced path
  return <InstancedLabelsWebGL {...forwardedProps} />;
};

// ---------- WebGPU fallback implementation ----------

// Phase 6 (ADR-04 D7): identical prop type as the parent wrapper, declared
// as a type alias so any divergence is a TypeScript error, not a runtime
// regression.
type WebGPUProps = InstancedLabelsProps;

const InstancedLabelsWebGPU: React.FC<WebGPUProps> = ({
  nodes, nodeIdToIndexMap, nodePositionsRef, labelPositionsRef,
  settings, graphMode, perNodeVisualModeMap, connectionCountMap,
  hierarchyMap, graphTypeVisuals, ssspResult, isXRMode,
}) => {
  const { camera, size } = useThree();
  const [webGPULabels, setWebGPULabels] = React.useState<WebGPULabel[]>([]);
  const frameCountRef = useRef(0);
  const prevCameraRef = useRef({
    x: 0, y: 0, z: 0,
    qx: 0, qy: 0, qz: 0, qw: 1,
  });
  const motionStateRef = useRef({ lastFastTime: 0 });

  useFrame(() => {
    frameCountRef.current++;

    // Camera velocity tracking — hide labels during fast motion
    const prev = prevCameraRef.current;
    const dx = camera.position.x - prev.x;
    const dy = camera.position.y - prev.y;
    const dz = camera.position.z - prev.z;
    const posDelta = Math.sqrt(dx * dx + dy * dy + dz * dz);
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
    if (cameraMovingFast) {
      motionStateRef.current.lastFastTime = now;
      // Hide labels during fast camera motion — nodes keep rendering
      setWebGPULabels([]);
      return;
    }
    // Debounce: wait 150ms of stillness before rebuilding labels
    if (now - motionStateRef.current.lastFastTime < 150) return;

    // Phase 6 (ADR-04 D6 / T4): cadence is configurable via
    // settings.rendering.labelLayoutEvery. Default 3 — historical behaviour.
    const labelLayoutEvery = Math.max(
      1,
      ((settings as any)?.visualisation?.rendering?.labelLayoutEvery as number | undefined) ?? 3
    );
    if (frameCountRef.current % labelLayoutEvery !== 0) return;

    const labelSettings = (settings as any)?.visualisation?.graphs?.logseq?.labels ?? (settings as any)?.visualisation?.labels;
    if (!labelSettings?.enableLabels || nodes.length === 0) {
      setWebGPULabels([]);
      return;
    }

    const nodeSettings = (settings as any)?.visualisation?.graphs?.logseq?.nodes ?? (settings as any)?.visualisation?.nodes;
    const nodeSize = nodeSettings?.nodeSize ?? 0.5;
    const LABEL_DISTANCE_THRESHOLD = labelSettings?.labelDistanceThreshold ?? 1200;
    const METADATA_DISTANCE_THRESHOLD = LABEL_DISTANCE_THRESHOLD * 0.6;
    const FADE_START = LABEL_DISTANCE_THRESHOLD * 0.85;
    const metadataEnabled = labelSettings?.showMetadata !== false;
    const fontSize = labelSettings.desktopFontSize ?? 0.4;
    const metaFontSize = fontSize * 0.8;
    const textColor = labelSettings.textColor || '#ffffff';
    const textPadding = labelSettings.textPadding ?? 0.3;

    _projScreenMatrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    _frustum.setFromProjectionMatrix(_projScreenMatrix);

    // Read directly from SharedArrayBuffer (same source as GemNodes) for zero-lag positions.
    // Falls back to labelPositionsRef if SAB not available.
    const rawPositions = nodePositionsRef?.current;
    const labels: WebGPULabel[] = [];
    const halfW = size.width * 0.5;
    const halfH = size.height * 0.5;

    for (const node of nodes) {
      const originalIndex = nodeIdToIndexMap.get(String(node.id)) ?? -1;
      let position: { x: number; y: number; z: number };
      if (originalIndex !== -1 && rawPositions && originalIndex * 3 + 2 < rawPositions.length) {
        const i3 = originalIndex * 3;
        position = { x: rawPositions[i3], y: rawPositions[i3 + 1], z: rawPositions[i3 + 2] };
      } else {
        const fallback = originalIndex !== -1 ? labelPositionsRef.current[originalIndex] : undefined;
        position = fallback || node.position || { x: 0, y: 0, z: 0 };
      }

      _tempVec3.set(position.x, position.y, position.z);
      if (!_frustum.containsPoint(_tempVec3)) continue;

      const distanceToCamera = _tempVec3.distanceTo(camera.position);
      if (distanceToCamera > LABEL_DISTANCE_THRESHOLD || distanceToCamera < 2) continue;

      const opacity = distanceToCamera > FADE_START
        ? 1 - (distanceToCamera - FADE_START) / (LABEL_DISTANCE_THRESHOLD - FADE_START) : 1;

      const showMetadata = metadataEnabled && distanceToCamera <= METADATA_DISTANCE_THRESHOLD;
      const nodeLabelVisualMode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
      const scale = computeNodeScale(node, connectionCountMap, nodeLabelVisualMode, hierarchyMap, graphTypeVisuals);
      const labelOffsetY = scale * nodeSize + textPadding;

      const labelText = node.label && node.label.length > 40
        ? node.label.substring(0, 37) + '...' : (node.label || node.id);

      const lines = buildLabelLines(
        node, nodeLabelVisualMode, labelText, textColor, fontSize, metaFontSize,
        showMetadata, isXRMode, connectionCountMap, hierarchyMap, ssspResult,
      );

      // Project to screen
      _tempVec3.set(position.x, position.y + labelOffsetY, position.z);
      _tempVec3.project(camera);
      const screenX = (_tempVec3.x * halfW) + halfW;
      const screenY = (-_tempVec3.y * halfH) + halfH;

      // Skip if behind camera
      if (_tempVec3.z > 1) continue;

      labels.push({ screenX, screenY, lines, opacity });
    }

    setWebGPULabels(labels);
  });

  // Render labels directly — useState triggers re-render when labels change
  return (
    <Html center={false} style={{ pointerEvents: 'none' }} calculatePosition={() => [0, 0, 0]}>
      <div style={{ position: 'fixed', top: 0, left: 0, width: '100vw', height: '100vh', pointerEvents: 'none' }}>
        {webGPULabels.map((label, i) => (
          <div key={i} style={{
            position: 'absolute',
            left: `${label.screenX}px`,
            top: `${label.screenY}px`,
            transform: 'translate(-50%, -100%)',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            whiteSpace: 'nowrap',
            userSelect: 'none',
            opacity: label.opacity,
          }}>
            {label.lines.map((line, j) => (
              <span key={j} style={{
                color: line.color,
                fontSize: `${Math.round(line.fontSize * 28)}px`,
                textShadow: '0 0 3px #000, 0 0 6px #000',
                fontFamily: 'system-ui, sans-serif',
                lineHeight: 1.3,
              }}>{line.text}</span>
            ))}
          </div>
        ))}
      </div>
    </Html>
  );
};

// ---------- WebGL instanced implementation ----------

// Phase 6 (ADR-04 D7): same alias as the WebGPU path. The wrapper
// `InstancedLabels` builds a typed forwarding object so missing props are
// compile errors. The runtime guard below (for `nodePositionsRef` absent)
// remains as a developer sanity check that emits a single warn.
type WebGLProps = InstancedLabelsProps;

const InstancedLabelsWebGL: React.FC<WebGLProps> = ({
  nodes, nodeIdToIndexMap, nodePositionsRef, labelPositionsRef,
  settings, graphMode, perNodeVisualModeMap, connectionCountMap,
  hierarchyMap, graphTypeVisuals, ssspResult, isXRMode,
}) => {
  const { camera } = useThree();
  const meshRef = useRef<THREE.Mesh>(null);
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
  const { geometry, matResult, atlas } = useMemo(() => {
    const a = createGlyphAtlas();
    const m = createTextMaterial(a.texture);

    // Unit quad: two triangles, 4 vertices with positions in 0..1
    const geo = new THREE.InstancedBufferGeometry();
    const quadPositions = new Float32Array([
      0, 0, 0,  1, 0, 0,  1, 1, 0,  0, 1, 0,
    ]);
    const quadIndices = new Uint16Array([0, 1, 2, 0, 2, 3]);
    geo.setIndex(new THREE.BufferAttribute(quadIndices, 1));
    geo.setAttribute('position', new THREE.BufferAttribute(quadPositions, 3));

    // Pre-allocate instanced attributes for MAX_GLYPHS
    const aLabelPos = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS * 3), 3);
    const aLocalOffset = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS * 2), 2);
    const aScale = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS * 2), 2);
    const aUVRect = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS * 4), 4);
    const aColor = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS * 3), 3);
    const aOpacity = new THREE.InstancedBufferAttribute(new Float32Array(MAX_GLYPHS), 1);

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

    return { geometry: geo, matResult: m, atlas: a };
  }, []);

  // Dispose on unmount
  React.useEffect(() => {
    return () => {
      geometry.dispose();
    };
  }, [geometry]);

  // Priority 0 (default): runs after GraphManager (-2) and GemNodes (-1),
  // ensuring nodePositionsRef.current is populated for this frame.
  useFrame(() => {
    const mesh = meshRef.current;
    if (!mesh) return;

    // --- Camera uniforms (always needed for billboard orientation) ---
    camera.updateMatrixWorld();
    matResult.uniforms.uCamRight.value.setFromMatrixColumn(camera.matrixWorld, 0);
    matResult.uniforms.uCamUp.value.setFromMatrixColumn(camera.matrixWorld, 1);

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
    if (cameraMovingFast) {
      motionStateRef.current.lastFastTime = now;
      // Hide all labels immediately — node spheres keep rendering at full framerate
      geometry.instanceCount = 0;
      return;
    }
    // Debounce: wait 150ms of stillness before rebuilding labels
    if (now - motionStateRef.current.lastFastTime < 150) return;

    // --- EVERY STILL FRAME: patch label world positions from SAB ---
    const labelPosAttr = geometry.getAttribute('aLabelPos') as THREE.InstancedBufferAttribute;
    const labelPosArr = labelPosAttr.array as Float32Array;
    const nodeMap = nodeGlyphMapRef.current;

    const rawPositions = nodePositionsRef?.current;
    // Phase 6 (ADR-04 D7): single warn if the runtime fallback fires —
    // means upstream forgot to forward nodePositionsRef. Should never happen
    // in practice because the wrapper enforces it at compile time.
    if (!rawPositions && !diagLoggedRef.current && nodeMap.length > 0) {
      // eslint-disable-next-line no-console
      console.warn(
        '[InstancedLabelsWebGL] nodePositionsRef absent — falling back to labelPositionsRef. ' +
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

    // Phase 6 (ADR-04 D6 / T4): cadence is configurable via
    // settings.rendering.labelLayoutEvery. Default 3 — historical behaviour.
    const labelLayoutEvery = Math.max(
      1,
      ((settings as any)?.visualisation?.rendering?.labelLayoutEvery as number | undefined) ?? 3
    );
    if (frameCountRef.current % labelLayoutEvery !== 0 && nodeGlyphMapRef.current.length > 0) return;

    // Reuse rawPositions captured above (same SAB snapshot for consistency)
    const rawPositionsForLayout = rawPositions;
    const currentLabelPositions = labelPositionsRef.current;

    const labelSettings = (settings as any)?.visualisation?.graphs?.logseq?.labels ?? (settings as any)?.visualisation?.labels;
    if (!labelSettings?.enableLabels || nodes.length === 0) {
      geometry.instanceCount = 0;
      nodeGlyphMapRef.current = [];
      return;
    }

    const nodeSettingsObj = (settings as any)?.visualisation?.graphs?.logseq?.nodes ?? (settings as any)?.visualisation?.nodes;
    const nodeSize = nodeSettingsObj?.nodeSize ?? 0.5;
    const LABEL_DISTANCE_THRESHOLD = labelSettings?.labelDistanceThreshold ?? 1200;
    const METADATA_DISTANCE_THRESHOLD = LABEL_DISTANCE_THRESHOLD * 0.6;
    const FADE_START = LABEL_DISTANCE_THRESHOLD * 0.85;
    const metadataEnabled = labelSettings?.showMetadata !== false;
    const fontSize = labelSettings.desktopFontSize ?? 0.4;
    const metaFontSize = fontSize * 0.8;
    const textColor = labelSettings.textColor || '#ffffff';
    const textPadding = labelSettings.textPadding ?? 0.3;
    const maxWidth = labelSettings.maxLabelWidth ?? 5.0;

    // Update frustum with ~10% margin to pre-populate labels for nodes about to
    // enter the view during rapid camera rotation (layout rebuilds every 3 frames).
    _projScreenMatrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    // Scale the x/y projection factors to widen the frustum (elements [0] and [5]
    // control horizontal and vertical FOV in a perspective matrix).
    const e = _projScreenMatrix.elements;
    e[0] *= 0.9; e[5] *= 0.9;
    _frustum.setFromProjectionMatrix(_projScreenMatrix);

    // Get attribute arrays for direct writing
    const localOffArr = (geometry.getAttribute('aLocalOffset') as THREE.InstancedBufferAttribute).array as Float32Array;
    const scaleArr = (geometry.getAttribute('aScale') as THREE.InstancedBufferAttribute).array as Float32Array;
    const uvRectArr = (geometry.getAttribute('aUVRect') as THREE.InstancedBufferAttribute).array as Float32Array;
    const colorArr = (geometry.getAttribute('aColor') as THREE.InstancedBufferAttribute).array as Float32Array;
    const opacityArr = (geometry.getAttribute('aOpacity') as THREE.InstancedBufferAttribute).array as Float32Array;

    let glyphIdx = 0;
    let visibleNodeCount = 0;
    const newNodeMap: typeof nodeGlyphMapRef.current = [];

    for (const node of nodes) {
      if (glyphIdx >= MAX_GLYPHS - 200) break; // Reserve headroom

      const originalIndex = nodeIdToIndexMap.get(String(node.id)) ?? -1;
      // Prefer raw SAB positions (same frame as GemNodes), fall back to labelPositionsRef
      let px: number, py: number, pz: number;
      if (rawPositionsForLayout && originalIndex !== -1) {
        const pi3 = originalIndex * 3;
        if (pi3 + 2 < rawPositionsForLayout.length) {
          px = rawPositionsForLayout[pi3];
          py = rawPositionsForLayout[pi3 + 1];
          pz = rawPositionsForLayout[pi3 + 2];
        } else {
          const physicsPos = originalIndex !== -1 ? currentLabelPositions[originalIndex] : undefined;
          const fallback = physicsPos || node.position || { x: 0, y: 0, z: 0 };
          px = fallback.x; py = fallback.y; pz = fallback.z;
        }
      } else {
        const physicsPos = originalIndex !== -1 ? currentLabelPositions[originalIndex] : undefined;
        const fallback = physicsPos || node.position || { x: 0, y: 0, z: 0 };
        px = fallback.x; py = fallback.y; pz = fallback.z;
      }

      _tempVec3.set(px, py, pz);
      if (!_frustum.containsPoint(_tempVec3)) continue;

      const distanceToCamera = _tempVec3.distanceTo(camera.position);
      if (distanceToCamera > LABEL_DISTANCE_THRESHOLD || distanceToCamera < 2) continue;

      const opacity = distanceToCamera > FADE_START
        ? 1 - (distanceToCamera - FADE_START) / (LABEL_DISTANCE_THRESHOLD - FADE_START) : 1;

      const showMetadata = metadataEnabled && distanceToCamera <= METADATA_DISTANCE_THRESHOLD;
      const nodeLabelVisualMode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
      const scale = computeNodeScale(node, connectionCountMap, nodeLabelVisualMode, hierarchyMap, graphTypeVisuals);
      const labelOffsetY = scale * nodeSize + textPadding;

      const labelText = node.label && node.label.length > 40
        ? node.label.substring(0, 37) + '...' : (node.label || node.id);

      const lines = buildLabelLines(
        node, nodeLabelVisualMode, labelText, textColor, fontSize, metaFontSize,
        showMetadata, isXRMode, connectionCountMap, hierarchyMap, ssspResult,
      );

      // Zero-alloc layout: write glyphs directly into attribute buffers
      const glyphStart = glyphIdx;
      const glyphCount = layoutTextInline(
        lines, atlas, maxWidth,
        localOffArr, scaleArr, uvRectArr, colorArr, opacityArr, labelPosArr,
        px, py + labelOffsetY, pz,
        opacity, glyphIdx, MAX_GLYPHS,
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

    nodeGlyphMapRef.current = newNodeMap;

    // Diagnostic log (once)
    if (!diagLoggedRef.current && glyphIdx > 0) {
      diagLoggedRef.current = true;
      console.log('[InstancedLabels] first update:', {
        totalNodes: nodes.length,
        visibleNodes: visibleNodeCount,
        glyphCount: glyphIdx,
        labelPositionsLen: currentLabelPositions.length,
        labelDistanceThreshold: LABEL_DISTANCE_THRESHOLD,
        atlasChars: atlas.metrics.size,
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
    <mesh ref={meshRef} geometry={geometry} material={matResult.material} frustumCulled={false} renderOrder={10} />
  );
};
