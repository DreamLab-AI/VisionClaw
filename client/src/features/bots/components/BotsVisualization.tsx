/**
 * BotsVisualization.tsx
 * Thin orchestrator: wires BotsDataContext → per-agent positions →
 * BotsNode + BotsEdges renderers.
 *
 * Responsibilities:
 *   - Consume `useBotsData()` context (no WS/polling logic here)
 *   - Maintain `positionsRef` (server positions or initial circle layout)
 *   - Resolve colour palette from settings
 *   - Render loading / error / empty states
 *   - Delegate all 3-D rendering to BotsNode / BotsEdges
 */
import React, { useRef, useEffect, useState, useMemo } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { Html } from '@react-three/drei';
import { BotsAgent, BotsEdge, BotsState, TokenUsage } from '../types/BotsTypes';
import { createLogger } from '../../../utils/loggerConfig';
import { useTelemetry } from '../../../telemetry/useTelemetry';
import { agentTelemetry } from '../../../telemetry/AgentTelemetry';
import { useSettingsStore } from '../../../store/settingsStore';
import { useBotsData } from '../contexts/BotsDataContext';
import { getVisionClawColors } from './BotsShared';
import { agentTrustKey } from '../agentIdentity';
import { BotsNode } from './BotsNode';
import { BotsEdges } from './BotsEdges';
import { useAgentTargetStore } from '../../../store/agentTargetStore';
import { sharedNodePositions, sharedNodeIdToIndexMap } from '../../graph/contexts/NodePositionContext';
import { resolveNodeWorldPosition } from '../../visualisation/cameraFocus';

const logger = createLogger('BotsVisualization');

