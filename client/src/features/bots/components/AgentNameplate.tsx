/**
 * AgentNameplate.tsx — the label above an agent's marker (ADR-2142 / ADR-2135).
 *
 * The anchor is the marker map's position and nothing else (`nameplateAnchor`). The text is the
 * `/bots/data` record's name when the roster knows the agent, else the name the agent's state frame carried
 * (the registry), else its wire id. Level of detail by camera distance as before (`nameplateTier`).
 */
import React, { useRef, useState } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';
import { useAgentTargetStore } from '../../../store/agentTargetStore';
import { useSettingsStore } from '../../../store/settingsStore';
import { agentMarkerPosition } from '../agentMarkerPass';
import { demoRoleLabel } from '../virtualAgents';
import type { BotsAgent } from '../types/BotsTypes';
import {
  computeNameplateTier, tickNameplateDensity, NAMEPLATE_DENSITY_LIMIT, NAMEPLATE_HYSTERESIS,
  NAMEPLATE_NAME_DISTANCE_FACTOR, type NameplateTier,
} from '../nameplateTier';

/** Where a nameplate sits: the agent's marker position, from the marker map. */
export function nameplateAnchor(wireId: number | string, out: { set(x: number, y: number, z: number): unknown }): boolean {
  return agentMarkerPosition(wireId, out);
}

/** The label text: the roster record's name, else the registry name, else the demo role, else the wire id. */
export function nameplateText(
  wireId: number,
  registryName: string | undefined,
  record: Pick<BotsAgent, 'name'> | undefined,
): string {
  return record?.name || registryName || demoRoleLabel(wireId) || `agent 0x${(wireId >>> 0).toString(16)}`;
}

export interface AgentNameplateProps {
  wireId: number;
  record?: BotsAgent;
  color: string;
}

const _p = new THREE.Vector3();

export const AgentNameplate: React.FC<AgentNameplateProps> = ({ wireId, record, color }) => {
  const ref = useRef<THREE.Group>(null);
  const camera = useThree(s => s.camera);
  const registryName = useAgentTargetStore(s => s.agents.get(wireId)?.name);
  const agentVisuals = useSettingsStore(s => s.settings?.visualisation?.graphTypeVisuals?.agent);
  const lod = agentVisuals?.nameplateLod ?? true;
  const fullDistance = agentVisuals?.nameplateFullDistance ?? 40;
  const [tier, setTier] = useState<NameplateTier>('full');
  const tierRef = useRef<NameplateTier>('full');

  useFrame((state) => {
    const g = ref.current;
    if (!g) return;
    const ok = nameplateAnchor(wireId, _p);
    g.visible = ok;
    if (!ok) return;
    g.position.copy(_p);
    let next: NameplateTier = 'full';
    if (lod) {
      const dist = camera.position.distanceTo(_p);
      const crowd = tickNameplateDensity(state.clock.elapsedTime, dist < fullDistance);
      next = computeNameplateTier(dist, tierRef.current, {
        fullDistance,
        nameDistance: fullDistance * NAMEPLATE_NAME_DISTANCE_FACTOR,
        hysteresis: NAMEPLATE_HYSTERESIS,
        capName: crowd > NAMEPLATE_DENSITY_LIMIT,
      });
    }
    if (next !== tierRef.current) { tierRef.current = next; setTier(next); }
  });

  const text = nameplateText(wireId, registryName, record);
  return (
    <group ref={ref} visible={false}>
      {tier !== 'hidden' && (
        <Html center style={{ pointerEvents: 'none', whiteSpace: 'nowrap', textAlign: 'center', transform: 'translateY(-26px)' }}>
          <div data-testid="agent-nameplate" style={{ color, fontSize: '11px', fontFamily: 'monospace', textShadow: '0 0 3px black' }}>
            <div>{text}</div>
            {tier === 'full' && record && <div style={{ fontSize: '9px', opacity: 0.8 }}>{record.type} · {record.status}</div>}
          </div>
        </Html>
      )}
    </group>
  );
};

export default AgentNameplate;
