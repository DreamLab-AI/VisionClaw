import React, { useRef, useMemo, useCallback, useEffect, forwardRef, useImperativeHandle } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import type { GraphVisualMode } from './GraphManager';
import type { Node as GraphNode } from '../managers/graphDataManager';
import { createGemNodeMaterial, createTslGemMaterial, createGemGeometry, applyTslInstanceTransform } from '../../../rendering/materials/GemNodeMaterial';
import { createCrystalOrbMaterial, createTslCrystalOrbMaterial, createCrystalOrbGeometry } from '../../../rendering/materials/CrystalOrbMaterial';
import { createAgentCapsuleMaterial, createTslAgentCapsuleMaterial, createAgentCapsuleGeometry } from '../../../rendering/materials/AgentCapsuleMaterial';
import { applyGlslMetadataGlow, GLSL_GLOW_PRESETS } from '../../../rendering/materials/GlslMetadataGlow';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GemMaterialSettings, GraphTypeVisualsSettings, QualityGatesSettings } from '../../settings/config/settings';
import type { Edge } from '../managers/graphDataManager';
import type { ThreeEvent } from '@react-three/fiber';
import { createLogger } from '../../../utils/loggerConfig';
// ADR-03 D7: analytics buffer no longer lives in the worker — it is rebuilt on
// the main thread from V3 binary frames. Stride 5 (ADR-031 D2):
// [clusterId, anomalyScore, communityId, centrality, ssspDistance].
import {
  nodeAnalyticsStore,
  ANALYTICS_STRIDE,
  ANALYTICS_CLUSTER_OFFSET,
  ANALYTICS_ANOMALY_OFFSET,
  ANALYTICS_COMMUNITY_OFFSET,
  ANALYTICS_CENTRALITY_OFFSET,
  ANALYTICS_SSSP_OFFSET,
} from '../../analytics/store/nodeAnalyticsStore';

const logger = createLogger('GemNodes');
import { computeNodeScale } from '../utils/nodeScaling';
import { isWebGPURenderer } from '../../../rendering/rendererFactory';
import { getTypeColor, getDomainColor } from '../hooks/useGraphNodeColors';
import { agentStatusActivity } from '../../bots/agentVisualConstants';
import { getAgentWork, AGENT_DONE_ACTIVITY } from '../../bots/agentWorkTargets';
import { attentionHeat } from '../../visualisation/attentionHeat';
import { heatBrightenFactor } from '../../visualisation/heatColor';

/** Minimal hierarchy node shape compatible with HierarchyNode from hierarchyDetector */
interface HierarchyNodeLike {
  depth?: number;
}

/** SSSP (Single Source Shortest Path) analysis result */
interface SSSPResult {
  sourceNodeId: string;
  distances?: Record<string, number>;
  normalizedDistances?: Record<string, number>;
}

export interface GemNodesProps {
  nodes: GraphNode[];
  edges: Edge[];
  graphMode: GraphVisualMode;
  perNodeVisualModeMap: Map<string, GraphVisualMode>;
  /**
   * Force this geometry/material regardless of node-population majority. When
   * set, the component renders exactly ONE population's primitive (gem / orb /
   * capsule) so a mixed graph can show all three populations simultaneously via
   * sibling GemNodes meshes. When unset, falls back to the dominant-mode pick.
   */
  forceMode?: GraphVisualMode;
  /**
   * Added to each reported instanceId so the parent's shared pointer handler
   * resolves the correct GLOBAL node when this mesh renders a CONTIGUOUS slice
   * of the parent's displayNodes (per-population multi-mesh). Default 0.
   */
  instanceIdBase?: number;
  nodePositionsRef: React.MutableRefObject<Float32Array | null>;
  connectionCountMap: Map<string, number>;
  hierarchyMap: Map<string, HierarchyNodeLike>;
  nodeIdToIndexMap: Map<string, number>;
  settings: Record<string, unknown> | undefined;
  ssspResult: SSSPResult | null;
  onPointerDown: (event: ThreeEvent<PointerEvent>) => void;
  onPointerMove: (event: ThreeEvent<PointerEvent>) => void;
  onPointerUp: (event: ThreeEvent<PointerEvent>) => void;
  onPointerMissed: () => void;
  onDoubleClick: (event: ThreeEvent<MouseEvent>) => void;
  /** Right-click a node — used by the additive-expansion context menu. */
  onContextMenu?: (event: ThreeEvent<MouseEvent>) => void;
  selectedNodeId: string | null;
  /** Live drag state ref — when set, the dragged node uses this position instead of SAB */
  dragDataRef?: React.MutableRefObject<{
    isDragging: boolean;
    nodeId: string | null;
    currentNodePos3D: { x: number; y: number; z: number };
  }>;
}

export interface GemNodesHandle {
  getMesh: () => THREE.InstancedMesh | null;
  getColorArray: () => Float32Array | null;
}

/** Round up to next power of 2 (minimum 1). */
const nextPowerOf2 = (n: number): number => Math.pow(2, Math.ceil(Math.log2(Math.max(n, 1))));

// Width of the per-instance metadata DataTexture. WebGPU caps texture
// dimensions at 8192; a 1D (count×1) layout overflows that once the instance
// capacity exceeds 8192 (a 31k-node graph rounds up to 32768), which makes the
// texture — and the node bind group that samples it — invalid, so the gems
// silently fail to render on the WebGPU backend (WebGL's higher cap masked it).
// Keep the width a power of 2 well under the cap and grow in the height axis.
const METADATA_TEX_WIDTH = 2048;

// Attention-heat (task V1) re-uploads the metadata texture's w channel at most
// ~2Hz. The DataTexture upload is the real cost; heat decays continuously but
// the eye cannot resolve a faster emissive fade, so we sample it on this cadence.
const HEAT_UPLOAD_INTERVAL_MS = 500;

// Agent momentum-nudge: per-frame lerp factors controlling how quickly an
// agent's visual position converges on its work target (NUDGE) or drifts
// outward from the graph centre when idle (DRIFT).
const AGENT_NUDGE_SPEED = 0.03;
const AGENT_DRIFT_SPEED = 0.005;

// Node scaling delegated to shared computeNodeScale (../utils/nodeScaling.ts)

const getDominantMode = (
  nodes: GraphNode[], global: GraphVisualMode, perNode: Map<string, GraphVisualMode>,
): GraphVisualMode => {
  if (perNode.size === 0) return global;
  const c: Record<string, number> = { knowledge_graph: 0, ontology: 0, agent: 0 };
  for (const n of nodes) c[perNode.get(String(n.id)) || global]++;
  let best = global, max = -1;
  for (const [m, v] of Object.entries(c)) if (v > max) { max = v; best = m as GraphVisualMode; }
  return best;
};

const _mat = new THREE.Matrix4();
const _col = new THREE.Color();
// Task #54: reusable temps for the GPU-transform raycast (zero per-pick alloc).
const _rayCenter = new THREE.Vector3();
const _raySphere = new THREE.Sphere();
const _rayHit = new THREE.Vector3();
const _baseHSL = { h: 0, s: 0, l: 0 };
const ONTOLOGY_SPECTRUM = ['#FF6B6B', '#FFD93D', '#4ECDC4', '#AA96DA', '#95E1D3'];
const AGENT_STATUS_MAP: Record<string, string> = {
  active: '#2ECC71', busy: '#F39C12', idle: '#95A5A6', error: '#E74C3C',
};

/** Deterministic hue from string (0-1). Used for node-label-based color differentiation. */
const hashHue = (s: string): number => {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return ((h >>> 0) % 360) / 360;
};

/** Map a lastModified timestamp to 0-1 recency (1 = recent, 0 = stale). */
const computeRecency = (lastModified: string | number | undefined): number => {
  if (!lastModified) return 0.3;
  const ms = typeof lastModified === 'number' ? lastModified : Date.parse(String(lastModified));
  if (isNaN(ms)) return 0.3;
  const ageSec = (Date.now() - ms) / 1000;
  return Math.max(0.01, Math.exp(-ageSec / 3600)); // 0s->1.0, 1h->~0.37, 4h->~0.02
};