// ---------------------------------------------------------------------------
// Main Visualization Component
// Note: pure rendering component — positions come from server physics via
// binary protocol. No client-side physics computation.
// ---------------------------------------------------------------------------
export const BotsVisualization: React.FC = () => {
  const settings = useSettingsStore(state => state.settings);
  // Per-swarm hue tint (control-centre Agents → Behaviour). Client-only, default on.
  const swarmTint = useSettingsStore(s => s.get<boolean>('visualisation.graphTypeVisuals.agent.swarmTint')) ?? true;
  const { botsData: contextBotsData } = useBotsData();
  const telemetry = useTelemetry('BotsVisualization');

  const [botsData, setBotsData] = useState<BotsState>({
    agents: new Map(),
    edges: new Map(),
    communications: [],
    tokenUsage: { total: 0, byAgent: {} },
    lastUpdate: 0,
  });
  const [isLoading, setIsLoading]     = useState(true);
  const [error]                       = useState<string | null>(null);
  const [_mcpConnected, setMcpConnected] = useState(false);

  // Positions keyed by agent ID — updated from server or assigned as initial circle layout
  const positionsRef = useRef<Map<string, THREE.Vector3>>(new Map());

  const colors = useMemo(
    () => getVisionClawColors(settings as unknown as Record<string, unknown> | undefined),
    [settings],
  );

  // Sync context data → local BotsState + positionsRef
  useEffect(() => {
    if (!contextBotsData) {
      logger.debug('[VISIONCLAW] No context data available yet');
      return;
    }

    logger.debug('[VISIONCLAW] Processing bots data from context', contextBotsData);
    setIsLoading(false);

    const agents = contextBotsData.agents || [];
    const agentMap = new Map<string, BotsAgent>();

    agents.forEach((agent, index) => {
      agentMap.set(agent.id, agent);

      agentTelemetry.logAgentAction(agent.id, agent.type, 'state_update', {
        status: agent.status, health: agent.health,
        cpuUsage: agent.cpuUsage, tokenRate: agent.tokenRate,
      });

      if (
        agent.position &&
        (agent.position.x !== undefined ||
         agent.position.y !== undefined ||
         agent.position.z !== undefined)
      ) {
        positionsRef.current.set(
          agent.id,
          new THREE.Vector3(
            agent.position.x || 0,
            agent.position.y || 0,
            agent.position.z || 0,
          ),
        );
      } else if (!positionsRef.current.has(agent.id)) {
        const radius = 25;
        const angle  = (index / agents.length) * Math.PI * 2;
        const height = (Math.random() - 0.5) * 15;
        const newPosition = new THREE.Vector3(
          Math.cos(angle) * radius,
          height,
          Math.sin(angle) * radius,
        );
        positionsRef.current.set(agent.id, newPosition);

        agentTelemetry.logThreeJSOperation('position_update', agent.id,
          { x: newPosition.x, y: newPosition.y, z: newPosition.z },
          undefined,
          { reason: 'initial_calculation', agentType: agent.type, index, totalAgents: agents.length },
        );
      }
    });

    const edges   = contextBotsData.edges || [];
    const edgeMap = new Map<string, BotsEdge>();
    edges.forEach((edge: BotsEdge) => edgeMap.set(edge.id, edge));

    const contextRecord = contextBotsData as unknown as Record<string, unknown>;
    setBotsData({
      agents: agentMap,
      edges:  edgeMap,
      communications: [],
      tokenUsage: (contextRecord.tokenUsage as TokenUsage | undefined) || { total: 0, byAgent: {} },
      lastUpdate: Date.now(),
    });

    setMcpConnected(agentMap.size > 0);

    agentTelemetry.logAgentAction('visualization', 'system', 'data_update', {
      agentCount: agentMap.size,
      edgeCount:  edgeMap.size,
      hasContextData: !!contextBotsData,
    });
  }, [contextBotsData]);

  // Resolve agent→target KG node positions each frame so BotsNodes can
  // apply a momentum nudge toward their working area.
  const agentTargets = useAgentTargetStore(s => s.targets);
  const nudgeTargetsRef = useRef<Map<string, THREE.Vector3>>(new Map());

  useFrame(() => {
    const targets = agentTargets;
    const nudges = nudgeTargetsRef.current;
    nudges.clear();
    if (targets.size === 0) return;
    const positions = sharedNodePositions;
    const indexMap = sharedNodeIdToIndexMap;
    if (!positions || indexMap.size === 0) return;
    for (const [agentWireId, targetNodeId] of targets) {
      const pos = resolveNodeWorldPosition(targetNodeId, indexMap, positions);
      if (pos) {
        nudges.set(String(agentWireId), new THREE.Vector3(pos.x, pos.y, pos.z));
      }
    }
  });

  // -------------------------------------------------------------------------
  // Render states
  // -------------------------------------------------------------------------
  if (error) {
    return (
      <Html center>
        <div style={{ color: '#E74C3C', padding: '20px', textAlign: 'center' }}>
          <h3>VisionClaw Error</h3>
          <p>{error}</p>
        </div>
      </Html>
    );
  }

  if (isLoading) {
    return (
      <Html center>
        <div style={{ color: '#F1C40F', padding: '20px', textAlign: 'center' }}>
          <h3>Loading VisionClaw...</h3>
          <p>Initializing hive mind visualization</p>
        </div>
      </Html>
    );
  }

  if (botsData.agents.size === 0) return null;

  // -------------------------------------------------------------------------
  // Main render
  // -------------------------------------------------------------------------
  return (
    <group>
      {/* Inter-agent collaboration edges — one THREE.LineSegments draw for the
          whole layer (see BotsEdges). Endpoints track live agent positions;
          per-edge colour/opacity/pulse ride the vertex colours. */}
      <BotsEdges
        edges={botsData.edges}
        agents={botsData.agents}
        positionsRef={positionsRef}
        color={colors.edge}
      />

      {/* Nodes */}
      {Array.from(botsData.agents.values()).map((node, index) => {
        const position = positionsRef.current.get(node.id);
        if (!position) return null;

        const nodeColor = colors.getAgentColor
          ? colors.getAgentColor(node.type)
          : ((colors as unknown as Record<string, string>)[node.type] || colors.coordinator);

        return (
          <BotsNode
            // COM-14 / WP-1: React identity follows the agent's trust key
            // (did:nostr when carried, else the task_id fallback), not a
            // transient array index.
            key={agentTrustKey(node)}
            agent={node}
            position={position}
            index={index}
            color={nodeColor}
            swarmTint={swarmTint}
            nudgeTargetsRef={nudgeTargetsRef}
          />
        );
      })}
    </group>
  );
};
