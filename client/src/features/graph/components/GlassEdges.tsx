import React, { useMemo, useCallback, useEffect, useRef, forwardRef, useImperativeHandle } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import {
  createGlassEdgeMaterial,
  createGlassEdgeGeometry,
} from '../../../rendering/materials/GlassEdgeMaterial';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GemMaterialSettings, GlowSettings } from '../../settings/config/settings';

/**
 * Phase 6 (ADR-04 D1, D3, D4): edge capacity is dynamic and configured —
 * never a magic constant. The previous `MAX_EDGES = 10_000` (and the
 * symptom-level `16_000` bump in main commit d1f7f2548) are replaced by:
 *   - EDGE_INITIAL = 1024 — first allocation when points first arrive
 *   - grow x2 on overflow up to settings.rendering.maxEdgesCeiling
 *   - default ceiling DEFAULT_EDGE_CEILING = 32_768
 *   - edges above ceiling: draw `ceiling`, emit one structured warn naming
 *     the unrendered count. Never silent truncation.
 *
 * Phase 6 (ADR-04 D2 / T2): surface-to-surface offset for edge endpoints
 * is applied by the caller (GraphManager.tsx) which has per-node access
 * to compute `srcR = computeNodeScale(node, ...) * nodeSize` for each
 * edge. The `points` buffer arriving here therefore already encodes
 * surface-to-surface endpoints. This file documents that contract; the
 * matrix composition below assumes the two endpoints are the
 * surface-projected positions and draws the cylinder from midpoint with
 * full length, which is geometrically identical to the surface-aware
 * composition in ADR-04 D2 when the caller pre-offsets.
 *
 * The collapsed-edge guard (`len < 1e-6`) handles the `adjLen <= 0`
 * (overlapping nodes) case by scaling to zero.
 */
const EDGE_INITIAL = 1024;
const DEFAULT_EDGE_CEILING = 32768;

interface GlassEdgesProps {
  points: number[];
  settings: any;
  colorOverride?: string;
}

export interface GlassEdgesHandle {
  updatePoints(points: number[], count?: number): void;
  /** Update per-instance colors. Array of [r,g,b] floats (0-1), 3 per edge. */
  updateColors(colors: Float32Array, count: number): void;
}

/** Pre-allocated temp objects for matrix composition -- avoids per-frame GC. */
const tmpMat = new THREE.Matrix4();
const tmpPos = new THREE.Vector3();
const tmpSrc = new THREE.Vector3();
const tmpTgt = new THREE.Vector3();
const tmpUp = new THREE.Vector3(0, 1, 0);
const tmpQuat = new THREE.Quaternion();
const tmpDir = new THREE.Vector3();
const tmpScale = new THREE.Vector3();

/** Round up to next power of two for capacity sizing. */
function ceilToPowerOfTwo(n: number): number {
  if (n <= 1) return 1;
  return Math.pow(2, Math.ceil(Math.log2(n)));
}

/** Compute up to `limit` edge matrices. Returns total edge count (clamped to capacity).
 *  `dataLength` limits how many elements of `pts` to consider (avoids needing a sliced copy). */
function computeInstanceMatrices(
  mesh: THREE.InstancedMesh,
  pts: number[],
  capacity: number,
  limit?: number,
  dataLength?: number,
): number {
  const edgeCount = Math.min(Math.floor((dataLength ?? pts.length) / 6), capacity);
  const renderCount = limit !== undefined ? Math.min(limit, edgeCount) : edgeCount;
  for (let i = 0; i < renderCount; i++) {
    const off = i * 6;
    tmpSrc.set(pts[off], pts[off + 1], pts[off + 2]);
    tmpTgt.set(pts[off + 3], pts[off + 4], pts[off + 5]);

    // Midpoint
    tmpPos.addVectors(tmpSrc, tmpTgt).multiplyScalar(0.5);

    // Direction and length
    tmpDir.subVectors(tmpTgt, tmpSrc);
    const len = tmpDir.length();
    if (len < 1e-6) {
      // Collapsed/overlapping nodes (adjLen <= 0 case in ADR-04 D2)
      tmpMat.makeScale(0, 0, 0);
      mesh.setMatrixAt(i, tmpMat);
      continue;
    }
    tmpDir.normalize();

    // Quaternion: rotate unit-Y cylinder to align with edge direction
    // Guard against anti-parallel vectors (dot ~ -1) which cause NaN
    const dot = tmpUp.dot(tmpDir);
    if (dot < -0.9999) {
      // Anti-parallel: 180-degree rotation around X axis
      tmpQuat.set(1, 0, 0, 0);
    } else {
      tmpQuat.setFromUnitVectors(tmpUp, tmpDir);
    }

    // Compose: translate to midpoint, rotate, scale Y by length
    tmpScale.set(1, len, 1);
    tmpMat.compose(tmpPos, tmpQuat, tmpScale);
    mesh.setMatrixAt(i, tmpMat);
  }

  mesh.count = renderCount;
  mesh.instanceMatrix.needsUpdate = true;
  return edgeCount;
}

