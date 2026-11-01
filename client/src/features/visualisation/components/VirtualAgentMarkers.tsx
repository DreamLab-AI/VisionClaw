import React, { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { useFrame, type ThreeEvent } from '@react-three/fiber';
import { useAgentTargetStore } from '@/store/agentTargetStore';
import { useSettingsStore } from '@/store/settingsStore';
import { agentMarkers } from '@/features/bots/agentMarkerPass';
import { agentMarkerColour, agentWireId } from '@/features/bots/virtualAgents';
import { getVisionClawColors } from '@/features/bots/components/BotsShared';
import { useBotsDataOptional } from '@/features/bots/contexts/BotsDataContext';
import { agentMarkerSelection, indexAgentsByWire } from '@/features/bots/agentSelection';
import type { BotsAgent } from '@/features/bots/types/BotsTypes';

/**
 * One marker per registry agent (ADR-2142, ADR-2135).
 *
 * No agent is a graph node. Every agent, demo or real, is drawn as a small sphere at the position the marker
 * pass wrote for it (`agentMarkers`), beside the node it is working on, in its role or type colour, and the
 * beams start from the same point. One instanced mesh, grown by doubling, updated in place every frame.
 * A click selects the agent into the steering panel (R5): instance, then wire id, then roster record.
 */

const _m = new THREE.Matrix4();
const _q = new THREE.Quaternion();
const _s = new THREE.Vector3();
const _pos = new THREE.Vector3();
const _c = new THREE.Color();

const INITIAL_CAPACITY = 64;

interface Props {
  /** Sphere radius in world units. */
  radius?: number;
}

export const VirtualAgentMarkers: React.FC<Props> = ({ radius = 2.2 }) => {
  const meshRef = useRef<THREE.InstancedMesh>(null);
  const [capacity, setCapacity] = React.useState(INITIAL_CAPACITY);
  const geometry = useMemo(() => new THREE.SphereGeometry(1, 16, 12), []);
  const material = useMemo(() => new THREE.MeshBasicMaterial({ toneMapped: false }), []);
  /** instance index → agent wire id, rewritten every frame (what a click resolves through). */
  const slotIds = useRef<number[]>([]);

  const settings = useSettingsStore(s => s.settings);
  const typeColour = useMemo(() => getVisionClawColors(settings as unknown as Record<string, unknown> | undefined).getAgentColor, [settings]);
  const botsAgents = useBotsDataOptional()?.botsData?.agents;
  const byWire = useRef<Map<number, BotsAgent>>(new Map());
  byWire.current = useMemo(() => indexAgentsByWire(botsAgents), [botsAgents]);

  useEffect(() => () => { geometry.dispose(); material.dispose(); }, [geometry, material]);

  useFrame(() => {
    const mesh = meshRef.current;
    if (!mesh) return;
    const n = agentMarkers.positions.size;
    if (n > capacity) { setCapacity(c => { let k = c; while (k < n) k *= 2; return k; }); return; }
    let i = 0;
    slotIds.current.length = n;
    for (const [id, p] of agentMarkers.positions) {
      _pos.set(p.x, p.y, p.z);
      _s.setScalar(radius);
      _m.compose(_pos, _q, _s);
      mesh.setMatrixAt(i, _m);
      mesh.setColorAt(i, _c.set(agentMarkerColour(id, byWire.current.get(agentWireId(id))?.type, typeColour)));
      slotIds.current[i++] = id;
    }
    mesh.count = n;
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  });

  const onClick = (e: ThreeEvent<MouseEvent>) => {
    const id = e.instanceId === undefined ? undefined : slotIds.current[e.instanceId];
    if (id === undefined) return;
    e.stopPropagation();
    const record = byWire.current.get(agentWireId(id));
    const name = useAgentTargetStore.getState().agents.get(id)?.name;
    window.dispatchEvent(new CustomEvent('visionclaw:node-selected', { detail: agentMarkerSelection(id, record, name) }));
  };

  return (
    <instancedMesh
      key={capacity}
      ref={meshRef}
      args={[geometry, material, capacity]}
      frustumCulled={false}
      name="virtual-agent-markers"
      onClick={onClick}
    />
  );
};
