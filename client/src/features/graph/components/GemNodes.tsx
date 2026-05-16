import React, { useRef, useMemo, useCallback, useEffect, forwardRef, useImperativeHandle } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import type { GraphVisualMode } from './GraphManager';
import type { Node as GraphNode } from '../managers/graphDataManager';
import { createGemNodeMaterial, createTslGemMaterial, createGemGeometry } from '../../../rendering/materials/GemNodeMaterial';
import { createCrystalOrbMaterial, createCrystalOrbGeometry } from '../../../rendering/materials/CrystalOrbMaterial';
import { createAgentCapsuleMaterial, createAgentCapsuleGeometry } from '../../../rendering/materials/AgentCapsuleMaterial';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GemMaterialSettings, GraphTypeVisualsSettings, QualityGatesSettings } from '../../settings/config/settings';
import type { Edge } from '../managers/graphDataManager';
import type { ThreeEvent } from '@react-three/fiber';
import { createLogger } from '../../../utils/loggerConfig';
// ADR-03 D7: analytics buffer no longer lives in the worker.
// import { graphWorkerProxy } from '../managers/graphWorkerProxy';

const logger = createLogger('GemNodes');
import { computeNodeScale } from '../utils/nodeScaling';
import { isWebGPURenderer } from '../../../rendering/rendererFactory';

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
    onPointerDown, onPointerMove, onPointerUp, onPointerMissed, onDoubleClick,
    selectedNodeId,
  } = props;

  const meshRef = useRef<THREE.InstancedMesh | null>(null);
  const metaTexRef = useRef<THREE.DataTexture | null>(null);
  const prevMetaHashRef = useRef('');
  const dominant = getDominantMode(nodes, graphMode, perNodeVisualModeMap);

  // Read gem material settings from the settings store for live tuning
  const gemSettings = useSettingsStore(s => s.get<GemMaterialSettings>('visualisation.gemMaterial'));
  const lastGemSettingsRef = useRef<string>('');

  // Quality gate toggles for cluster/anomaly/community coloring
  const qualityGates = useSettingsStore(s => s.get<QualityGatesSettings>('qualityGates'));
  // Per-node analytics data from binary protocol V3 (refreshed periodically)
  const analyticsRef = useRef<Float32Array | null>(null);
  const analyticsFrameRef = useRef(0);

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

    // Per-instance metadata texture for TSL (RGBA float: quality, authority, connections, recency)
    const texData = new Float32Array(count * 4);
    const metaTex = new THREE.DataTexture(texData, count, 1, THREE.RGBAFormat, THREE.FloatType);
    metaTex.minFilter = THREE.NearestFilter;
    metaTex.magFilter = THREE.NearestFilter;
    metaTex.needsUpdate = true;
    metaTexRef.current = metaTex;

    meshRef.current = inst;
    return { mesh: inst, uniforms: matResult.uniforms };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dominant, capacityKey]);

  // Dispose previous GPU resources when dominant mode changes or on unmount.
  // R3F <primitive> never auto-disposes, so manual cleanup is required.
  useEffect(() => {
    const currentMetaTex = metaTexRef.current;
    return () => {
      if (mesh) {
        mesh.geometry?.dispose();
        if (mesh.material) {
          (mesh.material as THREE.Material).dispose();
        }
        mesh.dispose();
      }
      if (currentMetaTex) {
        currentMetaTex.dispose();
      }
    };
  }, [mesh]);

  useImperativeHandle(ref, () => ({
    getMesh: () => meshRef.current,
    getColorArray: () => meshRef.current?.instanceColor?.array as Float32Array | null ?? null,
  }), [mesh]);

  // TSL ENABLED (r183+) with PBR fallback — wire createTslGemMaterial once
  // the metadata texture and mesh are ready. Per-frame emissive modulation in
  // useFrame remains the active fallback path if TSL fails.
  const tslAppliedRef = useRef(false);
  useEffect(() => {
    if (
      dominant === 'knowledge_graph' &&
      isWebGPURenderer &&
      mesh &&
      metaTexRef.current &&
      nodes.length > 0 &&
      !tslAppliedRef.current
    ) {
      tslAppliedRef.current = true;
      createTslGemMaterial(
        mesh.material as THREE.MeshPhysicalMaterial,
        metaTexRef.current,
        nodes.length,
      ).then(success => {
        if (success) {
          logger.debug('[GemNodes] TSL metadata material active');
        } else {
          tslAppliedRef.current = false; // allow retry on next render cycle
        }
      }).catch(() => {
        tslAppliedRef.current = false;
      });
    }
    // Reset flag when dominant mode changes away from knowledge_graph
    if (dominant !== 'knowledge_graph') {
      tslAppliedRef.current = false;
    }
  }, [dominant, mesh, nodes.length]);

  const computeColor = useCallback((node: GraphNode, mode: GraphVisualMode, nodeIndex?: number): THREE.Color => {
    // Quality gate overrides: color-code by cluster/anomaly/community when enabled.
    // These take precedence over standard mode coloring (but not SSSP highlight).
    const analytics = analyticsRef.current;
    if (analytics && nodeIndex !== undefined && nodeIndex * 3 + 2 < analytics.length) {
      const a3 = nodeIndex * 3;
      const clusterId = analytics[a3];
      const anomalyScore = analytics[a3 + 1];
      const communityId = analytics[a3 + 2];

      // Anomaly highlighting: red intensity proportional to anomalyScore (0-1)
      if (qualityGates?.showAnomalies && anomalyScore > 0.01) {
        const intensity = Math.min(anomalyScore, 1.0);
        return _col.setRGB(0.9 * intensity + 0.1, 0.15 * (1 - intensity), 0.1);
      }

      // Cluster coloring: deterministic hue from clusterId (non-zero means assigned)
      if (qualityGates?.showClusters && clusterId > 0) {
        const hue = ((clusterId * 137) % 360) / 360; // golden angle spacing
        return _col.setHSL(hue, 0.7, 0.55);
      }

      // Community coloring: deterministic hue from communityId
      if (qualityGates?.showCommunities && communityId > 0) {
        const hue = ((communityId * 83) % 360) / 360;
        return _col.setHSL(hue, 0.65, 0.5);
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
      const depth = hierarchyMap?.get(node.id)?.depth ?? (node.metadata?.depth ?? 0);
      return _col.set(ONTOLOGY_SPECTRUM[Math.min(depth, ONTOLOGY_SPECTRUM.length - 1)]);
    }
    // Knowledge graph: derive hue from node label for visual differentiation,
    // with authority driving saturation + lightness. Connection count adds warmth.
    const auth = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
    const cc = connectionCountMap.get(String(node.id)) || 0;
    const hue = hashHue(node.label || String(node.id));
    const sat = 0.35 + auth * 0.35 + Math.min(cc / 20, 0.2);
    const lit = 0.45 + auth * 0.2;
    _col.setHSL(hue, Math.min(sat, 0.9), Math.min(lit, 0.75));
    return _col;
  }, [ssspResult, hierarchyMap, connectionCountMap, qualityGates]);

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
    if (!inst || nodes.length === 0) return;

    // Workaround: R3F <primitive> sometimes fails to attach InstancedMesh to scene.
    // If the mesh has no parent after mount, attach it directly.
    if (!inst.parent && scene) {
      scene.add(inst);
      logger.debug('[GemNodes] manually attached mesh to scene (R3F primitive workaround)');
    }

    if (uniforms.time) uniforms.time.value = clock.elapsedTime;

    // Track node count changes. Only reset reveal to 0 on fresh data load
    // (transitioning from 0 nodes). For incremental changes (filter toggles,
    // websocket updates), just clamp to avoid all-nodes-vanish flicker.
    if (nodes.length !== prevNodeCountRef.current) {
      if (prevNodeCountRef.current === 0) {
        revealedRef.current = 0; // Fresh load: wave-in from zero
      } else {
        revealedRef.current = Math.min(revealedRef.current, nodes.length);
      }
      prevNodeCountRef.current = nodes.length;
    }

    const positions = nodePositionsRef.current;
    frameCountRef.current++;

    // Refresh per-node analytics from worker every ~30 frames (~0.5s at 60fps)
    // to avoid Comlink overhead on every frame.
    analyticsFrameRef.current++;
    if (analyticsFrameRef.current % 30 === 1 &&
        (qualityGates?.showClusters || qualityGates?.showAnomalies || qualityGates?.showCommunities)) {
      // ADR-03 D7: analytics no longer live in the worker. Wire to the
      // main-thread analytics store in Phase 5; buffer remains stale until then.
      void analyticsFrameRef;
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
          nodeCount: nodes.length,
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
          matSamples,
          bboxSize: { x: +bboxSize.x.toFixed(1), y: +bboxSize.y.toFixed(1), z: +bboxSize.z.toFixed(1) },
          cameraPos: { x: +camera.position.x.toFixed(1), y: +camera.position.y.toFixed(1), z: +camera.position.z.toFixed(1) },
          dominant,
        });
      }
    }
    const visSettings = settings?.visualisation as Record<string, unknown> | undefined;
    const nodeSettings = visSettings?.nodes as Record<string, unknown> | undefined;
    const baseScale = ((nodeSettings?.nodeSize as number | undefined) ?? 0.5) / 0.5;
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

      if (!pEnabled) {
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
    if (revealedRef.current > nodes.length) {
      revealedRef.current = nodes.length;
    } else if (revealedRef.current < nodes.length) {
      revealedRef.current = Math.min(revealedRef.current + REVEAL_BATCH, nodes.length);
    }
    const visCount = revealedRef.current;

    let colorsDirty = false;
    for (let i = 0; i < visCount; i++) {
      const node = nodes[i];
      const mode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
      const propsVis = props.settings?.visualisation as Record<string, unknown> | undefined;
      let s = computeNodeScale(node, connectionCountMap, mode, hierarchyMap, propsVis?.graphTypeVisuals as GraphTypeVisualsSettings | undefined) * baseScale;
      const waveEnabled = animEnabled && ((anims?.selectionWaveEnabled as boolean | undefined) ?? true);
      const wSpeed = (anims?.waveSpeed as number | undefined) ?? 1.0;
      if (selectedNodeId && String(node.id) === selectedNodeId && waveEnabled) {
        s *= 1 + Math.sin(clock.elapsedTime * 3 * wSpeed) * 0.15;
      }

      // Map from visibleNodes index to graphData.nodes index for correct position lookup
      const nodeIdStr = String(node.id);
      const srcIdx = props.nodeIdToIndexMap.get(nodeIdStr);
      const posIdx = srcIdx !== undefined ? srcIdx : i;
      const i3 = posIdx * 3;
      let x: number, y: number, z: number;

      // Use live drag position for the dragged node (avoids 1-frame SAB lag)
      const drag = props.dragDataRef?.current;
      if (drag && drag.isDragging && drag.nodeId === nodeIdStr) {
        x = drag.currentNodePos3D.x;
        y = drag.currentNodePos3D.y;
        z = drag.currentNodePos3D.z;
      } else if (positions && i3 + 2 < positions.length) {
        x = positions[i3]; y = positions[i3 + 1]; z = positions[i3 + 2];
      } else {
        const p = node.position || { x: 0, y: 0, z: 0 };
        x = p.x; y = p.y; z = p.z;
      }
      _mat.makeScale(s, s, s);
      _mat.setPosition(x, y, z);
      inst.setMatrixAt(i, _mat);

      // Per-instance color via Three.js managed instanceColor.
      // Pass posIdx so computeColor can look up analytics data for this node.
      const c = computeColor(node, mode, posIdx);
      inst.setColorAt(i, c);
      colorsDirty = true;
    }
    inst.count = visCount;
    inst.instanceMatrix.needsUpdate = true;

    // Only upload instanceColor buffer when colors were actually written this frame
    if (inst.instanceColor && colorsDirty) inst.instanceColor.needsUpdate = true;

    // Dirty-flag metadata texture: only upload when inputs structurally change
    if (texBuf) {
      const sampleHash = nodes.length > 0
        ? `${nodes[0]?.metadata?.authorityScore ?? 0}-${nodes[Math.floor(nodes.length / 2)]?.metadata?.quality ?? 0}-${nodes[nodes.length - 1]?.metadata?.authorityScore ?? 0}`
        : '';
      const metaHash = `${nodes.length}-${connectionCountMap.size}-${selectedNodeId}-${sampleHash}`;
      if (metaHash !== prevMetaHashRef.current) {
        for (let i = 0; i < nodes.length; i++) {
          const node = nodes[i];
          const i4 = i * 4;
          const nid = String(node.id);
          texBuf[i4]     = node.metadata?.quality ?? node.metadata?.authorityScore ?? 0.5;
          texBuf[i4 + 1] = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
          const cc = connectionCountMap.get(nid) || 0;
          texBuf[i4 + 2] = Math.min(cc / 20, 1.0);
          texBuf[i4 + 3] = computeRecency(node.metadata?.lastModified ?? node.metadata?.updatedAt);
        }
        if (metaTexRef.current) metaTexRef.current.needsUpdate = true;
        prevMetaHashRef.current = metaHash;
      }
    }
  }, -1);

  return (
    <primitive
      key={mesh.uuid}
      object={mesh}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerMissed={onPointerMissed}
      onDoubleClick={onDoubleClick}
    />
  );
};

export const GemNodes = forwardRef(GemNodesInner);
export default GemNodes;