/** Allocate a new InstancedMesh + instanceColor buffer at the given capacity. */
function allocateMesh(
  capacity: number,
  edgeRadius: number,
  initialColor: string | undefined,
  initialOpacity: number | undefined,
): { mesh: THREE.InstancedMesh; uniforms: any } {
  const geo = createGlassEdgeGeometry(edgeRadius);
  const result = createGlassEdgeMaterial(initialColor);
  if (initialOpacity !== undefined) {
    (result.material as THREE.MeshPhysicalMaterial).opacity = initialOpacity;
  }
  const m = new THREE.InstancedMesh(geo, result.material, capacity);
  m.frustumCulled = false;
  m.count = 0;
  const colorArray = new Float32Array(capacity * 3);
  // Initialize to white (neutral multiply against material base color)
  for (let ci = 0; ci < colorArray.length; ci++) colorArray[ci] = 1.0;
  m.instanceColor = new THREE.InstancedBufferAttribute(colorArray, 3);
  return { mesh: m, uniforms: result.uniforms };
}

export const GlassEdges = forwardRef<GlassEdgesHandle, GlassEdgesProps>(
  ({ points, settings, colorOverride }, ref) => {
    const meshRef = useRef<THREE.InstancedMesh | null>(null);
    const edgeRevealRef = useRef(0);
    const totalEdgesRef = useRef(0);
    const instanceColorsActiveRef = useRef(false);
    // Read node reveal batch from settings to keep edge reveal in sync
    const nodeRevealBatch = (settings?.revealBatch as number | undefined) ?? 120;
    const EDGE_REVEAL_BATCH = Math.max(1, Math.round(nodeRevealBatch * 0.67));

    // --- Phase 6 (ADR-04 D1): dynamic capacity ---
    const renderingCeiling = useSettingsStore(s => s.settings?.visualisation?.rendering?.maxEdgesCeiling);
    const ceilingRef = useRef<number>(renderingCeiling ?? DEFAULT_EDGE_CEILING);
    // keep the ref in sync if the setting changes mid-session — the value is
    // only consulted at allocation time, so live-edit will affect the *next*
    // growth event rather than retroactively shrinking the buffer
    ceilingRef.current = renderingCeiling ?? DEFAULT_EDGE_CEILING;

    const capacityRef = useRef<number>(0);
    // Track reallocation frequency for telemetry — flag if >2 in 60s.
    const reallocationTimestampsRef = useRef<number[]>([]);
    // Track whether we have warned about ceiling overflow this prop tick.
    const overflowWarnedRef = useRef<number>(0);
    // Latest uniforms returned by allocateMesh — kept so consumers reading
    // them through this ref don't lose access on growth.
    const uniformsRef = useRef<any>(null);

    /** Allocate or grow the mesh to `targetCapacity`. Disposes the old mesh. */
    const reallocate = useCallback((targetCapacity: number): THREE.InstancedMesh => {
      const initialColor = colorOverride || settings?.color || undefined;
      const initialOpacity = settings?.opacity;
      const edgeRadius = settings?.edgeRadius ?? 0.03;
      const { mesh: nextMesh, uniforms: nextUniforms } = allocateMesh(
        targetCapacity,
        edgeRadius,
        initialColor,
        initialOpacity,
      );
      const prev = meshRef.current;
      if (prev) {
        // Copy prior per-instance colours before replacing the attribute
        const prevColors = (prev.instanceColor as THREE.InstancedBufferAttribute | null)
          ?.array as Float32Array | undefined;
        const nextColors = (nextMesh.instanceColor as THREE.InstancedBufferAttribute)
          .array as Float32Array;
        if (prevColors) {
          nextColors.set(prevColors.subarray(0, Math.min(prevColors.length, nextColors.length)));
        }
        prev.geometry.dispose();
        (prev.material as THREE.Material).dispose();
        prev.dispose();
      }
      meshRef.current = nextMesh;
      uniformsRef.current = nextUniforms;
      capacityRef.current = targetCapacity;

      // Telemetry: log reallocation frequency
      const now = performance.now();
      const ts = reallocationTimestampsRef.current;
      ts.push(now);
      // prune to last 60s
      while (ts.length > 0 && now - ts[0] > 60_000) ts.shift();
      if (ts.length > 2) {
        // eslint-disable-next-line no-console
        console.warn(
          `[GlassEdges] capacity reallocated ${ts.length}x in last 60s — ` +
          `consider raising rendering.maxEdgesCeiling above current capacity ${targetCapacity}.`
        );
      }
      return nextMesh;
    }, [colorOverride, settings?.color, settings?.opacity, settings?.edgeRadius]);

    // Initial mesh allocation: small placeholder until first non-empty points.
    const { mesh, uniforms } = useMemo(() => {
      const { mesh: m, uniforms: u } = allocateMesh(
        EDGE_INITIAL,
        settings?.edgeRadius ?? 0.03,
        colorOverride || settings?.color || undefined,
        settings?.opacity,
      );
      meshRef.current = m;
      uniformsRef.current = u;
      capacityRef.current = EDGE_INITIAL;

      // Initial population — first batch only, rest via progressive reveal
      if (points.length >= 6) {
        const initialEdgeCount = Math.floor(points.length / 6);
        const ceiling = ceilingRef.current;
        const sized = Math.min(ceilToPowerOfTwo(initialEdgeCount * 1.25), ceiling);
        if (sized > EDGE_INITIAL && initialEdgeCount > EDGE_INITIAL) {
          // Need a bigger first-allocation: re-allocate now before reveal starts
          // (ADR-04 D3: capacity sized to final count before reveal begins)
          const targetCapacity = Math.max(sized, EDGE_INITIAL);
          const { mesh: bigger, uniforms: biggerUniforms } = allocateMesh(
            targetCapacity,
            settings?.edgeRadius ?? 0.03,
            colorOverride || settings?.color || undefined,
            settings?.opacity,
          );
          m.geometry.dispose();
          (m.material as THREE.Material).dispose();
          m.dispose();
          meshRef.current = bigger;
          uniformsRef.current = biggerUniforms;
          capacityRef.current = targetCapacity;
          computeInstanceMatrices(bigger, points, targetCapacity, EDGE_REVEAL_BATCH);
          return { mesh: bigger, uniforms: biggerUniforms };
        }
        computeInstanceMatrices(m, points, capacityRef.current, EDGE_REVEAL_BATCH);
      }
      return { mesh: m, uniforms: u };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Update material color and opacity when settings change.
    // When per-instance colors are active (updateColors was called),
    // keep base color white so instance colors come through pure.
    useEffect(() => {
      const m = meshRef.current ?? mesh;
      const mat = m.material as THREE.MeshPhysicalMaterial;
      if (!instanceColorsActiveRef.current) {
        const targetColor = colorOverride || settings?.color;
        if (targetColor && mat.color) {
          mat.color.set(targetColor);
        }
      }
      if (settings?.opacity !== undefined) {
        mat.opacity = settings.opacity;
      }
      mat.needsUpdate = true;
    }, [colorOverride, settings?.color, settings?.opacity, mesh]);

    // Read glow settings for edge emissive values.
    // Stored in a ref so useCallback closures always read the latest value
    // without needing glowSettings in their dependency arrays.
    const glowSettings = useSettingsStore(s => s.get<GlowSettings>('visualisation.glow'));
    const glowSettingsRef = useRef(glowSettings);
    glowSettingsRef.current = glowSettings;

    // Apply shared gem material settings (ior, transmission) from settings store
    const gemSettings = useSettingsStore(s => s.get<GemMaterialSettings>('visualisation.gemMaterial'));
    useEffect(() => {
      if (!gemSettings) return;
      const m = meshRef.current ?? mesh;
      const mat = m.material as THREE.MeshPhysicalMaterial;
      if (gemSettings.ior !== undefined) mat.ior = gemSettings.ior;
      if (gemSettings.transmission !== undefined) mat.transmission = gemSettings.transmission;
      mat.needsUpdate = true;
    }, [gemSettings, mesh]);

    /** Ensure capacity is sufficient for `edgeCount`. May reallocate. */
    const ensureCapacity = useCallback((edgeCount: number): THREE.InstancedMesh => {
      const ceiling = ceilingRef.current;
      const capacity = capacityRef.current;
      if (edgeCount <= capacity) return meshRef.current!;

      // Need to grow. If we'd exceed ceiling, draw ceiling edges and warn.
      if (edgeCount > ceiling) {
        if (capacity < ceiling) {
          // First grow to ceiling
          reallocate(ceiling);
        }
        if (overflowWarnedRef.current !== edgeCount) {
          overflowWarnedRef.current = edgeCount;
          // eslint-disable-next-line no-console
          console.warn(
            `[GlassEdges] edge count ${edgeCount} exceeds ceiling ${ceiling}. ` +
            `${edgeCount - ceiling} edges will not be rendered. ` +
            `Raise rendering.maxEdgesCeiling to render all edges.`
          );
        }
        return meshRef.current!;
      }

      const newCap = Math.min(Math.max(edgeCount, capacity * 2), ceiling);
      return reallocate(newCap);
    }, [reallocate]);

    // Recompute when points prop changes -- reset progressive reveal
    useEffect(() => {
      if (points.length >= 6) {
        const edgeCount = Math.floor(points.length / 6);
        ensureCapacity(edgeCount);
        totalEdgesRef.current = Math.min(edgeCount, capacityRef.current);
        edgeRevealRef.current = 0; // Reset for progressive reveal in useFrame
        // Reset overflow warn flag so next overflow logs again
        if (edgeCount <= capacityRef.current) overflowWarnedRef.current = 0;
      } else {
        const m = meshRef.current ?? mesh;
        m.count = 0;
        totalEdgesRef.current = 0;
        m.instanceMatrix.needsUpdate = true;
      }
    }, [points, mesh, ensureCapacity]);

    // Dispose GPU resources on unmount.
    // R3F <primitive> never auto-disposes, so manual cleanup is required.
    useEffect(() => {
      return () => {
        const m = meshRef.current;
        if (m) {
          m.geometry?.dispose();
          if (m.material) {
            (m.material as THREE.Material).dispose();
          }
          m.dispose();
        }
      };
    }, []);

    // Imperative path for hot-loop updates from useFrame callers.
    // Always recompute instance matrices when called — the caller
    // (GraphManager useFrame) already controls call frequency.
    // Previous hash-based dedup (`${len}-first-last`) was too coarse:
    // it only sampled 3 values, causing edges to freeze when those
    // specific values were stable while interior edge positions changed.
    const updatePoints = useCallback(
      (newPts: number[], count?: number) => {
        const len = count ?? newPts.length;
        const m = meshRef.current;
        if (!m) return;
        if (len < 6) {
          if (m.count !== 0) {
            m.count = 0;
            m.instanceMatrix.needsUpdate = true;
          }
          return;
        }
        const edgeCount = Math.floor(len / 6);
        const activeMesh = ensureCapacity(edgeCount);
        computeInstanceMatrices(activeMesh, newPts, capacityRef.current, undefined, len);
      },
      [ensureCapacity],
    );

    // Update per-instance edge colors from a packed Float32Array [r,g,b, r,g,b, ...]
    const updateColors = useCallback(
      (colors: Float32Array, count: number) => {
        const m = meshRef.current;
        if (!m || !m.instanceColor) return;
        const attr = m.instanceColor as THREE.InstancedBufferAttribute;
        const dst = attr.array as Float32Array;
        const len = Math.min(count * 3, dst.length, colors.length);
        dst.set(colors.subarray(0, len));
        attr.needsUpdate = true;
        // When per-instance colors are active, set base color to white
        // so the instance colors come through unmodified by material tint
        instanceColorsActiveRef.current = count > 0;
        const mat = m.material as THREE.MeshPhysicalMaterial;
        if (count > 0 && mat.color) {
          mat.color.setRGB(1, 1, 1);
          // Read edge emissive base from glow settings (via ref for stable callback identity)
          const edgeEmissiveBase = glowSettingsRef.current?.edgeGlowStrength ?? 0.3;
          const normalizedEmissive = Math.min(edgeEmissiveBase / 3, 1.0);
          mat.emissive.setRGB(normalizedEmissive, normalizedEmissive, normalizedEmissive);
        }
      },
      [],
    );

    useImperativeHandle(ref, () => ({ updatePoints, updateColors }), [updatePoints, updateColors]);

    // Subtle emissive pulse on edges — base and amplitude driven by glow settings.
    // Phase 6 (ADR-04 D10): no allocations inside useFrame — only reads refs
    // and writes to existing material properties.
    useFrame(({ clock }) => {
      const m = meshRef.current;
      const mat = m?.material as THREE.MeshPhysicalMaterial | undefined;
      if (mat) {
        const gs = glowSettingsRef.current;
        const edgeGlow = gs?.edgeGlowStrength ?? 1.0;
        const pulseSpeed = gs?.pulseSpeed ?? 0.8;
        const emissiveBase = 0.15 * edgeGlow;
        const emissiveAmplitude = 0.08 * edgeGlow;
        mat.emissiveIntensity = emissiveBase + Math.sin(clock.elapsedTime * pulseSpeed) * emissiveAmplitude;
      }

      // Progressive edge reveal: ramp up each frame (initial prop-based load only)
      if (m && edgeRevealRef.current < totalEdgesRef.current && points.length >= 6) {
        edgeRevealRef.current = Math.min(
          edgeRevealRef.current + EDGE_REVEAL_BATCH,
          totalEdgesRef.current,
        );
        computeInstanceMatrices(m, points, capacityRef.current, edgeRevealRef.current);
      }
    });

    // The mesh ref can change on capacity growth, so we render through a
    // <group> + <primitive> that reads the current ref each render.
    return <primitive object={meshRef.current ?? mesh} />;
  },
);

GlassEdges.displayName = 'GlassEdges';

export default GlassEdges;
