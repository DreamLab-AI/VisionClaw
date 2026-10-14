import React, { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { useFrame } from '@react-three/fiber';
import { useAgentTargetStore } from '@/store/agentTargetStore';
import {
  VIRTUAL_ROLES,
  resolveVirtualAgentPosition,
  virtualWireId,
} from '@/features/bots/virtualAgents';
import type { BeamPositionResolver } from './TransientBeamsLayer';

/**
 * Markers for the shared demo's agents (ADR-2142).
 *
 * The six demo agents are not graph nodes: the server's director only sends their `0x23` action
 * frames. Each is drawn as a small sphere hovering beside the node it is working on, in its role
 * colour, and the beams start from the same point (`resolveAgentPosition` falls back to the same
 * resolver). One instanced mesh, updated in place every frame; an absent or expired agent is
 * scaled to zero. There is no demo branch in the renderer: this layer draws whatever virtual
 * agents the frames describe.
 */

const _pos = new THREE.Vector3();
const _m = new THREE.Matrix4();
const _q = new THREE.Quaternion();
const _s = new THREE.Vector3();
const _c = new THREE.Color();

interface Props {
  /** Resolve a target node's world position (the viewer's own lookup). */
  resolveNodePosition: BeamPositionResolver;
  /** Sphere radius in world units. */
  radius?: number;
}

export const VirtualAgentMarkers: React.FC<Props> = ({ resolveNodePosition, radius = 2.2 }) => {
  const meshRef = useRef<THREE.InstancedMesh>(null);
  const geometry = useMemo(() => new THREE.SphereGeometry(1, 16, 12), []);
  const material = useMemo(() => new THREE.MeshBasicMaterial({ toneMapped: false }), []);

  useEffect(() => {
    const mesh = meshRef.current;
    if (!mesh) return;
    VIRTUAL_ROLES.forEach((role, i) => mesh.setColorAt(i, _c.set(role.colour)));
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  }, []);

  useEffect(() => () => { geometry.dispose(); material.dispose(); }, [geometry, material]);

  useFrame(() => {
    const mesh = meshRef.current;
    if (!mesh) return;
    const state = useAgentTargetStore.getState();
    const now = Date.now();
    const resolve = (id: number, out: { x: number; y: number; z: number }) => {
      _pos.set(out.x, out.y, out.z);
      const ok = resolveNodePosition(id, _pos);
      out.x = _pos.x; out.y = _pos.y; out.z = _pos.z;
      return ok;
    };
    VIRTUAL_ROLES.forEach((role, i) => {
      const present = resolveVirtualAgentPosition(virtualWireId(role.suffix), _pos, state, resolve, now);
      _s.setScalar(present ? radius : 0);
      _m.compose(_pos, _q, _s);
      mesh.setMatrixAt(i, _m);
    });
    mesh.instanceMatrix.needsUpdate = true;
  });

  return (
    <instancedMesh
      ref={meshRef}
      args={[geometry, material, VIRTUAL_ROLES.length]}
      frustumCulled={false}
      name="virtual-agent-markers"
    />
  );
};
