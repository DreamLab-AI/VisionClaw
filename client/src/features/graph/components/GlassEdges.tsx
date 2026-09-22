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
 *   - default ceiling DEFAULT_EDGE_CEILING = 65_536 (a CAPACITY ceiling, not a
 *     per-frame draw commitment — `mesh.count` is still the live edge count;
 *     the buffer just reserves ~5 MB of matrix+colour headroom so dense graphs
 *     at 25k-node scale render far more of their edges before truncating)
 *   - edges above ceiling: draw `ceiling`, never silent truncation. When the
 *     ceiling is a configured value (settings present) the cap is by design →
 *     one structured `console.info` line. When we're on the hardcoded default
 *     fallback (setting absent) → one `console.warn` nudging configuration.
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
const DEFAULT_EDGE_CEILING = 65536;

interface GlassEdgesProps {
  points: number[];
  settings: any;
  colorOverride?: string;
}

export interface GlassEdgesHandle {
  updatePoints(points: number[], count?: number): void;
  /** Update per-instance colors. Array of [r,g,b] floats (0-1), 3 per edge. */
  updateColors(colors: Float32Array, count: number): void;
  /** Update per-edge raw weights (one float per edge). Drives tube radius
   *  (X/Z axes) via a principled sqrt-area mapping; length (Y) is unaffected. */
  updateWidths(weights: Float32Array, count: number): void;
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

// --- Principled edge-weight -> tube-radius mapping (NO magic multiplier) ---
// The cylinder geometry is built once at the design radius (settings.baseWidth, 1:1).
// Per-edge weight rescales only the X/Z (radius) axes of the instance matrix;
// the Y axis stays = edge length, so endpoints and length are untouched.
//
//   factor = clamp( sqrt(weight / WEIGHT_MEDIAN), MIN_FACTOR, MAX_FACTOR )
//
// - sqrt: a tube's visible cross-sectional area ∝ radius². Taking sqrt(weight)
//   makes rendered AREA grow linearly with weight — perceptually honest, the
//   ink an edge occupies is proportional to its weight.
// - / WEIGHT_MEDIAN (p50 = 1 for this dataset): the typical edge renders at the
//   geometry's design radius (factor 1). Reading the median rather than a fixed
//   scalar keeps the mapping correct if the weight distribution shifts.
// - clamp [0.5, 2.0]: weight 0.5 -> 0.707 -> floored to 0.5 (thin); weight 7 ->
//   sqrt(7)=2.646 -> capped to 2.0 (thick). Bounds keep the thickest edges
//   legible without swamping the scene, and the thinnest still hittable.
const WEIGHT_MEDIAN = 1.0;
const MIN_WIDTH_FACTOR = 0.5;
const MAX_WIDTH_FACTOR = 2.0;
function weightToRadiusFactor(weight: number): number {
  if (!(weight > 0)) return 1.0; // missing/0/NaN weight -> design radius
  const f = Math.sqrt(weight / WEIGHT_MEDIAN);
  return f < MIN_WIDTH_FACTOR ? MIN_WIDTH_FACTOR : f > MAX_WIDTH_FACTOR ? MAX_WIDTH_FACTOR : f;
}

/** Compute up to `limit` edge matrices. Returns total edge count (clamped to capacity).
 *  `dataLength` limits how many elements of `pts` to consider (avoids needing a sliced copy).
 *  `radiusFactors`, if supplied, holds PRECOMPUTED per-edge X/Z radius factors
 *  (weightToRadiusFactor already applied at width-update time). The frame loop
 *  reads them by index — no sqrt/clamp recompute here. Math is byte-identical to
 *  computing `weightToRadiusFactor(weight)` inline; only the timing moved. */
function computeInstanceMatrices(
  mesh: THREE.InstancedMesh,
  pts: number[],
  capacity: number,
  limit?: number,
  dataLength?: number,
  radiusFactors?: Float32Array | null,
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

    // Compose: translate to midpoint, rotate, scale Y by LENGTH (unchanged)
    // and X/Z by the per-edge weight->radius factor (1.0 when no weights given,
    // preserving prior fixed-radius behaviour). Only the tube radius changes;
    // source/target endpoints and length are untouched.
    const r = radiusFactors ? radiusFactors[i] : 1.0;
    tmpScale.set(r, len, r);
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
    // Precomputed per-edge X/Z radius factors (weightToRadiusFactor already
    // applied). Edge weight is SEMANTIC — it changes only on graph data/settings
    // change, never frame-to-frame — so the sqrt/clamp is computed ONCE here when
    // updateWidths detects a changed weight buffer, not per edge per frame in the
    // matrix hot loop (~21411 redundant sqrt+clamp/frame eliminated). The factor
    // buffer is owned here and grown in place; the frame loop reads it by index.
    const radiusFactorsRef = useRef<Float32Array | null>(null);
    // Cheap numeric change-detection signature for the incoming weight buffer so
    // the precompute runs only when weights actually change. Numeric (not string)
    // so the steady-state path allocates nothing. -1 = "no weights yet".
    const weightSigRef = useRef<number>(-1);
    // Once a caller drives this mesh imperatively (GraphManager's per-frame edge
    // hot loop, which applies node-type-visibility filtering), the `points` prop
    // becomes a stale, UNFILTERED snapshot. The prop-driven progressive reveal
    // must then yield to the imperative source — otherwise it re-populates the
    // mesh with filtered-out edges every frame, defeating visibility toggles.
    const imperativeActiveRef = useRef(false);
    // Read node reveal batch from settings to keep edge reveal in sync
    const nodeRevealBatch = (settings?.revealBatch as number | undefined) ?? 120;
    const EDGE_REVEAL_BATCH = Math.max(1, Math.round(nodeRevealBatch * 0.67));

    // `baseWidth` IS the cylinder radius, 1:1 — no halving, no hidden scale.
    // The slider value maps directly onto the tube radius the geometry is built at.
    const edgeRadius = settings?.baseWidth ?? 0.1;

    // --- Phase 6 (ADR-04 D1): dynamic capacity ---
    const renderingCeiling = useSettingsStore(s => s.settings?.visualisation?.rendering?.maxEdgesCeiling);
    const ceilingRef = useRef<number>(renderingCeiling ?? DEFAULT_EDGE_CEILING);
    // keep the ref in sync if the setting changes mid-session — the value is
    // only consulted at allocation time, so live-edit will affect the *next*
    // growth event rather than retroactively shrinking the buffer
    ceilingRef.current = renderingCeiling ?? DEFAULT_EDGE_CEILING;
    // Whether the ceiling is a configured value vs the hardcoded default
    // fallback. When configured, truncation at that ceiling is the user's
    // deliberate choice, so overflow is reported as a calm info line rather
    // than a warning. Held in a ref so ensureCapacity's callback reads the
    // current value without re-creating on every settings tick.
    const userSetCeilingRef = useRef<boolean>(renderingCeiling !== undefined);
    userSetCeilingRef.current = renderingCeiling !== undefined;

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
        console.warn(
          `[GlassEdges] capacity reallocated ${ts.length}x in last 60s — ` +
          `consider raising rendering.maxEdgesCeiling above current capacity ${targetCapacity}.`
        );
      }
      return nextMesh;
    }, [colorOverride, settings?.color, settings?.opacity, edgeRadius]);

    // Initial mesh allocation: small placeholder until first non-empty points.
    const { mesh, uniforms } = useMemo(() => {
      const { mesh: m, uniforms: u } = allocateMesh(
        EDGE_INITIAL,
        edgeRadius,
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
            edgeRadius,
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
      // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
    }, []);

    // Update material color and opacity when settings change.
    // When per-instance colors are active (updateColors was called),
    // keep base color white so instance colors come through pure.
    useEffect(() => {
      const m = meshRef.current ?? mesh;
      const mat = m.material as THREE.MeshPhysicalMaterial;
      // The base material.color MULTIPLIES the per-edge instanceColor. Once the
      // edge-type palette is driving instanceColor (updateColors called), the
      // base MUST stay neutral white (1,1,1) or it re-tints every tube and
      // collapses the 11-colour palette back into a single-hue wash. Only when
      // NO per-instance colours are active do we honour the flat edges.color
      // setting as a global tint (multiplied against the white-initialised buffer).
      if (mat.color) {
        if (instanceColorsActiveRef.current) {
          mat.color.setRGB(1, 1, 1);
        } else {
          const targetColor = colorOverride || settings?.color;
          if (targetColor) mat.color.set(targetColor);
        }
      }
      // opacity drives the TSL opacityNode (materialOpacity * fresnelGain), so
      // mat.opacity must equal the setting verbatim — no high floor, no clamp.
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
      // Edges intentionally keep transmission 0 so the opacity/alpha channel
      // (driven by edges.opacity via the TSL opacityNode) has full authority
      // over edge visibility. The shared gemMaterial.transmission applies to
      // node gems, NOT edges — applying it here re-introduced the white,
      // opaque-ish weave (transmission masks alpha) that swamped the nodes.
      mat.transmission = 0;
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
          const hidden = edgeCount - ceiling;
          if (userSetCeilingRef.current) {
            // Configured ceiling — truncation is by design. One structured info
            // line (not a warning) so it reads as telemetry, not a fault.
            // eslint-disable-next-line no-console
            console.info(
              `[GlassEdges] edges=${edgeCount} ceiling=${ceiling} rendered=${ceiling} ` +
              `hidden=${hidden} (by-design cap; raise visualisation.rendering.maxEdgesCeiling to show more)`
            );
          } else {
            // Hardcoded-default fallback (setting absent) — nudge the operator
            // to configure a ceiling that fits their graph.
            console.warn(
              `[GlassEdges] edge count ${edgeCount} exceeds ceiling ${ceiling}. ` +
              `${hidden} edges will not be rendered. ` +
              `Raise rendering.maxEdgesCeiling to render all edges.`
            );
          }
        }
        return meshRef.current!;
      }

      const newCap = Math.min(Math.max(edgeCount, capacity * 2), ceiling);
      return reallocate(newCap);
    }, [reallocate]);

    // Recompute when points prop changes -- reset progressive reveal.
    // Skip entirely once an imperative caller owns the mesh: the prop is then a
    // stale unfiltered snapshot and must not clobber the imperative state.
    useEffect(() => {
      if (imperativeActiveRef.current) return;
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
        // Imperative source is now authoritative — disable prop-driven reveal.
        imperativeActiveRef.current = true;
        if (len < 6) {
          if (m.count !== 0) {
            m.count = 0;
            m.instanceMatrix.needsUpdate = true;
          }
          return;
        }
        const edgeCount = Math.floor(len / 6);
        const activeMesh = ensureCapacity(edgeCount);
        computeInstanceMatrices(activeMesh, newPts, capacityRef.current, undefined, len, radiusFactorsRef.current);
      },
      [ensureCapacity],
    );

    // Convert per-edge raw weights -> precomputed X/Z radius factors. The edge
    // hot loop (useEdgeBufferComputation) re-sends weights every frame, but the
    // weights are semantic and only change on graph-data/settings change. We
    // therefore precompute weightToRadiusFactor ONCE per actual change (guarded
    // by a cheap signature) so the per-frame matrix loop reads a flat factor by
    // index instead of running sqrt+clamp ~21411x/frame. The math is identical;
    // only the timing moved. Zero per-frame allocation once the factor buffer is
    // sized — it is reused in place across frames and only grown on count change.
    const updateWidths = useCallback((weights: Float32Array, count: number) => {
      if (count <= 0) {
        if (radiusFactorsRef.current !== null) {
          radiusFactorsRef.current = null;
          weightSigRef.current = -1;
        }
        return;
      }
      // Cheap numeric signature: fold count + a sparse sample of the weight buffer
      // into one accumulator. Weights are static between data changes, so this
      // skips the precompute on stable ticks with a full O(1)-amortised scan and
      // zero allocation (pure number arithmetic, no string concat).
      const step = count > 8 ? Math.floor(count / 8) : 1;
      let sig = count * 2654435761;
      for (let s = 0; s < count; s += step) {
        sig = (sig * 31 + weights[s] * 1000003) >>> 0;
      }
      sig = (sig * 31 + weights[count - 1] * 1000003) >>> 0;
      if (sig === weightSigRef.current && radiusFactorsRef.current && radiusFactorsRef.current.length >= count) {
        return; // weights unchanged — keep existing precomputed factors
      }
      weightSigRef.current = sig;
      let factors = radiusFactorsRef.current;
      if (!factors || factors.length < count) {
        factors = new Float32Array(count); // grow on change only, never per frame
        radiusFactorsRef.current = factors;
      }
      for (let i = 0; i < count; i++) factors[i] = weightToRadiusFactor(weights[i]);
    }, []);

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
          // Per-edge type colours now own edge hue. Force the base white so the
          // instanceColor multiply renders TRUE (a non-white base would re-tint
          // the whole palette into one colour — the original grey/white wash).
          // Emissive stays a neutral grey so the glow tracks each tube's own
          // instanceColor intensity rather than imposing a shared hue.
          mat.color.setRGB(1, 1, 1);
          const edgeEmissiveBase = glowSettingsRef.current?.edgeGlowStrength ?? 0.3;
          const normalizedEmissive = Math.min(edgeEmissiveBase / 3, 1.0);
          mat.emissive.setRGB(normalizedEmissive, normalizedEmissive, normalizedEmissive);
        }
      },
      [],
    );

    useImperativeHandle(ref, () => ({ updatePoints, updateColors, updateWidths }), [updatePoints, updateColors, updateWidths]);

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

      // Progressive edge reveal: ramp up each frame (initial prop-based load only).
      // Yields permanently once the imperative hot loop takes over.
      if (!imperativeActiveRef.current && m && edgeRevealRef.current < totalEdgesRef.current && points.length >= 6) {
        edgeRevealRef.current = Math.min(
          edgeRevealRef.current + EDGE_REVEAL_BATCH,
          totalEdgesRef.current,
        );
        computeInstanceMatrices(m, points, capacityRef.current, edgeRevealRef.current, undefined, radiusFactorsRef.current);
      }
    });

    // The mesh ref can change on capacity growth, so we render through a
    // <group> + <primitive> that reads the current ref each render.
    return <primitive object={meshRef.current ?? mesh} />;
  },
);

GlassEdges.displayName = 'GlassEdges';

export default GlassEdges;