const GemNodesInner: React.ForwardRefRenderFunction<GemNodesHandle, GemNodesProps> = (props, ref) => {
  const {
    nodes, graphMode, perNodeVisualModeMap, nodePositionsRef,
    connectionCountMap, hierarchyMap, settings, ssspResult,
    onPointerDown, onPointerMove, onPointerUp, onPointerMissed, onDoubleClick, onContextMenu,
    selectedNodeId, forceMode, instanceIdBase,
  } = props;

  const meshRef = useRef<THREE.InstancedMesh | null>(null);
  const metaTexRef = useRef<THREE.DataTexture | null>(null);
  // Task #50 (WebGL parity): live uniform driving the GLSL per-instance glow
  // scale (uGlowStrength). Ticked each frame from the per-type glow strength so
  // the injected shader breathes per-node instead of the whole mesh sharing one
  // global emissiveIntensity. Mirrors the WebGPU TSL `uGlowStrength` uniform.
  const glslGlowStrengthRef = useRef({ value: 1 });
  // Per-mesh guard: the GLSL augment is applied once per InstancedMesh; a
  // recreated mesh (mode change / capacity growth) re-applies. Tracks the mesh
  // the augment was last bound to (the material's own userData guards re-entry).
  const glslGlowMeshRef = useRef<THREE.InstancedMesh | null>(null);
  // Task #54: per-instance transform texture (RGBA float = x, y, z, scale),
  // sampled on the GPU via instanceIndex by the material's positionNode. When
  // active (WebGPU only), the per-frame loop writes this texture's typed array
  // instead of composing a mat4 + setMatrixAt per instance on the CPU.
  const xformTexRef = useRef<THREE.DataTexture | null>(null);
  const gpuTransformRef = useRef(false);
  // Task #54: picking source-of-truth when the transform lives in the texture
  // (not instanceMatrix). The custom raycast (below) reads this — the SAME
  // (x, y, z, scale) the GPU samples — so node picking matches the rendered
  // position exactly. visibleCount mirrors inst.count.
  const xformBufRef = useRef<Float32Array | null>(null);
  const visibleCountRef = useRef(0);
  const geomRadiusRef = useRef(0.5);
  const prevMetaHashRef = useRef('');
  // Attention-heat re-upload throttle (task V1). The bucket advances at ~2Hz
  // while heat is live (driving the fade) and once more on settle; folding it
  // into metaHash makes a decaying-heat frame re-upload meta.w without a full
  // structural change. Off for the agent population (meta.w is status activity).
  const lastHeatUploadRef = useRef(0);
  const heatBucketRef = useRef(0);
  const heatWasActiveRef = useRef(false);
  // Agent momentum-nudge: per-instance visual offset accumulated frame-over-
  // frame. Working agents converge on their target; done agents drift outward.
  const agentOffsetsRef = useRef<Float32Array | null>(null);
  // Attention-heat → instanceColor (WebGL-visible per-instance heat). The base
  // (unheated) RGB is cached on each colour repaint so the 2Hz heat pass can
  // rescale it in place — brightening touched gems and letting them fade — without
  // re-running computeColor (HSL/hash) every tick. Keyed on the same heat bucket
  // as the metadata upload so both stay in lock-step.
  const baseColorCacheRef = useRef<Float32Array | null>(null);
  const prevHeatColorBucketRef = useRef(-1);
  // Track meshes manually added to the scene so we can remove them synchronously
  const sceneMeshesRef = useRef<Set<THREE.InstancedMesh>>(new Set());
  // forceMode pins the geometry/material to one population (multi-mesh path);
  // otherwise pick the majority population (legacy single-mesh path).
  const dominant = forceMode ?? getDominantMode(nodes, graphMode, perNodeVisualModeMap);

  // Keep a synchronously-updated ref to nodes so useFrame always reads
  // the latest filtered array, even if R3F's callback-ref update lags
  // behind a React re-render by one frame.
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;

  // Read gem material settings from the settings store for live tuning
  const gemSettings = useSettingsStore(s => s.get<GemMaterialSettings>('visualisation.gemMaterial'));
  const lastGemSettingsRef = useRef<string>('');

  // Quality gate toggles for cluster/anomaly/community coloring
  const qualityGates = useSettingsStore(s => s.get<QualityGatesSettings>('qualityGates'));
  // Base node color from the control panel ("Node Color" swatch) — anchors the
  // knowledge-graph palette when no semantic (cluster/community/anomaly/SSSP) mode is active.
  const baseNodeColor = useSettingsStore(s => s.get<string>('visualisation.graphs.knowledge.nodes.baseColor'));
  // KG colour scheme: 'type' (per node-type palette), 'domain' (per domain palette),
  // or 'base' (legacy baseColor + label-hash hue jitter). Default 'type'.
  const colorScheme = useSettingsStore(s => s.get<string>('visualisation.graphs.knowledge.nodes.colorScheme')) ?? 'type';
  // Per-node analytics data from binary protocol V3 (refreshed periodically).
  // Stride 5 (ADR-031 D2): [clusterId, anomalyScore, communityId, centrality, ssspDistance].
  const analyticsRef = useRef<Float32Array | null>(null);
  const analyticsFrameRef = useRef(0);
  // Per-refresh maxima for normalising the centrality / SSSP colour ramps.
  const maxCentralityRef = useRef(0);
  const maxSsspRef = useRef(0);

  // Dirty-gated scale + colour caches. computeNodeScale (sqrt/log/parseInt) and
  // computeColor (HSL/hashing) are expensive and their inputs (degree, hierarchy,
  // scheme, selection, analytics) do NOT change frame-to-frame — only node
  // POSITIONS do (physics). We recompute scale/colour only when their inputs
  // change so the per-frame loop just writes matrices instead of re-running both
  // functions and re-uploading the full colour buffer for every node every frame.
  const scaleCacheRef = useRef<Float32Array | null>(null);
  // posIdx (node.id -> position-buffer index) is invariant between data changes:
  // it only depends on currentNodes order + nodeIdToIndexMap, the SAME inputs that
  // gate the scale cache. Precompute it once so the per-frame loop skips ~5651
  // String(node.id) allocations + ~5651 Map.get() calls per frame (zero-alloc).
  const posIdxCacheRef = useRef<Int32Array | null>(null);
  const prevScaleHashRef = useRef('');
  const prevColorHashRef = useRef('');
  const analyticsVersionRef = useRef(0);
  // Tracks the mesh the caches were last written against. A recreated mesh
  // (dominant-mode change / capacity growth) starts with grey init colours, so
  // we must repaint regardless of whether the input hashes changed.
  const cachedMeshRef = useRef<THREE.InstancedMesh | null>(null);

  // Allocate instance buffer at next-power-of-2 of node count (minimum 4096).
  // Recreate when visual mode changes OR nodes exceed current capacity.
  const capacityRef = useRef(0);
  const neededCapacity = nextPowerOf2(Math.max(nodes.length, 4096));
  const capacityKey = neededCapacity > capacityRef.current ? neededCapacity : capacityRef.current;

  const { mesh, uniforms } = useMemo(() => {
    const count = nextPowerOf2(Math.max(nodes.length, 4096));
    capacityRef.current = count;
    const [geo, matResult] = dominant === 'ontology'
      ? [createCrystalOrbGeometry(), createCrystalOrbMaterial()] as const
      : dominant === 'agent'
        ? [createAgentCapsuleGeometry(), createAgentCapsuleMaterial()] as const
        : [createGemGeometry(), createGemNodeMaterial()] as const;

    const inst = new THREE.InstancedMesh(geo, matResult.material, count);
    inst.frustumCulled = false;
    inst.count = 0; // Start invisible -- useFrame sets actual count
    const id = new THREE.Matrix4();
    for (let i = 0; i < count; i++) {
      inst.setMatrixAt(i, id);
      inst.setColorAt(i, _col.set(0.5, 0.5, 0.5));
    }
    inst.instanceMatrix.needsUpdate = true;
    if (inst.instanceColor) inst.instanceColor.needsUpdate = true;

    // Geometry bounding radius — picking radius is this * per-instance scale.
    geo.computeBoundingSphere();
    geomRadiusRef.current = geo.boundingSphere?.radius ?? 0.5;

    // Task #54: custom raycast. With the GPU transform active, instanceMatrix is
    // identity (all instances at origin), so the default InstancedMesh raycast
    // would mis-pick. This sphere-test reads the live transform texture buffer
    // (the same x/y/z/scale the GPU renders), preserving node picking + drag.
    // Off WebGPU (gpuTransformRef false) it defers to Three's default raycast.
    inst.raycast = function (raycaster: THREE.Raycaster, intersects: THREE.Intersection[]): void {
      if (!gpuTransformRef.current) {
        THREE.InstancedMesh.prototype.raycast.call(this, raycaster, intersects);
        return;
      }
      const buf = xformBufRef.current;
      const n = Math.min(visibleCountRef.current, this.count);
      if (!buf || n === 0) return;
      const baseR = geomRadiusRef.current;
      for (let i = 0; i < n; i++) {
        const t4 = i * 4;
        _rayCenter.set(buf[t4], buf[t4 + 1], buf[t4 + 2]);
        _raySphere.center.copy(_rayCenter);
        _raySphere.radius = baseR * buf[t4 + 3];
        if (raycaster.ray.intersectSphere(_raySphere, _rayHit)) {
          const distance = raycaster.ray.origin.distanceTo(_rayHit);
          if (distance < raycaster.near || distance > raycaster.far) continue;
          intersects.push({
            distance,
            point: _rayHit.clone(),
            instanceId: i,
            object: this,
          });
        }
      }
      intersects.sort((a, b) => a.distance - b.distance);
    };

    // Per-instance metadata texture for TSL (RGBA float: quality, authority,
    // connections, recency). Laid out row-major across a 2D grid so the width
    // stays within the WebGPU max texture dimension (8192). Instance i maps to
    // texel (i % W, i / W); the linear buffer offset is still i*4, so the
    // per-instance write path in useFrame is unchanged.
    const metaTexW = Math.min(count, METADATA_TEX_WIDTH);
    const metaTexH = Math.ceil(count / metaTexW);
    const texData = new Float32Array(metaTexW * metaTexH * 4);
    const metaTex = new THREE.DataTexture(texData, metaTexW, metaTexH, THREE.RGBAFormat, THREE.FloatType);
    metaTex.minFilter = THREE.NearestFilter;
    metaTex.magFilter = THREE.NearestFilter;
    metaTex.needsUpdate = true;
    metaTexRef.current = metaTex;
    // Stash on the mesh for commit-time ref reconciliation (see
    // handleCommittedMesh): ref side effects in this useMemo can leak from
    // renders React discards, so the committed mesh is the source of truth.
    inst.userData.metaTex = metaTex;

    // WebGL parity (task #50): the GLSL emissive injection (GlslMetadataGlow)
    // stays GLSL ES 1.00 and cannot vertex-sample the metadata DataTexture, so it
    // reads per-instance metadata from InstancedBufferAttributes instead. aGlowMeta
    // SHARES the same texData buffer (instance i at i*4 — identical layout to the
    // texture), so the per-frame write site updates both at once. aInstanceIndex is
    // a static phase seed. WebGPU skips these: its TSL path samples metaTex by
    // instanceIndex, and an InstancedBufferAttribute there crashes the backend
    // (drawIndexed(Infinity)) — hence the WebGL-only guard.
    if (!isWebGPURenderer) {
      const metaAttr = new THREE.InstancedBufferAttribute(texData, 4);
      metaAttr.setUsage(THREE.DynamicDrawUsage);
      geo.setAttribute('aGlowMeta', metaAttr);
      const idxArr = new Float32Array(metaTexW * metaTexH);
      for (let i = 0; i < idxArr.length; i++) idxArr[i] = i;
      geo.setAttribute('aInstanceIndex', new THREE.InstancedBufferAttribute(idxArr, 1));

      // Apply the GLSL metadata-glow augment synchronously to THIS freshly-created
      // material (mirrors the WebGPU TSL augment). Doing it here — not in a
      // post-mount effect — guarantees every recreated mesh (capacity growth /
      // dominant-mode change) gets the per-node emissive: an effect races mesh
      // recreation and left the active mesh on the flat fallback path.
      const preset = dominant === 'ontology' ? GLSL_GLOW_PRESETS.orb
        : dominant === 'agent' ? GLSL_GLOW_PRESETS.agent
        : GLSL_GLOW_PRESETS.gem;
      const applied = applyGlslMetadataGlow(
        matResult.material as THREE.MeshStandardMaterial,
        matResult.uniforms.time as { value: number },
        glslGlowStrengthRef.current,
        preset,
      );
      if (applied) glslGlowMeshRef.current = inst;
    }

    // Per-instance transform texture (same row-major grid as the metadata
    // texture). Texel i = (x, y, z, scale). Default scale 1 so an instance is
    // never invisible before its first physics write. Reset the active flag —
    // the augment is (re)applied per mesh in the effect below. Dispose the
    // texture from a prior mesh generation (mode change / capacity growth).
    xformTexRef.current?.dispose();
    const xformData = new Float32Array(metaTexW * metaTexH * 4);
    for (let i = 3; i < xformData.length; i += 4) xformData[i] = 1; // scale = 1
    const xformTex = new THREE.DataTexture(xformData, metaTexW, metaTexH, THREE.RGBAFormat, THREE.FloatType);
    xformTex.minFilter = THREE.NearestFilter;
    xformTex.magFilter = THREE.NearestFilter;
    xformTex.needsUpdate = true;
    xformTexRef.current = xformTex;
    // The texture's lifetime is the MESH's lifetime, not the effect lifecycle:
    // StrictMode's simulated unmount runs the cleanup below (disposing the
    // texture + nulling the ref) but useMemo — and this mesh — survive the
    // remount. Stash the texture on the mesh so the remount effect can re-sync
    // the ref; otherwise the per-frame loop feeds the raycast a null picking
    // buffer forever and every pointer interaction (click/drag/double-click)
    // silently dies while rendering keeps showing the stale GPU texture.
    inst.userData.xformTex = xformTex;
    gpuTransformRef.current = false;

    // WebGPU parity: apply the TSL emissive + GPU-transform augments to THIS
    // freshly-created material HERE, not in a post-mount effect. React double-
    // renders this component (dev), creating two meshes; the effects bound to the
    // first (stale) mesh while the committed live mesh kept the flat-emissive +
    // CPU-instanceMatrix fallback — silently off the GPU-resident path. useMemo
    // runs for the committed mesh, so applying here reaches the live one (this is
    // exactly what fixed the WebGL aGlowMeta path above). The async augments are
    // fire-and-forget and mutate only this material; transform flips gpuTransform
    // on only if this mesh is still the live one when it resolves.
    if (isWebGPURenderer) {
      const emissiveAugment = dominant === 'ontology' ? createTslCrystalOrbMaterial
        : dominant === 'agent' ? createTslAgentCapsuleMaterial
        : createTslGemMaterial;
      void emissiveAugment(matResult.material as THREE.MeshStandardMaterial, metaTex, metaTexW, metaTexH);
      void applyTslInstanceTransform(matResult.material as THREE.MeshStandardMaterial, xformTex, metaTexW, metaTexH)
        .then(ok => { if (ok && meshRef.current === inst) gpuTransformRef.current = true; });
    }

    meshRef.current = inst;
    return { mesh: inst, uniforms: matResult.uniforms };
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, [dominant, capacityKey]);

  // Dispose previous GPU resources on unmount only.
  // Stale mesh cleanup on mode change is handled synchronously in useFrame
  // (via sceneMeshesRef) to avoid async useEffect timing gaps during rapid toggles.
  useEffect(() => {
    // StrictMode remount recovery: the simulated unmount's cleanup disposed the
    // transform texture and nulled the ref, but the useMemo mesh (and the TSL
    // material sampling the texture) survived. Re-sync the ref from the mesh and
    // re-upload so the GPU resource is re-initialised and the picking buffer
    // (xformBufRef, fed from this texture each frame) comes back to life.
    const tex = meshRef.current?.userData.xformTex as THREE.DataTexture | undefined;
    if (tex && xformTexRef.current !== tex) {
      xformTexRef.current = tex;
      tex.needsUpdate = true;
    }
    return () => {
      for (const old of sceneMeshesRef.current) {
        if (old.parent) old.parent.remove(old);
        old.geometry?.dispose();
        (old.material as THREE.Material)?.dispose();
        old.dispose();
      }
      sceneMeshesRef.current.clear();
      // Task #54: release the GPU transform texture on unmount.
      xformTexRef.current?.dispose();
      xformTexRef.current = null;
    };
  }, []);

  useImperativeHandle(ref, () => ({
    getMesh: () => meshRef.current,
    getColorArray: () => meshRef.current?.instanceColor?.array as Float32Array | null ?? null,
  }), [mesh]);

  // Per-node metadata-driven emissive + GPU instance transform (WebGPU) and the
  // GLSL metadata-glow (WebGL) are BOTH applied synchronously inside the mesh
  // useMemo above, against the freshly-created material. They are NOT wired from
  // post-mount effects: React double-renders this component (dev), so useMemo
  // runs twice and creates two meshes, but an effect binds to only one of them —
  // it raced mesh recreation and left the committed live mesh on the flat PBR /
  // CPU-instanceMatrix fallback (the stale mesh kept the augment). useMemo runs
  // for the committed mesh, so applying there reaches the live one on every
  // recreation (capacity growth / dominant-mode change). Nothing to do here.

  const computeColor = useCallback((node: GraphNode, mode: GraphVisualMode, nodeIndex?: number): THREE.Color => {
    // ADR-031 D6: `colorScheme` is the single "node colour by" selector. The
    // analytic modes (community/cluster/centrality/sssp) colour from the live V3
    // analytics buffer; each FALLS THROUGH to the standard type/domain/base
    // scheme below when the node has no value for that signal, so unclustered /
    // no-data nodes stay visible rather than collapsing to one dark colour.
    // Anomaly is the one exception — an always-on red overlay (gated by
    // qualityGates.showAnomalies) that wins over whatever scheme is active.
    const analytics = analyticsRef.current;
    if (analytics && nodeIndex !== undefined && nodeIndex * ANALYTICS_STRIDE + ANALYTICS_SSSP_OFFSET < analytics.length) {
      const a = nodeIndex * ANALYTICS_STRIDE;
      const clusterId = analytics[a + ANALYTICS_CLUSTER_OFFSET];
      const anomalyScore = analytics[a + ANALYTICS_ANOMALY_OFFSET];
      const communityId = analytics[a + ANALYTICS_COMMUNITY_OFFSET];
      const centrality = analytics[a + ANALYTICS_CENTRALITY_OFFSET];
      const ssspDistance = analytics[a + ANALYTICS_SSSP_OFFSET];

      // Anomaly highlight overlay: red intensity proportional to anomalyScore.
      if (qualityGates?.showAnomalies && anomalyScore > 0.01) {
        const intensity = Math.min(anomalyScore, 1.0);
        return _col.setRGB(0.9 * intensity + 0.1, 0.15 * (1 - intensity), 0.1);
      }

      // Community (Louvain, wire offset 44): deterministic hue per community.
      if (colorScheme === 'community' && communityId > 0) {
        const hue = ((communityId * 83) % 360) / 360;
        return _col.setHSL(hue, 0.65, 0.5);
      }

      // Cluster (DBSCAN/k-means, wire offset 36): golden-angle hue per cluster.
      if (colorScheme === 'cluster' && clusterId > 0) {
        const hue = ((clusterId * 137) % 360) / 360;
        return _col.setHSL(hue, 0.7, 0.55);
      }

      // Centrality (PageRank, normalised, wire offset 48). Scaled by the live
      // maximum so the (tiny, sum-to-1) PageRank values span the full ramp.
      if (colorScheme === 'centrality' && centrality > 0) {
        const max = maxCentralityRef.current || 1;
        const t = Math.min(centrality / max, 1.0);
        return _col.setHSL(0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t); // blue→cyan→yellow ramp
      }

      // SSSP distance from the active source (wire offset 28). Only paints once a
      // run exists (maxSssp > 0) and no explicit on-demand ssspResult overrides.
      if (colorScheme === 'sssp' && !ssspResult && maxSsspRef.current > 0) {
        if (!Number.isFinite(ssspDistance)) return _col.set('#444444'); // unreachable
        const max = maxSsspRef.current || 1;
        const t = Math.min(ssspDistance / max, 1.0);
        return _col.setRGB(Math.min(1, t * 1.2), Math.min(1, (1 - t) * 1.2), 0.1);
      }
    }

    if (ssspResult) {
      const d = ssspResult.distances?.[node.id] ?? NaN;
      if (String(node.id) === ssspResult.sourceNodeId) return _col.set('#00FFFF');
      if (!isFinite(d)) return _col.set('#666666');
      const nd = ssspResult.normalizedDistances?.[node.id] || 0;
      return _col.setRGB(Math.min(1, nd * 1.2), Math.min(1, (1 - nd) * 1.2), 0.1);
    }
    if (mode === 'agent') {
      return _col.set(AGENT_STATUS_MAP[node.metadata?.status?.toLowerCase() || 'active'] || '#2ECC71');
    }
    if (mode === 'ontology') {
      // Prefer real hierarchy depth when present (additive/safe legacy path).
      const depth = hierarchyMap?.get(node.id)?.depth ?? (node.metadata?.depth ?? 0);
      if (depth > 0) {
        return _col.set(ONTOLOGY_SPECTRUM[Math.min(depth, ONTOLOGY_SPECTRUM.length - 1)]);
      }
      // No depth in this dataset: differentiate by ontology CLASS. Stable hue
      // from class_iri (→ page_iri → id) so each of the ~4262 classes is
      // distinct and consistent across frames. owl_class (schema) reads
      // slightly lighter/more saturated than ontology_node (instances).
      const key = node.metadata?.class_iri ?? node.metadata?.page_iri ?? String(node.id);
      const hue = hashHue(key);
      const isClass = node.metadata?.type === 'owl_class';
      _col.setHSL(hue, isClass ? 0.72 : 0.62, isClass ? 0.58 : 0.5);
      return _col;
    }
    // Knowledge graph: hue is driven by the active colour scheme (type/domain),
    // then modulated by authority + connection count so high-degree nodes pop.
    const auth = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
    const cc = connectionCountMap.get(String(node.id)) || 0;

    if (colorScheme === 'type') {
      // Hue from per-type palette; small authority/connection lightness+saturation lift.
      const nodeType = node.metadata?.type ?? node.metadata?.nodeType;
      _col.copy(getTypeColor(nodeType)).getHSL(_baseHSL);
      const sat = _baseHSL.s + auth * 0.15 + Math.min(cc / 30, 0.1);
      const lit = _baseHSL.l + auth * 0.12 + Math.min(cc / 40, 0.06);
      _col.setHSL(_baseHSL.h, Math.min(sat, 0.95), Math.min(lit, 0.8));
      return _col;
    }

    if (colorScheme === 'domain') {
      // Hue from per-domain palette; same gentle authority/connection modulation.
      const domain = node.metadata?.domain ?? node.metadata?.source_domain;
      _col.set(getDomainColor(domain)).getHSL(_baseHSL);
      const sat = _baseHSL.s + auth * 0.15 + Math.min(cc / 30, 0.1);
      const lit = _baseHSL.l + auth * 0.12 + Math.min(cc / 40, 0.06);
      _col.setHSL(_baseHSL.h, Math.min(sat, 0.95), Math.min(lit, 0.8));
      return _col;
    }

    // 'base': anchor on the user's "Node Color" swatch, then add per-node
    // variation (hue jitter from label hash) so nodes stay distinguishable.
    if (baseNodeColor) {
      _col.set(baseNodeColor).getHSL(_baseHSL);
      const jitter = (hashHue(node.label || String(node.id)) - 0.5) * 0.12; // ±0.06 hue spread
      const hue = (_baseHSL.h + jitter + 1) % 1;
      const sat = _baseHSL.s + auth * 0.25 + Math.min(cc / 25, 0.15);
      const lit = _baseHSL.l + auth * 0.15;
      _col.setHSL(hue, Math.min(sat, 0.95), Math.min(lit, 0.8));
      return _col;
    }
    const hue = hashHue(node.label || String(node.id));
    const sat = 0.35 + auth * 0.35 + Math.min(cc / 20, 0.2);
    const lit = 0.45 + auth * 0.2;
    _col.setHSL(hue, Math.min(sat, 0.9), Math.min(lit, 0.75));
    return _col;
  }, [ssspResult, hierarchyMap, connectionCountMap, qualityGates, baseNodeColor, colorScheme]);

  // Progressive reveal: ramp up visible instance count over frames so nodes
  // appear in waves (~120 nodes/frame at 60fps → full 1090 in ~0.15s).
  const revealedRef = useRef(0);
  const prevNodeCountRef = useRef(0);
  const REVEAL_BATCH = 120;

  const diagLoggedRef = useRef(false);
  const frameCountRef = useRef(0);
  // Priority -1: run after GraphManager (-2) populates nodePositionsRef,
  // but before InstancedLabels (0) reads positions for label placement.
  useFrame(({ clock, camera, scene }) => {
    const inst = meshRef.current;
    // Read from ref to guarantee we have the latest filtered array,
    // bypassing any stale-closure risk in R3F's callback-ref pipeline.
    const currentNodes = nodesRef.current;
    if (!inst || currentNodes.length === 0) {
      // Ensure mesh is invisible when all nodes are filtered out
      if (inst) inst.count = 0;
      return;
    }

    // Clean up stale meshes from previous dominant-mode changes.
    // Must run every frame (not just when !inst.parent) because R3F's primitive
    // may attach the new mesh before useFrame runs, skipping the old cleanup path.
    for (const old of sceneMeshesRef.current) {
      if (old !== inst && old.parent) {
        old.parent.remove(old);
        old.geometry?.dispose();
        (old.material as THREE.Material)?.dispose();
        old.dispose();
        sceneMeshesRef.current.delete(old);
      }
    }

    // R3F <primitive> sometimes fails to attach InstancedMesh to scene.
    // Manually add it if needed.
    if (!inst.parent && scene) {
      scene.add(inst);
      sceneMeshesRef.current.add(inst);
    }

    if (uniforms.time) uniforms.time.value = clock.elapsedTime;

    // Track node count changes. Only reset reveal to 0 on fresh data load
    // (transitioning from 0 nodes). For incremental changes (filter toggles,
    // websocket updates), just clamp to avoid all-nodes-vanish flicker.
    if (currentNodes.length !== prevNodeCountRef.current) {
      if (prevNodeCountRef.current === 0) {
        revealedRef.current = 0; // Fresh load: wave-in from zero
      } else {
        revealedRef.current = Math.min(revealedRef.current, currentNodes.length);
      }
      prevNodeCountRef.current = currentNodes.length;
    }

    const positions = nodePositionsRef.current;
    frameCountRef.current++;

    // Refresh per-node analytics from the main-thread store every ~30 frames
    // (~0.5s at 60fps). The store is fed by V3 binary frames (cluster_id at
    // wire offset 36) and returns a render-index-aligned buffer (stride 5).
    // ADR-031 D6: an analytic colorScheme (community/cluster/centrality/sssp)
    // also consumes the buffer, so refresh for it even when the gates are off.
    analyticsFrameRef.current++;
    const analyticColorScheme =
      colorScheme === 'community' || colorScheme === 'cluster' ||
      colorScheme === 'centrality' || colorScheme === 'sssp';
    if (analyticsFrameRef.current % 30 === 1 &&
        (analyticColorScheme || qualityGates?.showClusters || qualityGates?.showAnomalies ||
         qualityGates?.showCommunities || qualityGates?.showCentrality || qualityGates?.showSSSP)) {
      const buf = nodeAnalyticsStore.getIndexedBuffer(props.nodeIdToIndexMap);
      if (buf) {
        analyticsRef.current = buf;
        // Compute maxima for the centrality / SSSP colour ramps (cheap stride scan).
        let maxC = 0, maxS = 0;
        for (let b = 0; b < buf.length; b += ANALYTICS_STRIDE) {
          const c = buf[b + ANALYTICS_CENTRALITY_OFFSET];
          if (c > maxC) maxC = c;
          const s = buf[b + ANALYTICS_SSSP_OFFSET];
          if (Number.isFinite(s) && s > maxS) maxS = s;
        }
        maxCentralityRef.current = maxC;
        maxSsspRef.current = maxS;
        analyticsVersionRef.current++; // invalidate colour cache on fresh analytics
      }
    }

    // Delayed diagnostic — fires at frame 60 when positions are loaded (dev only).
    // Phase 6 (ADR-04 D10): one-shot diagnostic, guarded by diagLoggedRef so
    // the allocations below execute exactly once per session. Disable the
    // zero-alloc lint rule for this block — the alternative (module-scope
    // temps just for a single diagnostic) is uglier than the deliberate
    // exception.
    if (import.meta.env.DEV) {
      if (!diagLoggedRef.current && frameCountRef.current >= 60) {
        diagLoggedRef.current = true;
        const mat = inst.material as THREE.MeshPhysicalMaterial & { opacityNode?: unknown; emissiveNode?: unknown; colorNode?: unknown };
        // Sample first 3 instance matrices
        // eslint-disable-next-line no-restricted-syntax
        const tempMat = new THREE.Matrix4();
        // eslint-disable-next-line no-restricted-syntax
        const tempVec = new THREE.Vector3();
        // eslint-disable-next-line no-restricted-syntax
        const tempScale = new THREE.Vector3();
        const matSamples: Array<{ i: number; pos: { x: number; y: number; z: number }; scale: number }> = [];
        for (let si = 0; si < Math.min(3, inst.count); si++) {
          inst.getMatrixAt(si, tempMat);
          tempVec.setFromMatrixPosition(tempMat);
          tempScale.setFromMatrixScale(tempMat);
          matSamples.push({ i: si, pos: { x: +tempVec.x.toFixed(1), y: +tempVec.y.toFixed(1), z: +tempVec.z.toFixed(1) }, scale: +tempScale.x.toFixed(2) });
        }
        // Compute bounding box from first 20 instances
        // eslint-disable-next-line no-restricted-syntax
        const bbox = new THREE.Box3();
        for (let bi = 0; bi < Math.min(20, inst.count); bi++) {
          inst.getMatrixAt(bi, tempMat);
          tempVec.setFromMatrixPosition(tempMat);
          bbox.expandByPoint(tempVec);
        }
        // eslint-disable-next-line no-restricted-syntax
        const bboxSize = new THREE.Vector3();
        bbox.getSize(bboxSize);
        logger.debug('[GemNodes] DIAG frame60:', {
          nodeCount: currentNodes.length,
          instCount: inst.count,
          hasPositions: !!positions,
          posLen: positions?.length ?? 0,
          visible: inst.visible,
          hasParent: !!inst.parent,
          parentType: inst.parent?.type,
          frustumCulled: inst.frustumCulled,
          matType: mat?.type,
          matTransmission: mat?.transmission,
          matOpacity: mat?.opacity,
          matTransparent: mat?.transparent,
          matDepthWrite: mat?.depthWrite,
          matSide: mat?.side,
          hasOpacityNode: !!mat?.opacityNode,
          hasEmissiveNode: !!mat?.emissiveNode,
          hasColorNode: !!mat?.colorNode,
          // Task #54: when true, instanceMatrix is intentionally identity and
          // matSamples below read identity — the live transform lives in the
          // transform texture / positionNode, not the matrix buffer.
          gpuTransform: gpuTransformRef.current,
          matSamples,
          bboxSize: { x: +bboxSize.x.toFixed(1), y: +bboxSize.y.toFixed(1), z: +bboxSize.z.toFixed(1) },
          cameraPos: { x: +camera.position.x.toFixed(1), y: +camera.position.y.toFixed(1), z: +camera.position.z.toFixed(1) },
          dominant,
        });
      }
    }
    const visSettings = settings?.visualisation as Record<string, unknown> | undefined;
    const graphsKnowledge = (visSettings?.graphs as Record<string, unknown> | undefined)?.knowledge as Record<string, unknown> | undefined;
    const nodeSettings = graphsKnowledge?.nodes as Record<string, unknown> | undefined;
    // 1:1 — the slider value IS the global size gain; no hidden multiplier.
    // Per-node magnitude comes from computeNodeScale (degree + content size).
    const baseScale = (nodeSettings?.nodeSize as number | undefined) ?? 1.0;
    const texBuf = metaTexRef.current?.image?.data as Float32Array | undefined;

    // Read animation settings for pulse/wave control
    const anims = visSettings?.animations as Record<string, unknown> | undefined;
    const animEnabled = (anims?.enableNodeAnimations as boolean | undefined) ?? true;
    const pEnabled = animEnabled && ((anims?.pulseEnabled as boolean | undefined) ?? true);
    const pSpeed = (anims?.pulseSpeed as number | undefined) ?? 1.2;
    const pStrength = (anims?.pulseStrength as number | undefined) ?? 0.8;

    // Per-frame emissive modulation (replaces TSL which breaks InstancedMesh on WebGPU).
    // Gentle breathing pulse on the shared material — all instances share it but
    // per-instance color variation comes from instanceColor.
    const currentMat = inst.material as THREE.MeshPhysicalMaterial;
    if (currentMat.emissiveIntensity !== undefined) {
      const u = uniforms as Record<string, { value: number }>;
      // Read glow intensity from settings; fall back to conservative default
      const vis = settings?.visualisation as Record<string, unknown> | undefined;
      const glow = vis?.glow as Record<string, unknown> | undefined;
      const glowBase = (glow?.intensity as number | undefined) ?? 0.3;

      // Read per-type visual settings — no hardcoded multipliers
      const typeVisuals = (vis as any)?.graphTypeVisuals;
      const agentVis = typeVisuals?.agent;
      const kgVis = typeVisuals?.knowledgeGraph;
      const ontoVis = typeVisuals?.ontology;

      // Per-type glow strength from settings (defaults from DEFAULT_GRAPH_TYPE_VISUALS)
      const typeGlowStrength =
        dominant === 'agent' ? (agentVis?.bioluminescentIntensity ?? 0.6) :
        dominant === 'ontology' ? (ontoVis?.glowStrength ?? 1.8) :
        (kgVis?.glowStrength ?? 2.5);
      const agentBaseEmissive = agentVis?.nucleusGlowIntensity ?? 0.6;
      const breathingSpeed = agentVis?.breathingSpeed ?? 1.5;
      const breathingAmplitude = agentVis?.breathingAmplitude ?? 0.4;
      const kgInnerGlow = kgVis?.innerGlowIntensity ?? 0.3;

      // Task #50 (WebGL parity): when the GLSL per-instance glow owns this mesh,
      // the injected shader breathes per-node (authority→pulse) from the metadata
      // texture. Feed it the per-type glow scale via the live uGlowStrength uniform
      // and hold the base emissiveIntensity at a low stable floor so the per-node
      // term dominates — matching the WebGPU TSL emissiveNode. NO global breathing
      // here (that was the uniform-breathing parity gap this replaces).
      if (glslGlowMeshRef.current === inst) {
        glslGlowStrengthRef.current.value = glowBase * typeGlowStrength;
        currentMat.emissiveIntensity = glowBase * kgInnerGlow;
      } else if (!pEnabled) {
        // Pulse disabled — static emissive from type-specific settings
        currentMat.emissiveIntensity = dominant === 'agent'
          ? agentBaseEmissive * glowBase
          : glowBase * kgInnerGlow * typeGlowStrength;
      } else if (dominant === 'agent' && u.activityLevel) {
        const pulse = Math.pow((Math.sin(clock.elapsedTime * pSpeed * Math.PI) + 1) * 0.5, 4);
        currentMat.emissiveIntensity = agentBaseEmissive * glowBase + pulse * u.activityLevel.value * breathingAmplitude * pStrength;
      } else {
        // Knowledge graph / ontology: breathing emissive driven by type and glow settings
        const breathDamping = dominant === 'ontology' ? (ontoVis?.nebulaGlowIntensity ?? 0.7) : kgInnerGlow;
        const breath = (Math.sin(clock.elapsedTime * pSpeed * breathingSpeed) + 1) * 0.5;
        currentMat.emissiveIntensity = glowBase * breathDamping * typeGlowStrength
          + breath * glowBase * breathingAmplitude * pStrength;
      }
    }

    // Apply gem material settings from settings store — only when values actually change
    // to avoid forcing shader recompilation every frame via needsUpdate.
    const settingsKey = gemSettings ? JSON.stringify(gemSettings) : '';
    if (settingsKey !== lastGemSettingsRef.current && gemSettings && currentMat instanceof THREE.MeshPhysicalMaterial) {
      lastGemSettingsRef.current = settingsKey;
      if (gemSettings.ior !== undefined) currentMat.ior = gemSettings.ior;
      if (gemSettings.transmission !== undefined) currentMat.transmission = gemSettings.transmission;
      if (gemSettings.clearcoat !== undefined) currentMat.clearcoat = gemSettings.clearcoat;
      if (gemSettings.clearcoatRoughness !== undefined) currentMat.clearcoatRoughness = gemSettings.clearcoatRoughness;
      if (gemSettings.emissiveIntensity !== undefined) currentMat.emissiveIntensity = gemSettings.emissiveIntensity;
      if (gemSettings.iridescence !== undefined) currentMat.iridescence = gemSettings.iridescence;
      currentMat.needsUpdate = true;
    }

    // Progressive reveal: ramp up visible count each frame; clamp when nodes shrink
    if (revealedRef.current > currentNodes.length) {
      revealedRef.current = currentNodes.length;
    } else if (revealedRef.current < currentNodes.length) {
      revealedRef.current = Math.min(revealedRef.current + REVEAL_BATCH, currentNodes.length);
    }
    const visCount = revealedRef.current;
    const nodeCount = currentNodes.length;

    // A recreated mesh has grey init colours — force a repaint of both caches.
    if (cachedMeshRef.current !== inst) {
      cachedMeshRef.current = inst;
      prevScaleHashRef.current = '';
      prevColorHashRef.current = '';
    }

    // --- Scale + posIdx caches: recompute only when affecting inputs change --
    const cacheLen = capacityRef.current || nextPowerOf2(Math.max(nodeCount, 4096));
    // ADR-04 D10 compliance: these are GROW-ONLY caches, not per-frame
    // allocations. The length guard means a frame allocates only when the node
    // count outgrows the current buffer, and cacheLen is a power-of-2 capacity,
    // so growth is amortised O(1) and stops entirely once the graph settles.
    // They cannot be hoisted to module scope: the size is data-dependent and
    // the buffers are per-instance. Steady-state frames allocate nothing.
    if (!scaleCacheRef.current || scaleCacheRef.current.length < nodeCount) {
      // eslint-disable-next-line no-restricted-syntax -- grow-only cache, guarded by the length check above
      scaleCacheRef.current = new Float32Array(cacheLen);
    }
    if (!posIdxCacheRef.current || posIdxCacheRef.current.length < nodeCount) {
      // eslint-disable-next-line no-restricted-syntax -- grow-only cache, guarded by the length check above
      posIdxCacheRef.current = new Int32Array(cacheLen);
    }
    const scaleCache = scaleCacheRef.current;
    const posIdxCache = posIdxCacheRef.current;
    const propsVis = props.settings?.visualisation as Record<string, unknown> | undefined;
    const graphTypeVisuals = propsVis?.graphTypeVisuals as GraphTypeVisualsSettings | undefined;
    // Attention-heat gate, hoisted so BOTH the colour repaint below and the
    // metadata upload later read the same value. KG + ontology gems brighten with
    // heat; the agent population keeps meta.w = status activity, so heat never
    // touches its colour.
    const isAgent = dominant === 'agent';
    const kgVisuals = graphTypeVisuals?.knowledgeGraph;
    const heatEnabled = !isAgent && (kgVisuals?.attentionHeatEnabled ?? true);
    const scaleHash = `${nodeCount}-${connectionCountMap.size}-${baseScale}-${hierarchyMap?.size ?? 0}`;
    if (scaleHash !== prevScaleHashRef.current) {
      prevScaleHashRef.current = scaleHash;
      for (let i = 0; i < nodeCount; i++) {
        const node = currentNodes[i];
        const nodeIdStr = String(node.id);
        const mode = perNodeVisualModeMap.get(nodeIdStr) || graphMode;
        scaleCache[i] = computeNodeScale(node, connectionCountMap, mode, hierarchyMap, graphTypeVisuals) * baseScale;
        const srcIdx = props.nodeIdToIndexMap.get(nodeIdStr);
        posIdxCache[i] = srcIdx !== undefined ? srcIdx : i;
      }
    }

    // --- Colour cache: recompute + upload only when colour inputs change -----
    const colorHash = `${nodeCount}-${connectionCountMap.size}-${colorScheme}-${baseNodeColor ?? ''}-${selectedNodeId ?? ''}-${ssspResult ? 's' : ''}-${qualityGates?.showClusters ? 1 : 0}${qualityGates?.showAnomalies ? 1 : 0}${qualityGates?.showCommunities ? 1 : 0}${qualityGates?.showCentrality ? 1 : 0}${qualityGates?.showSSSP ? 1 : 0}-${analyticsVersionRef.current}-${heatEnabled ? 1 : 0}`;
    if (colorHash !== prevColorHashRef.current) {
      prevColorHashRef.current = colorHash;
      // Cache the UNHEATED base RGB so the 2Hz heat pass can rescale without
      // recomputing computeColor. Sized to the colour capacity (stride 3).
      let baseCache = baseColorCacheRef.current;
      if (!baseCache || baseCache.length < nodeCount * 3) {
        // eslint-disable-next-line no-restricted-syntax -- grow-only cache, guarded by the length check above
        baseCache = new Float32Array(cacheLen * 3);
        baseColorCacheRef.current = baseCache;
      }
      for (let i = 0; i < nodeCount; i++) {
        const node = currentNodes[i];
        const mode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
        const srcIdx = props.nodeIdToIndexMap.get(String(node.id));
        const c = computeColor(node, mode, srcIdx !== undefined ? srcIdx : i);
        const b3 = i * 3;
        baseCache[b3] = c.r; baseCache[b3 + 1] = c.g; baseCache[b3 + 2] = c.b;
        // Bake current heat into the first paint so a colour-scheme change mid-
        // stream never flashes an unheated frame before the heat pass catches up.
        if (heatEnabled) {
          const f = heatBrightenFactor(c.r, c.g, c.b, attentionHeat.getHeat(String(node.id)));
          c.setRGB(c.r * f, c.g * f, c.b * f);
        }
        inst.setColorAt(i, c);
      }
      if (inst.instanceColor) inst.instanceColor.needsUpdate = true;
    }

    // --- Per-frame matrix write: positions change every frame (physics). -----
    // Scale + posIdx come from the gated caches; the per-node loop touches
    // positions only. Selected-node wave and live-drag are single-node concerns,
    // resolved to local indices ONCE here (one Map lookup each) so the loop does
    // integer comparisons instead of ~5651 String(node.id) allocations per frame.
    const waveEnabled = animEnabled && ((anims?.selectionWaveEnabled as boolean | undefined) ?? true);
    const wSpeed = (anims?.waveSpeed as number | undefined) ?? 1.0;
    const drag = props.dragDataRef?.current;
    // Local (visibleNodes) index of the selected node, for the wave modulation.
    const selectedLocalIdx = (selectedNodeId && waveEnabled)
      ? currentNodes.findIndex(n => String(n.id) === selectedNodeId)
      : -1;
    const waveScale = selectedLocalIdx >= 0
      ? 1 + Math.sin(clock.elapsedTime * 3 * wSpeed) * 0.15
      : 1;
    // Local index of the dragged node (live position overrides the SAB read).
    const dragLocalIdx = (drag && drag.isDragging && drag.nodeId)
      ? currentNodes.findIndex(n => String(n.id) === drag.nodeId)
      : -1;
    // Task #54: when the GPU transform is active (WebGPU), the loop writes the
    // transform texture's typed array (x, y, z, scale) and the positionNode
    // composes the vertex on-GPU. instanceMatrix stays identity (set once at
    // mesh creation). Off WebGPU we keep the CPU mat4 path (setMatrixAt). Both
    // paths read the SAME position/scale/wave/drag inputs so the visual output
    // (sizing scheme, selection wave, live-drag) is identical.
    const gpuXform = gpuTransformRef.current;
    const xformBuf = gpuXform ? (xformTexRef.current?.image?.data as Float32Array | undefined) : undefined;
    // Keep the picking refs current (raycast reads them when GPU transform on).
    xformBufRef.current = xformBuf ?? null;
    visibleCountRef.current = visCount;
    // --- Agent momentum-nudge: graph centre + offset buffer setup ---
    let graphCX = 0, graphCY = 0, graphCZ = 0;
    if (isAgent && positions) {
      let sn = 0;
      for (let s = 0; s < positions.length - 2; s += 150) {
        graphCX += positions[s]; graphCY += positions[s + 1]; graphCZ += positions[s + 2];
        sn++;
      }
      if (sn > 0) { graphCX /= sn; graphCY /= sn; graphCZ /= sn; }
      if (!agentOffsetsRef.current || agentOffsetsRef.current.length < visCount * 3) {
        const old = agentOffsetsRef.current;
        // eslint-disable-next-line no-restricted-syntax -- grow-only cache
        agentOffsetsRef.current = new Float32Array(cacheLen * 3);
        if (old) agentOffsetsRef.current.set(old.subarray(0, Math.min(old.length, visCount * 3)));
      }
    }
    const agentOff = isAgent ? agentOffsetsRef.current : null;
    for (let i = 0; i < visCount; i++) {
      let s = scaleCache[i];
      if (i === selectedLocalIdx) {
        s *= waveScale;
      }

      const i3 = posIdxCache[i] * 3;
      let x: number, y: number, z: number;

      // Use live drag position for the dragged node (avoids 1-frame SAB lag)
      if (i === dragLocalIdx && drag) {
        x = drag.currentNodePos3D.x;
        y = drag.currentNodePos3D.y;
        z = drag.currentNodePos3D.z;
      } else if (positions && i3 + 2 < positions.length) {
        x = positions[i3]; y = positions[i3 + 1]; z = positions[i3 + 2];
      } else {
        // Cold fallback: SAB not yet populated for this index. Read the node's
        // own position; default to origin component-wise (no object literal).
        const p = currentNodes[i].position;
        x = p?.x ?? 0; y = p?.y ?? 0; z = p?.z ?? 0;
      }

      // Agent momentum-nudge: visual offset toward work target (working) or
      // away from graph centre (done/idle). Dragged nodes skip.
      if (agentOff && i !== dragLocalIdx) {
        const o3 = i * 3;
        const work = getAgentWork(String(currentNodes[i].id));
        if (work && work.state === 'working' && positions) {
          const tIdx = props.nodeIdToIndexMap.get(work.targetNodeId);
          if (tIdx !== undefined) {
            const t3 = tIdx * 3;
            if (t3 + 2 < positions.length) {
              const dx = positions[t3] - x - agentOff[o3];
              const dy = positions[t3 + 1] - y - agentOff[o3 + 1];
              const dz = positions[t3 + 2] - z - agentOff[o3 + 2];
              agentOff[o3] += dx * AGENT_NUDGE_SPEED;
              agentOff[o3 + 1] += dy * AGENT_NUDGE_SPEED;
              agentOff[o3 + 2] += dz * AGENT_NUDGE_SPEED;
            }
          }
        } else if (work && work.state === 'done') {
          const cx = x + agentOff[o3] - graphCX;
          const cy = y + agentOff[o3 + 1] - graphCY;
          const cz = z + agentOff[o3 + 2] - graphCZ;
          const d = Math.sqrt(cx * cx + cy * cy + cz * cz) || 1;
          agentOff[o3] += (cx / d) * AGENT_DRIFT_SPEED;
          agentOff[o3 + 1] += (cy / d) * AGENT_DRIFT_SPEED;
          agentOff[o3 + 2] += (cz / d) * AGENT_DRIFT_SPEED;
        }
        x += agentOff[o3];
        y += agentOff[o3 + 1];
        z += agentOff[o3 + 2];
      }

      if (xformBuf) {
        const t4 = i * 4;
        xformBuf[t4] = x; xformBuf[t4 + 1] = y; xformBuf[t4 + 2] = z; xformBuf[t4 + 3] = s;
      } else {
        _mat.makeScale(s, s, s);
        _mat.setPosition(x, y, z);
        inst.setMatrixAt(i, _mat);
      }
    }
    inst.count = visCount;
    if (xformBuf) {
      if (xformTexRef.current) xformTexRef.current.needsUpdate = true;
    } else {
      inst.instanceMatrix.needsUpdate = true;
    }

    // Dirty-flag metadata texture: only upload when inputs structurally change
    if (texBuf) {
      // Attention-heat (task V1): knowledge + ontology nodes heat up as agents
      // touch them (0x23 AGENT_ACTION → attentionHeat accumulator). The heat is
      // blended into meta.w below so it glows through the EXISTING recency
      // emissive path — no new shader code. The agent population keeps meta.w =
      // status activity untouched (do NOT disturb that path). Governed by the
      // knowledgeGraph.attentionHeat* knobs; both KG and ontology meshes are
      // non-agent, so both keep the shared accumulator's decay in step.
      if (!isAgent) {
        attentionHeat.configure({
          enabled: (kgVisuals?.attentionHeatEnabled ?? true),
          halfLifeMs: (kgVisuals?.attentionHeatHalfLife ?? 20) * 1000,
        });
      }
      // Throttle heat-driven re-uploads to ~2Hz: advance the bucket while heat is
      // live (each tick re-samples the fade) plus ONE settle tick after the last
      // heat decays away (writes meta.w back to pure recency). Structural changes
      // still upload immediately via the rest of metaHash.
      const heatLive = heatEnabled && attentionHeat.hasHeat();
      if (heatLive || (heatEnabled && heatWasActiveRef.current)) {
        const tNow = performance.now();
        if (tNow - lastHeatUploadRef.current >= HEAT_UPLOAD_INTERVAL_MS) {
          heatBucketRef.current++;
          lastHeatUploadRef.current = tNow;
          heatWasActiveRef.current = heatLive; // false once the settle tick runs
        }
      }

      const sampleHash = currentNodes.length > 0
        ? `${currentNodes[0]?.metadata?.authorityScore ?? 0}-${currentNodes[Math.floor(currentNodes.length / 2)]?.metadata?.quality ?? 0}-${currentNodes[currentNodes.length - 1]?.metadata?.authorityScore ?? 0}`
        : '';
      // Agent status feeds meta.w, but never touches authority/quality — fold a
      // per-agent status signature into the dirty key so a status change re-uploads
      // (agent populations are small, so the join is cheap).
      const statusSig = isAgent
        ? currentNodes.map(n => (n.metadata?.status as string | undefined) ?? '').join('')
        : '';
      // Fold the throttled heat bucket in so a decaying-heat frame re-uploads.
      const heatSig = heatEnabled ? `-h${heatBucketRef.current}` : '';
      const metaHash = `${currentNodes.length}-${connectionCountMap.size}-${selectedNodeId}-${sampleHash}-${statusSig}${heatSig}`;
      if (metaHash !== prevMetaHashRef.current) {
        for (let i = 0; i < currentNodes.length; i++) {
          const node = currentNodes[i];
          const i4 = i * 4;
          const nid = String(node.id);
          texBuf[i4]     = node.metadata?.quality ?? node.metadata?.authorityScore ?? 0.5;
          texBuf[i4 + 1] = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
          const cc = connectionCountMap.get(nid) || 0;
          texBuf[i4 + 2] = Math.min(cc / 20, 1.0);
          // meta.w: recency for gem/ontology (heated up by live attention when
          // enabled); status-derived activity for agents so the capsule shader
          // (and the WebGL GLSL glow) rests idle swarms and brightens/pulses
          // active ones. Reinterpreting the channel is safe here because this
          // mesh renders a single population.
          const recency = computeRecency(node.metadata?.lastModified ?? node.metadata?.updatedAt);
          if (isAgent) {
            const aw = getAgentWork(nid);
            texBuf[i4 + 3] = (aw && aw.state === 'done')
              ? AGENT_DONE_ACTIVITY
              : agentStatusActivity(node.metadata?.status as string | undefined);
          } else {
            texBuf[i4 + 3] = heatEnabled
              ? Math.max(recency, attentionHeat.getHeat(nid))
              : recency;
          }
        }
        if (metaTexRef.current) metaTexRef.current.needsUpdate = true;
        // WebGL parity: aGlowMeta wraps the SAME texBuf — re-upload it too so the
        // GLSL emissive injection sees the refreshed per-instance metadata.
        const glowAttr = inst.geometry.getAttribute('aGlowMeta');
        if (glowAttr) glowAttr.needsUpdate = true;
        prevMetaHashRef.current = metaHash;
      }
    }

    // --- Attention-heat → instanceColor (WebGL-visible per-instance heat) ------
    // instanceColor multiplies the built-in diffuse, so scaling the cached base
    // RGB by a per-node heat gain literally brightens a touched gem in place —
    // the signal the metadata/aGlowMeta emissive path is too subtle to surface on
    // WebGL. Runs on every 2Hz heat-bucket advance (the SAME throttle the metadata
    // upload uses), plus the one settle tick that writes colours back to base as
    // heat cools. Ratio-preserving gain keeps community/sssp hue (a hot red gem
    // brightens red, never white). Agents are excluded (heatEnabled is false).
    if (heatEnabled && heatBucketRef.current !== prevHeatColorBucketRef.current) {
      prevHeatColorBucketRef.current = heatBucketRef.current;
      const baseCache = baseColorCacheRef.current;
      if (baseCache && inst.instanceColor) {
        for (let i = 0; i < nodeCount; i++) {
          const b3 = i * 3;
          const br = baseCache[b3], bg = baseCache[b3 + 1], bb = baseCache[b3 + 2];
          const f = heatBrightenFactor(br, bg, bb, attentionHeat.getHeat(String(currentNodes[i].id)));
          _col.setRGB(br * f, bg * f, bb * f);
          inst.setColorAt(i, _col);
        }
        inst.instanceColor.needsUpdate = true;
      }
    }
  }, -1);

  // Commit-time ref reconciliation. The mesh/texture refs are assigned as side
  // effects inside the mesh useMemo, and React may run that memo in a render it
  // then DISCARDS (StrictMode double-render, concurrent interruption). The refs
  // then point at a mesh React never committed: useFrame drives — and manually
  // scene-adds — the phantom mesh (visible, count>0, no events) while the
  // committed mesh (the one R3F registered the pointer handlers on) stays at
  // count 0. Net effect: the graph you SEE has no event handlers and the mesh
  // with handlers renders nothing, so hover/click/drag/double-click all die
  // silently. R3F invokes this callback ref with the COMMITTED object at attach
  // time — re-point every per-mesh ref at it; the per-frame stale-mesh sweep
  // then removes and disposes the phantom generation.
  const handleCommittedMesh = useCallback((obj: THREE.InstancedMesh | null) => {
    if (!obj) return;
    if (meshRef.current !== obj) meshRef.current = obj;
    const xt = obj.userData.xformTex as THREE.DataTexture | undefined;
    if (xt && xformTexRef.current !== xt) { xformTexRef.current = xt; xt.needsUpdate = true; }
    const mt = obj.userData.metaTex as THREE.DataTexture | undefined;
    if (mt && metaTexRef.current !== mt) { metaTexRef.current = mt; mt.needsUpdate = true; }
  }, []);

  // This mesh renders a contiguous slice of the parent's displayNodes, so R3F
  // reports an instanceId LOCAL to this mesh. Shift it by instanceIdBase before
  // delegating so the parent's shared handler indexes the correct GLOBAL node.
  // Mutate-then-delegate preserves the synthetic event (stopPropagation et al.).
  const base = instanceIdBase ?? 0;
  const shiftInstanceId = <E extends { instanceId?: number }>(e: E): E => {
    if (base !== 0 && typeof e.instanceId === 'number') e.instanceId += base;
    return e;
  };

  return (
    <primitive
      key={mesh.uuid}
      object={mesh}
      ref={handleCommittedMesh}
      onPointerDown={(e: ThreeEvent<PointerEvent>) => onPointerDown(shiftInstanceId(e))}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerMissed={onPointerMissed}
      onDoubleClick={(e: ThreeEvent<MouseEvent>) => onDoubleClick(shiftInstanceId(e))}
      onContextMenu={onContextMenu ? (e: ThreeEvent<MouseEvent>) => onContextMenu(shiftInstanceId(e)) : undefined}
    />
  );
};

export const GemNodes = forwardRef(GemNodesInner);
export default GemNodes;
