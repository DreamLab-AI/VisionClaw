/**
 * CrystalOrb — pure-renderer component for ontology nodes.
 *
 * Phase 6 (ADR-04 D9 / T8): standalone, side-effect-free node renderer.
 * Consumes a filtered slice of nodes (by class flag — typically the
 * ontology subset) and renders a single InstancedMesh of unit spheres.
 *
 *   - Geometry : SphereGeometry r=0.5
 *   - Material : CrystalOrbMaterial (PBR, high transmission, low roughness)
 *
 * Contract:
 *   - Does NOT compute positions. Reads from `positionsRef` (a
 *     SharedArrayBuffer-backed Float32Array view).
 *   - Does NOT own labels. `InstancedLabels` mounts alongside, not inside.
 *   - Does NOT own selection state. Selection lives in client state.
 *   - useFrame mutates only its own InstancedMesh — no external writes.
 *
 * Zero-alloc invariant (ADR-04 D10): all working vectors / matrices /
 * quaternions are pre-allocated at module scope. useFrame must not
 * construct typed arrays or THREE.* objects per frame.
 */

import React, { useEffect, useMemo, useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { createCrystalOrbGeometry, createCrystalOrbMaterial } from '../../../rendering/materials/CrystalOrbMaterial';

export interface CrystalOrbProps {
  /** Live ref to position buffer [x,y,z, x,y,z, ...] backed by SAB. */
  positionsRef: React.MutableRefObject<Float32Array | null>;
  /** Visible instance count. May be less than the buffer's capacity. */
  count: number;
  /** Per-orb scale multiplier; combined with the unit-r=0.5 geometry. */
  radius?: number;
}

// Module-scope pre-allocated temps (ADR-04 D10 / T5)
const _tmpMat = new THREE.Matrix4();
const _tmpPos = new THREE.Vector3();
const _tmpScale = new THREE.Vector3();
const _identityQuat = new THREE.Quaternion();

export const CrystalOrb: React.FC<CrystalOrbProps> = ({ positionsRef, count, radius = 1.0 }) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);

  const { mesh, material } = useMemo(() => {
    const geo = createCrystalOrbGeometry();
    const result = createCrystalOrbMaterial();
    const m = new THREE.InstancedMesh(geo, result.material, Math.max(count, 1));
    m.frustumCulled = false;
    m.count = 0;
    meshRef.current = m;
    return { mesh: m, material: result.material };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Dispose GPU resources on unmount.
  useEffect(() => {
    return () => {
      mesh.geometry.dispose();
      material.dispose();
      mesh.dispose();
    };
  }, [mesh, material]);

  useFrame(() => {
    const m = meshRef.current;
    if (!m) return;
    const positions = positionsRef.current;
    if (!positions || positions.length === 0) return;

    const renderCount = Math.min(count, positions.length / 3);
    _tmpScale.set(radius, radius, radius);

    for (let i = 0; i < renderCount; i++) {
      const i3 = i * 3;
      _tmpPos.set(positions[i3], positions[i3 + 1], positions[i3 + 2]);
      _tmpMat.compose(_tmpPos, _identityQuat, _tmpScale);
      m.setMatrixAt(i, _tmpMat);
    }
    m.count = renderCount;
    m.instanceMatrix.needsUpdate = true;
  });

  return <primitive object={mesh} />;
};

CrystalOrb.displayName = 'CrystalOrb';

export default CrystalOrb;
