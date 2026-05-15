import React, { useMemo, useCallback, useEffect, useRef, forwardRef, useImperativeHandle } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import {
  createGlassEdgeMaterial,
  createGlassEdgeGeometry,
} from '../../../rendering/materials/GlassEdgeMaterial';
import { useSettingsStore } from '../../../store/settingsStore';
import type { GemMaterialSettings, GlowSettings } from '../../settings/config/settings';

const MAX_EDGES = 16_000;

interface GlassEdgesProps {
  points: number[];
  settings: any;
  colorOverride?: string;
}

export interface GlassEdgesHandle {
  updatePoints(points: number[], count?: number): void;
  /** Update per-instance colors. Array of [r,g,b] floats (0-1), 3 per edge. */
  updateColors(colors: Float32Array, count: number): void;
  /** Update per-instance alpha (opacity multiplier). Floats 0-1, 1 per edge. */
  updateAlphas(alphas: Float32Array, count: number): void;
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

/** Compute up to `limit` edge matrices. Returns total edge count.
 *  `dataLength` limits how many elements of `pts` to consider (avoids needing a sliced copy). */
function computeInstanceMatrices(mesh: THREE.InstancedMesh, pts: number[], limit?: number, dataLength?: number): number {
  const edgeCount = Math.min(Math.floor((dataLength ?? pts.length) / 6), MAX_EDGES);
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

export const GlassEdges = forwardRef<GlassEdgesHandle, GlassEdgesProps>(
  ({ points, settings, colorOverride }, ref) => {
    const meshRef = useRef<THREE.InstancedMesh | null>(null);
    const edgeRevealRef = useRef(0);
    const totalEdgesRef = useRef(0);
    const instanceColorsActiveRef = useRef(false);
    // Read node reveal batch from settings to keep edge reveal in sync
    const nodeRevealBatch = (settings?.revealBatch as number | undefined) ?? 120;
    const EDGE_REVEAL_BATCH = Math.max(1, Math.round(nodeRevealBatch * 0.67));

    const { mesh, uniforms } = useMemo(() => {
      // Resolve initial edge color: prefer override, then settings, then default
      const initialColor = colorOverride || settings?.color || undefined;
      const geo = createGlassEdgeGeometry(settings?.edgeRadius ?? 0.03);
      const result = createGlassEdgeMaterial(initialColor);

      // Apply initial opacity from settings if provided
      if (settings?.opacity !== undefined) {
        (result.material as THREE.MeshPhysicalMaterial).opacity = settings.opacity;
      }

      const m = new THREE.InstancedMesh(geo, result.material, MAX_EDGES);
      m.frustumCulled = false;
      m.count = 0;

      // Pre-allocate instanceColor buffer for per-edge-type coloring.
      // When populated via updateColors(), Three.js multiplies instance color
      // with the material base color. Set material color to white when
      // instance colors are active so the per-edge colors come through pure.
      const colorArray = new Float32Array(MAX_EDGES * 3);
      // Initialize to white (neutral multiply)
      for (let ci = 0; ci < colorArray.length; ci++) colorArray[ci] = 1.0;
      m.instanceColor = new THREE.InstancedBufferAttribute(colorArray, 3);

      // Pre-allocate instanceAlpha buffer for per-edge-type opacity (ADR-048
      // bridge edges should render semi-transparent so the tiers read as
      // separate layers). Attribute name 'instanceAlpha' is shared between
      // WebGL (onBeforeCompile injection) and WebGPU (TSL attribute node).
      const alphaArray = new Float32Array(MAX_EDGES);
      for (let ai = 0; ai < alphaArray.length; ai++) alphaArray[ai] = 1.0;
      geo.setAttribute('instanceAlpha', new THREE.InstancedBufferAttribute(alphaArray, 1));

      // Initial population — first batch only, rest via progressive reveal
      if (points.length >= 6) {
        computeInstanceMatrices(m, points, EDGE_REVEAL_BATCH);
      }

      meshRef.current = m;
      return { mesh: m, uniforms: result.uniforms };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Update material color and opacity when settings change.
    // When per-instance colors are active (updateColors was called),
    // keep base color white so instance colors come through pure.
    useEffect(() => {
      const mat = mesh.material as THREE.MeshPhysicalMaterial;
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
      const mat = mesh.material as THREE.MeshPhysicalMaterial;
      if (gemSettings.ior !== undefined) mat.ior = gemSettings.ior;
      if (gemSettings.transmission !== undefined) mat.transmission = gemSettings.transmission;
      mat.needsUpdate = true;
    }, [gemSettings, mesh]);

    // Recompute when points prop changes -- reset progressive reveal
    useEffect(() => {
      if (points.length >= 6) {
        totalEdgesRef.current = Math.min(Math.floor(points.length / 6), MAX_EDGES);
        edgeRevealRef.current = 0; // Reset for progressive reveal in useFrame
      } else {
        mesh.count = 0;
        totalEdgesRef.current = 0;
        mesh.instanceMatrix.needsUpdate = true;
      }
    }, [points, mesh]);

    // Dispose GPU resources on unmount.
    // R3F <primitive> never auto-disposes, so manual cleanup is required.
    useEffect(() => {
      return () => {
        if (mesh) {
          mesh.geometry?.dispose();
          if (mesh.material) {
            (mesh.material as THREE.Material).dispose();
          }
          mesh.dispose();
        }
      };
    }, [mesh]);

    // Imperative path for hot-loop updates from useFrame callers.
    // Always recompute instance matrices when called — the caller
    // (GraphManager useFrame) already controls call frequency.
    // Previous hash-based dedup (`${len}-first-last`) was too coarse:
    // it only sampled 3 values, causing edges to freeze when those
    // specific values were stable while interior edge positions changed.
    const updatePoints = useCallback(
      (newPts: number[], count?: number) => {
        const len = count ?? newPts.length;
        if (len < 6) {
          if (mesh.count !== 0) {
            mesh.count = 0;
            mesh.instanceMatrix.needsUpdate = true;
          }
          return;
        }
        computeInstanceMatrices(mesh, newPts, undefined, len);
      },
      [mesh],
    );

    // Update per-instance edge colors from a packed Float32Array [r,g,b, r,g,b, ...]
    const updateColors = useCallback(
      (colors: Float32Array, count: number) => {
        if (!mesh.instanceColor) return;
        const attr = mesh.instanceColor as THREE.InstancedBufferAttribute;
        const dst = attr.array as Float32Array;
        const len = Math.min(count * 3, dst.length, colors.length);
        dst.set(colors.subarray(0, len));
        attr.needsUpdate = true;
        // When per-instance colors are active, set base color to white
        // so the instance colors come through unmodified by material tint
        instanceColorsActiveRef.current = count > 0;
        const mat = mesh.material as THREE.MeshPhysicalMaterial;
        if (count > 0 && mat.color) {
          mat.color.setRGB(1, 1, 1);
          // Set emissive white so the per-instance color modulates the glow hue.
          // instanceColor is multiplied in, so white emissive + colored instance
          // = colored bloom of the right hue.
          mat.emissive.setRGB(1, 1, 1);
        }
      },
      [mesh],
    );

    // Update per-instance alpha (opacity multiplier). Written into the
    // `instanceAlpha` attribute; shader code in GlassEdgeMaterial multiplies
    // it into the final fragment alpha.
    const updateAlphas = useCallback(
      (alphas: Float32Array, count: number) => {
        const geom = mesh.geometry as THREE.BufferGeometry;
        const attr = geom.attributes.instanceAlpha as THREE.InstancedBufferAttribute | undefined;
        if (!attr) return;
        const dst = attr.array as Float32Array;
        const len = Math.min(count, dst.length, alphas.length);
        dst.set(alphas.subarray(0, len));
        attr.needsUpdate = true;
      },
      [mesh],
    );

    useImperativeHandle(ref, () => ({ updatePoints, updateColors, updateAlphas }), [updatePoints, updateColors, updateAlphas]);

    // Subtle emissive pulse on edges — base and amplitude driven by glow settings.
    // Emissive base raised from 0.15 → 0.6 so edges clear the bloom threshold
    // (default 0.3) and glow through the post-processing pipeline. The old
    // 0.15 base kept edges dim (~0.07 channel brightness) — below the threshold.
    useFrame(({ clock }) => {
      const mat = meshRef.current?.material as THREE.MeshPhysicalMaterial | undefined;
      if (mat) {
        const gs = glowSettingsRef.current;
        const edgeGlow = gs?.edgeGlowStrength ?? 1.0;
        const pulseSpeed = gs?.pulseSpeed ?? 0.8;
        const emissiveBase = 0.6 * edgeGlow;
        const emissiveAmplitude = 0.2 * edgeGlow;
        mat.emissiveIntensity = emissiveBase + Math.sin(clock.elapsedTime * pulseSpeed) * emissiveAmplitude;
      }

      // Progressive edge reveal: ramp up each frame (initial prop-based load only)
      if (edgeRevealRef.current < totalEdgesRef.current && points.length >= 6) {
        edgeRevealRef.current = Math.min(
          edgeRevealRef.current + EDGE_REVEAL_BATCH,
          totalEdgesRef.current,
        );
        computeInstanceMatrices(mesh, points, edgeRevealRef.current);
      }
    });

    return <primitive object={mesh} />;
  },
);

GlassEdges.displayName = 'GlassEdges';

export default GlassEdges;
