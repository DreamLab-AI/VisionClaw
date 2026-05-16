/**
 * AgentCapsule — pure-renderer component for agent nodes.
 *
 * Phase 6 (ADR-04 D9 / T8): standalone, side-effect-free node renderer.
 * Consumes a filtered slice of nodes (agent class only) and renders a
 * single InstancedMesh of unit capsules.
 *
 *   - Geometry : CapsuleGeometry r=0.3 h=0.6
 *   - Material : AgentCapsuleMaterial (emissive-tinted opaque)
 *
 * Contract — see CrystalOrb.tsx (identical discipline).
 *
 * Note (ADR-04 R5): the surface-to-surface offset formula uses scalar r=0.3
 * for capsule envelope approximation. The actual capsule extends r=0.3 from
 * its axis, with half-spheres on each end at h/2 = 0.3. Along the long axis
 * the envelope reaches r=0.3 + 0.3 = 0.6 from the centre. We accept the
 * scalar-r approximation per ADR-04 R5.
 */

import React, { useEffect, useMemo, useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { createAgentCapsuleGeometry, createAgentCapsuleMaterial } from '../../../rendering/materials/AgentCapsuleMaterial';

export interface AgentCapsuleProps {
  /** Live ref to position buffer [x,y,z, x,y,z, ...] backed by SAB. */
  positionsRef: React.MutableRefObject<Float32Array | null>;
  /** Visible instance count. May be less than the buffer's capacity. */
  count: number;
  /** Per-capsule scale multiplier; combined with the geometry's r=0.3 / h=0.6. */
  radius?: number;
}

// Module-scope pre-allocated temps (ADR-04 D10 / T5)
const _tmpMat = new THREE.Matrix4();
const _tmpPos = new THREE.Vector3();
const _tmpScale = new THREE.Vector3();
const _identityQuat = new THREE.Quaternion();

export const AgentCapsule: React.FC<AgentCapsuleProps> = ({ positionsRef, count, radius = 1.0 }) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);

  const { mesh, material } = useMemo(() => {
    const geo = createAgentCapsuleGeometry();
    const result = createAgentCapsuleMaterial();
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

AgentCapsule.displayName = 'AgentCapsule';

export default AgentCapsule;
